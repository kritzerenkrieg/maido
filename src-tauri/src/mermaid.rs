//! Mermaid diagrams (H3): SVGs cached on disk by a hash of the source, the
//! Mermaid version and the theme; anything not cached renders in a hidden
//! helper webview with its own web process, which closes 3 s after its last
//! diagram so Mermaid's memory leaves with it.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

pub const LABEL: &str = "mermaid";
const VERSION: &str = env!("MAIDO_MERMAID_VERSION");
const IDLE_CLOSE: Duration = Duration::from_secs(3);
/// A helper that never answers must not leave diagrams waiting for ever.
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Serialize)]
pub struct Job {
    key: String,
    source: String,
    theme: String,
}

type Reply = Result<String, String>;

#[derive(Default)]
pub struct Mermaid {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    queue: VecDeque<Job>,
    waiting: HashMap<String, Vec<oneshot::Sender<Reply>>>,
    open: bool,
    /// Bumped by every new diagram, so a pending idle close knows to stand down.
    generation: u64,
}

fn cache_path(app: &AppHandle, key: &str) -> Option<PathBuf> {
    Some(app.path().app_cache_dir().ok()?.join("mermaid").join(format!("{key}.svg")))
}

impl Mermaid {
    /// The SVG for `source`, from the cache or the helper. `page` is the
    /// asking window's URL, which the helper's page is loaded relative to.
    pub async fn render(&self, app: &AppHandle, page: tauri::Url, source: String, theme: String) -> Reply {
        let theme = if theme == "dark" { "dark" } else { "default" }.to_string();
        let key = blake3::hash(format!("{VERSION}\0{theme}\0{source}").as_bytes()).to_hex()[..32].to_string();
        if let Some(svg) = cache_path(app, &key).and_then(|p| std::fs::read_to_string(p).ok()) {
            return Ok(svg);
        }
        let (tx, rx) = oneshot::channel();
        let open = {
            let mut inner = self.inner.lock().unwrap();
            inner.generation += 1;
            let waiters = inner.waiting.entry(key.clone()).or_default();
            waiters.push(tx);
            if waiters.len() == 1 {
                inner.queue.push_back(Job { key, source, theme });
            }
            std::mem::replace(&mut inner.open, true)
        };
        if open {
            let _ = app.emit_to(LABEL, "mermaid-jobs", ());
        } else if let Err(e) = open_helper(app, page) {
            self.inner.lock().unwrap().open = false;
            return Err(e);
        }
        match tokio::time::timeout(TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            _ => {
                // A helper that stopped answering would otherwise stay "open"
                // for ever and every later diagram would wait out its timeout.
                self.reset();
                if let Some(w) = app.get_webview_window(LABEL) {
                    let _ = w.destroy();
                }
                Err("Mermaid did not answer".into())
            }
        }
    }

    /// The helper asks for work.
    pub fn jobs(&self) -> Vec<Job> {
        self.inner.lock().unwrap().queue.drain(..).collect()
    }

    /// The helper finished one diagram.
    pub fn done(&self, app: &AppHandle, key: String, reply: Reply) {
        if let (Ok(svg), Some(path)) = (&reply, cache_path(app, &key)) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(path, svg);
        }
        let generation = {
            let mut inner = self.inner.lock().unwrap();
            for tx in inner.waiting.remove(&key).unwrap_or_default() {
                let _ = tx.send(reply.clone());
            }
            inner.generation
        };
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(IDLE_CLOSE).await;
            let mermaid = app.state::<Mermaid>();
            let mut inner = mermaid.inner.lock().unwrap();
            if inner.generation == generation && inner.queue.is_empty() && inner.waiting.is_empty() && inner.open {
                inner.open = false;
                if let Some(w) = app.get_webview_window(LABEL) {
                    let _ = w.destroy();
                }
            }
        });
    }

    /// The helper's window went away (closed above, or its process died).
    pub fn closed(&self) {
        self.reset();
    }

    fn reset(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.open = false;
        for (_, waiters) in inner.waiting.drain() {
            for tx in waiters {
                let _ = tx.send(Err("Mermaid stopped".into()));
            }
        }
        inner.queue.clear();
    }
}

fn open_helper(app: &AppHandle, mut page: tauri::Url) -> Result<(), String> {
    page.set_path("/mermaid-frame.html");
    page.set_query(None);
    page.set_fragment(None);
    // On Linux the page loads only after throttling is off; a hidden page's
    // timers otherwise run about once a second (20 diagrams: 31.8 s, not 5.5 s).
    let url = if cfg!(target_os = "linux") { WebviewUrl::External("about:blank".parse().unwrap()) } else { WebviewUrl::External(page.clone()) };
    let window = WebviewWindowBuilder::new(app, LABEL, url)
        .visible(false)
        .focused(false)
        .skip_taskbar(true)
        .build()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    window
        .with_webview(move |webview| {
            use webkit2gtk::WebViewExt;
            let view = webview.inner();
            unthrottle::apply(&view);
            view.load_uri(page.as_str());
        })
        .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "linux"))]
    let _ = window;
    Ok(())
}

/// WebKitGTK throttles timers and suspends animations in pages that are not
/// shown. WebKitGTK 2.42+ can switch that off per webview; the Rust bindings
/// predate the API.
#[cfg(target_os = "linux")]
mod unthrottle {
    use std::ffi::{CStr, c_char, c_void};

    use webkit2gtk::glib::object::ObjectType;

    const OFF: [&str; 3] = ["HiddenPageDOMTimerThrottling", "DOMTimersThrottling", "HiddenPageCSSAnimationSuspension"];

    #[link(name = "webkit2gtk-4.1")]
    unsafe extern "C" {
        fn webkit_web_view_get_settings(view: *mut c_void) -> *mut c_void;
        fn webkit_settings_get_all_features() -> *mut c_void;
        fn webkit_settings_set_feature_enabled(settings: *mut c_void, feature: *mut c_void, enabled: i32);
        fn webkit_feature_list_get_length(list: *mut c_void) -> usize;
        fn webkit_feature_list_get(list: *mut c_void, index: usize) -> *mut c_void;
        fn webkit_feature_list_unref(list: *mut c_void);
        fn webkit_feature_get_identifier(feature: *mut c_void) -> *const c_char;
    }

    pub fn apply(view: &webkit2gtk::WebView) {
        // SAFETY: `view` is a live WebKitWebView; the feature list is owned
        // here and released after use; features are borrowed from it.
        unsafe {
            let settings = webkit_web_view_get_settings(view.as_ptr() as *mut c_void);
            let list = webkit_settings_get_all_features();
            for i in 0..webkit_feature_list_get_length(list) {
                let feature = webkit_feature_list_get(list, i);
                let id = CStr::from_ptr(webkit_feature_get_identifier(feature));
                if OFF.iter().any(|off| id.to_bytes() == off.as_bytes()) {
                    webkit_settings_set_feature_enabled(settings, feature, 0);
                }
            }
            webkit_feature_list_unref(list);
        }
    }
}
