//! A workspace: the folder `maido <path>` opened, its open documents and its
//! file watcher (E1, E2). One per window.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, Weak, mpsc};
use std::time::Duration;

use ignore::WalkBuilder;
use ignore::gitignore::Gitignore;
use notify_debouncer_full::notify::{EventKind, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::doc::{Document, normalize, percent_decode};

/// Shown in the tree unless "all files" is on.
pub const DOC_EXTENSIONS: [&str; 4] = ["md", "markdown", "mmd", "mermaid"];

pub struct Workspace {
    pub root: PathBuf,
    /// The file `maido <file>` was started with, relative to `root`.
    pub initial: Option<String>,
    pub docs: Mutex<HashMap<String, Document>>,
    watcher: Mutex<Option<Arc<Mutex<Watcher>>>>,
    /// Every file, for quick open; dropped when files appear, go or move.
    files: Arc<Mutex<Option<Arc<Vec<String>>>>>,
    /// Counts searches; a search stops when a newer one starts.
    pub search: Arc<AtomicU64>,
}

type Watcher = Debouncer<notify_debouncer_full::notify::RecommendedWatcher, RecommendedCache>;

#[derive(Serialize)]
pub struct Entry {
    name: String,
    path: String,
    dir: bool,
}

#[derive(Clone, Serialize)]
struct FsChange {
    path: String,
    /// "change" or "remove".
    kind: &'static str,
    /// A file or folder appeared, disappeared or was renamed: the tree of its
    /// parent folder needs listing again.
    tree: bool,
}

impl Workspace {
    pub fn new(root: PathBuf, initial: Option<String>) -> Workspace {
        Workspace { root, initial, docs: Mutex::default(), watcher: Mutex::default(), files: Arc::default(), search: Arc::default() }
    }

    /// Every file in the workspace (listed once, then kept until the tree changes).
    pub fn files(&self) -> Arc<Vec<String>> {
        let mut files = self.files.lock().unwrap();
        files.get_or_insert_with(|| Arc::new(crate::find::list_files(&self.root))).clone()
    }

    /// The absolute path of `rel`, if it is inside the workspace (after
    /// following symlinks).
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        let path = normalize(&self.root.join(rel));
        let real = path.canonicalize().map_err(|e| format!("{rel}: {e}"))?;
        if !real.starts_with(&self.root) {
            return Err(format!("{rel} is outside the workspace"));
        }
        Ok(real)
    }

    pub fn relative(&self, path: &Path) -> Option<String> {
        let rel = path.strip_prefix(&self.root).ok()?;
        Some(rel.to_string_lossy().replace('\\', "/"))
    }

    /// One folder's entries: folders first, then files, by name; `.gitignore`
    /// respected, hidden files left out unless `all` is on (then everything
    /// but `.git`).
    pub fn list_dir(&self, rel: &str, all: bool) -> Result<Vec<Entry>, String> {
        let dir = self.resolve(rel)?;
        let mut entries: Vec<Entry> = walker(&dir)
            .hidden(!all)
            .max_depth(Some(1))
            .build()
            .filter_map(Result::ok)
            .filter(|e| e.depth() == 1)
            .filter_map(|e| {
                let dir = e.file_type()?.is_dir();
                if (!dir && !all && !is_doc(e.path())) || e.file_name() == ".git" {
                    return None;
                }
                Some(Entry { name: e.file_name().to_string_lossy().into(), path: self.relative(e.path())?, dir })
            })
            .collect();
        entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }

    /// A link `href` in the document `from`: the workspace file it points to
    /// and its `#anchor`, if the file exists.
    pub fn resolve_link(&self, from: &str, href: &str) -> Option<(String, Option<String>)> {
        let (path, anchor) = match href.split_once('#') {
            Some((p, a)) => (p, Some(percent_decode(a))),
            None => (href, None),
        };
        let path = percent_decode(path.split('?').next().unwrap_or(""));
        let base = if path.starts_with('/') { PathBuf::new() } else { Path::new(from).parent().unwrap_or(Path::new("")).to_path_buf() };
        let target = normalize(&base.join(path.trim_start_matches('/')));
        let full = self.resolve(&target.to_string_lossy()).ok()?;
        full.is_file().then(|| (self.relative(&full).unwrap_or_default(), anchor))
    }

    /// Watch every folder the tree could show (not ignored, not hidden), one
    /// level each, and tell the window `label` about changes as `fs-change`.
    pub fn watch(self: &Arc<Self>, app: AppHandle, label: String) {
        let (tx, rx) = mpsc::channel::<DebounceEventResult>();
        let Ok(debouncer) = new_debouncer(Duration::from_millis(150), None, tx) else { return };
        let watcher = Arc::new(Mutex::new(debouncer));
        *self.watcher.lock().unwrap() = Some(watcher.clone());
        let weak_watcher: Weak<Mutex<Watcher>> = Arc::downgrade(&watcher);
        let root = self.root.clone();
        let files = self.files.clone();
        std::thread::spawn(move || {
            watch_tree(&weak_watcher, &root);
            let ignore = Gitignore::new(root.join(".gitignore")).0;
            // Ends when the workspace drops its watcher, which drops `tx`.
            for result in rx {
                let Ok(events) = result else { continue };
                let mut seen = HashMap::new();
                for event in events {
                    let tree = matches!(event.kind, EventKind::Create(_) | EventKind::Remove(_))
                        || matches!(event.kind, EventKind::Modify(notify_debouncer_full::notify::event::ModifyKind::Name(_)));
                    if matches!(event.kind, EventKind::Access(_)) {
                        continue;
                    }
                    for path in &event.paths {
                        let Ok(rel) = path.strip_prefix(&root) else { continue };
                        if rel.components().any(|c| c.as_os_str().to_string_lossy().starts_with('.')) {
                            continue;
                        }
                        let is_dir = path.is_dir();
                        if ignore.matched_path_or_any_parents(path, is_dir).is_ignore() {
                            continue;
                        }
                        if is_dir && tree {
                            watch_tree(&weak_watcher, path);
                        }
                        let entry = seen.entry(rel.to_string_lossy().replace('\\', "/")).or_insert(false);
                        *entry |= tree;
                    }
                }
                if seen.values().any(|t| *t) {
                    *files.lock().unwrap() = None;
                }
                for (path, tree) in seen {
                    let kind = if root.join(&path).exists() { "change" } else { "remove" };
                    let _ = app.emit_to(label.as_str(), "fs-change", FsChange { path, kind, tree });
                }
            }
        });
    }
}

pub fn walker(dir: &Path) -> WalkBuilder {
    let mut w = WalkBuilder::new(dir);
    w.require_git(false).follow_links(false);
    w
}

fn watch_tree(watcher: &Weak<Mutex<Watcher>>, dir: &Path) {
    for entry in walker(dir).build().filter_map(Result::ok) {
        if entry.file_type().is_some_and(|t| t.is_dir()) {
            let Some(watcher) = watcher.upgrade() else { return };
            let _ = watcher.lock().unwrap().watch(entry.path(), RecursiveMode::NonRecursive);
        }
    }
}

pub fn is_doc(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| DOC_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
}

/// `maido <path>`: a folder opens as the workspace; a file opens with its
/// folder as the workspace; nothing means the current folder.
pub fn target(arg: Option<&str>, cwd: &Path) -> Result<(PathBuf, Option<String>), String> {
    let path = cwd.join(arg.unwrap_or("."));
    let path = path.canonicalize().map_err(|e| format!("maido: {}: {e}", path.display()))?;
    if path.is_dir() {
        return Ok((path, None));
    }
    let root = path.parent().ok_or("maido: no parent folder")?.to_path_buf();
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned());
    Ok((root, file))
}

/// Tabs, scroll positions and toggles per workspace, kept by the frontend
/// as opaque JSON in the app's data folder (E1).
pub struct Sessions {
    path: PathBuf,
    lock: Mutex<()>,
}

impl Sessions {
    pub fn new(path: PathBuf) -> Sessions {
        Sessions { path, lock: Mutex::new(()) }
    }

    fn read(&self) -> serde_json::Map<String, serde_json::Value> {
        std::fs::read_to_string(&self.path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn get(&self, key: &str) -> serde_json::Value {
        let _guard = self.lock.lock().unwrap();
        self.read().remove(key).unwrap_or(serde_json::Value::Null)
    }

    pub fn set(&self, key: &str, value: serde_json::Value) -> Result<(), String> {
        let _guard = self.lock.lock().unwrap();
        let mut all = self.read();
        all.insert(key.to_string(), value);
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_string(&all).unwrap_or_default()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &self.path).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_workspace() -> (PathBuf, Workspace) {
        let dir = std::env::temp_dir().join(format!("maido-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("docs/sub")).unwrap();
        std::fs::create_dir_all(dir.join("build")).unwrap();
        for f in ["README.md", "docs/a.md", "docs/sub/b.markdown", "docs/flow.mmd", "notes.txt", "build/out.md"] {
            std::fs::write(dir.join(f), "x").unwrap();
        }
        std::fs::write(dir.join(".gitignore"), "build/\n").unwrap();
        std::fs::write(dir.join(".env"), "x").unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let root = dir.canonicalize().unwrap();
        (dir, Workspace::new(root, None))
    }

    #[test]
    fn tree_links_and_targets() {
        let (dir, ws) = temp_workspace();
        let names = |rel: &str, all: bool| ws.list_dir(rel, all).unwrap().into_iter().map(|e| e.name).collect::<Vec<_>>();
        assert_eq!(names("", false), ["docs", "README.md"]);
        // Hidden files show with "all files" (never `.git`); a plain tree leaves them out.
        assert_eq!(names("", true), ["docs", ".env", ".gitignore", "notes.txt", "README.md"]);
        assert_eq!(names("docs", false), ["sub", "a.md", "flow.mmd"]);

        assert_eq!(ws.resolve_link("docs/a.md", "sub/b.markdown#Top%20part"), Some(("docs/sub/b.markdown".into(), Some("Top part".into()))));
        assert_eq!(ws.resolve_link("docs/a.md", "../README.md"), Some(("README.md".into(), None)));
        assert_eq!(ws.resolve_link("docs/a.md", "/README.md"), Some(("README.md".into(), None)));
        assert_eq!(ws.resolve_link("docs/a.md", "../../etc/passwd"), None);
        assert_eq!(ws.resolve_link("docs/a.md", "missing.md"), None);
        assert!(ws.resolve("../").is_err());

        let (root, file) = target(Some("docs/a.md"), &dir).unwrap();
        assert_eq!((root, file.as_deref()), (ws.root.join("docs"), Some("a.md")));
        assert_eq!(target(None, &dir).unwrap(), (ws.root.clone(), None));
        assert!(target(Some("nope"), &dir).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
