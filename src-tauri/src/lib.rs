mod diff;
mod doc;
mod files;
mod find;
mod highlight;
mod mermaid;
mod pty;
mod update;
mod workspace;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::ipc::{Channel, Response};
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};

use doc::Document;
use mermaid::Mermaid;
use pty::Terminals;
use update::Updates;
use workspace::{Entry, Sessions, Workspace};

/// Files larger than this are not opened (a 5 MB document parses in 300 ms).
const MAX_FILE: u64 = 20 * 1024 * 1024;

/// Each document window's workspace, by window label.
#[derive(Default)]
struct Windows(Mutex<HashMap<String, Arc<Workspace>>>);

impl Windows {
    fn get(&self, window: &WebviewWindow) -> Result<Arc<Workspace>, String> {
        self.0.lock().unwrap().get(window.label()).cloned().ok_or_else(|| "no workspace for this window".into())
    }
}

#[derive(Serialize)]
struct WorkspaceInfo {
    root: String,
    name: String,
    initial: Option<String>,
    /// Folders added beside the workspace's own.
    roots: Vec<RootInfo>,
}

#[derive(Serialize)]
struct RootInfo {
    id: u32,
    name: String,
    path: String,
}

fn root_info(e: workspace::Extra) -> RootInfo {
    RootInfo { id: e.id, name: folder_name(&e.path), path: e.path.to_string_lossy().into_owned() }
}

/// The folders added to the workspace on `root`, as last saved.
fn saved_roots(sessions: &Sessions, root: &Path) -> Vec<workspace::Extra> {
    let saved = sessions.get(&format!("roots:{}", root.to_string_lossy()));
    let list: Vec<(u32, String)> = serde_json::from_value::<Vec<serde_json::Value>>(saved)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| Some((v.get("id")?.as_u64()? as u32, v.get("path")?.as_str()?.to_string())))
        .collect();
    list.into_iter().filter_map(|(id, p)| Some(workspace::Extra { id, path: PathBuf::from(p).canonicalize().ok().filter(|p| p.is_dir())? })).collect()
}

fn save_roots(sessions: &Sessions, ws: &Workspace) -> Result<(), String> {
    let list: Vec<_> = ws.extras().iter().map(|e| serde_json::json!({ "id": e.id, "path": e.path })).collect();
    sessions.set(&format!("roots:{}", ws.root.to_string_lossy()), list.into())
}

/// A workspace on `root` with the folders saved for it; their files may be
/// shown as images.
fn new_workspace(app: &AppHandle, root: PathBuf, initial: Option<String>) -> Arc<Workspace> {
    let extras = saved_roots(&app.state::<Sessions>(), &root);
    for e in &extras {
        let _ = app.asset_protocol_scope().allow_directory(&e.path, true);
    }
    let _ = app.asset_protocol_scope().allow_directory(&root, true);
    Arc::new(Workspace::new(root, initial, extras))
}

/// A workspace's name: its folder's name (or the whole path for `/`).
fn folder_name(root: &Path) -> String {
    root.file_name().map_or_else(|| root.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned())
}

#[tauri::command]
fn workspace_info(window: WebviewWindow, windows: State<Windows>) -> Result<WorkspaceInfo, String> {
    let ws = windows.get(&window)?;
    Ok(WorkspaceInfo { root: ws.root.to_string_lossy().into_owned(), name: folder_name(&ws.root), initial: ws.initial.clone(), roots: ws.extras().into_iter().map(root_info).collect() })
}

#[tauri::command]
async fn list_dir(window: WebviewWindow, windows: State<'_, Windows>, path: String, all: bool) -> Result<Vec<Entry>, String> {
    windows.get(&window)?.list_dir(&path, all)
}

#[derive(Serialize)]
struct BlockView {
    key: String,
    line: usize,
    end: usize,
    /// Left out when the frontend already has this key on screen.
    html: Option<Arc<str>>,
    code: bool,
    math: bool,
}

#[derive(Serialize)]
struct OutlineItem {
    level: u8,
    id: String,
    text: String,
    line: usize,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum DocView {
    Markdown { blocks: Vec<BlockView>, outline: Vec<OutlineItem>, remote_images: usize },
    Mermaid { source: String },
    Text { text: String },
    Binary,
}

/// A workspace file as text: its path, its text (None for a binary file)
/// and a hash of its bytes, which saves compare against (E3).
fn read_file(ws: &Workspace, path: &str) -> Result<(PathBuf, Option<String>, String), String> {
    let full = ws.resolve(path)?;
    let meta = std::fs::metadata(&full).map_err(|e| e.to_string())?;
    if meta.len() > MAX_FILE {
        return Err(format!("{path} is {} MB; maido opens files up to 20 MB", meta.len() / 1024 / 1024));
    }
    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
    let hash = blake3::hash(&bytes).to_hex()[..16].to_string();
    if bytes.contains(&0) {
        return Ok((full, None, hash));
    }
    let text = String::from_utf8_lossy(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes)).into_owned();
    Ok((full, Some(text), hash))
}

/// `text` (the file on disk, or the editor's buffer) as the frontend shows it.
fn view(ws: &Workspace, path: &str, full: &Path, text: String, have: Vec<String>) -> DocView {
    let ext = full.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mmd" | "mermaid" => return DocView::Mermaid { source: text },
        "md" | "markdown" => {}
        _ => return DocView::Text { text },
    }

    let mut docs = ws.docs.lock().unwrap();
    let doc = match docs.get_mut(path) {
        Some(d) => {
            d.update(text, full, &ws.root_for(full));
            d
        }
        None => docs.entry(path.to_string()).or_insert_with(|| Document::parse(text, full, &ws.root_for(full))),
    };
    let have: std::collections::HashSet<String> = have.into_iter().collect();
    let mut seen: HashMap<&str, usize> = HashMap::new();
    let mut blocks = Vec::with_capacity(doc.blocks.len());
    let mut outline = Vec::new();
    let mut remote_images = 0;
    for b in &doc.blocks {
        let n = seen.entry(&b.hash).or_default();
        let key = format!("{}.{n}", b.hash);
        *n += 1;
        if let Some(h) = &b.heading {
            outline.push(OutlineItem { level: h.level, id: h.id.clone(), text: h.text.clone(), line: b.line });
        }
        remote_images += b.remote_images;
        let html = (!have.contains(&key)).then(|| b.html.clone());
        blocks.push(BlockView { key, line: b.line, end: b.end, html, code: !b.code.is_empty(), math: b.math });
    }
    DocView::Markdown { blocks, outline, remote_images }
}

/// Read `path` and return it as blocks (re-parsing only from the change on, H5).
/// `have` lists block keys already on screen, whose HTML is not sent again.
#[tauri::command]
async fn load_document(window: WebviewWindow, windows: State<'_, Windows>, path: String, have: Vec<String>) -> Result<DocView, String> {
    let ws = windows.get(&window)?;
    match read_file(&ws, &path)? {
        (full, Some(text), _) => Ok(view(&ws, &path, &full, text, have)),
        (_, None, _) => Ok(DocView::Binary),
    }
}

/// The editor's live preview (B5): the unsaved buffer, through the same
/// blocks and re-parse as the file on disk.
#[tauri::command]
async fn preview_document(window: WebviewWindow, windows: State<'_, Windows>, path: String, text: String, have: Vec<String>) -> Result<DocView, String> {
    let ws = windows.get(&window)?;
    let full = ws.resolve(&path)?;
    Ok(view(&ws, &path, &full, text, have))
}

#[derive(Serialize)]
struct Source {
    text: String,
    hash: String,
}

/// A file's text for the editor, with the hash its save is checked against.
#[tauri::command]
async fn load_source(window: WebviewWindow, windows: State<'_, Windows>, path: String) -> Result<Source, String> {
    let ws = windows.get(&window)?;
    match read_file(&ws, &path)? {
        (_, Some(text), hash) => Ok(Source { text, hash }),
        (_, None, _) => Err(format!("{path} is a binary file")),
    }
}

/// Save the editor's text. Refused with "conflict" when the file changed on
/// disk since the editor loaded it (`base`), unless `force` (E3): nothing is
/// overwritten silently. Written beside the file and renamed over it, so a
/// crash never leaves half a file. Returns the new hash.
#[tauri::command]
async fn save_file(window: WebviewWindow, windows: State<'_, Windows>, path: String, text: String, base: String, force: bool) -> Result<String, String> {
    let ws = windows.get(&window)?;
    let (full, _, current) = read_file(&ws, &path)?;
    if current != base && !force {
        return Err("conflict".into());
    }
    let name = full.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
    let tmp = full.with_file_name(format!(".{name}.maido-save"));
    let perms = std::fs::metadata(&full).map_err(|e| e.to_string())?.permissions();
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| e.to_string())?;
    let _ = std::fs::set_permissions(&tmp, perms);
    std::fs::rename(&tmp, &full).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(blake3::hash(text.as_bytes()).to_hex()[..16].to_string())
}

/// A new empty file, or a folder, in the workspace folder `dir` (N4).
#[tauri::command]
async fn fs_create(window: WebviewWindow, windows: State<'_, Windows>, dir: String, name: String, folder: bool) -> Result<String, String> {
    let ws = windows.get(&window)?;
    files::create(&ws, &dir, &name, folder)
}

#[tauri::command]
async fn fs_rename(window: WebviewWindow, windows: State<'_, Windows>, path: String, name: String) -> Result<String, String> {
    let ws = windows.get(&window)?;
    files::rename(&ws, &path, &name)
}

#[tauri::command]
async fn fs_trash(window: WebviewWindow, windows: State<'_, Windows>, paths: Vec<String>) -> Result<(), String> {
    let ws = windows.get(&window)?;
    files::trash(&ws, &paths)
}

#[tauri::command]
async fn fs_paste(window: WebviewWindow, windows: State<'_, Windows>, paths: Vec<String>, dir: String, cut: bool) -> Result<Vec<String>, String> {
    let ws = windows.get(&window)?;
    files::paste(&ws, &paths, &dir, cut)
}

/// The Edit menu's clipboard (N2): WebKitGTK does not let a page read it.
#[tauri::command]
fn clipboard_read(app: AppHandle) -> Result<String, String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().read_text().map_err(|e| e.to_string())
}

#[tauri::command]
fn clipboard_write(app: AppHandle, text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard().write_text(text).map_err(|e| e.to_string())
}

/// File > Open folder (N2): pick a folder in the system dialog and show it in
/// this window (its terminals end; the page reloads on `true`). A folder open
/// in another window is brought forward there instead.
#[tauri::command]
async fn open_folder(app: AppHandle, window: WebviewWindow, windows: State<'_, Windows>, path: Option<String>) -> Result<bool, String> {
    use tauri_plugin_dialog::DialogExt;
    let ws = windows.get(&window)?;
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            window.dialog().file().set_parent(&window).set_title("Select folder").set_directory(&ws.root).pick_folder(move |path| {
                let _ = tx.send(path);
            });
            let Some(path) = rx.await.map_err(|e| e.to_string())? else { return Ok(false) };
            path.into_path().map_err(|e| e.to_string())?
        }
    };
    let root = path.canonicalize().map_err(|e| format!("{}: {e}", path.display()))?;
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    if root == ws.root {
        return Ok(false);
    }
    if let Some(label) = windows.0.lock().unwrap().iter().find(|(_, w)| w.root == root).map(|(l, _)| l.clone())
        && let Some(w) = app.get_webview_window(&label)
    {
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(false);
    }
    app.state::<Terminals>().close_window(window.label());
    let ws = new_workspace(&app, root.clone(), None);
    windows.0.lock().unwrap().insert(window.label().to_string(), ws.clone());
    let _ = window.set_title(&format!("{} — maido", folder_name(&root)));
    ws.watch(app.clone(), window.label().to_string());
    Ok(true)
}

/// File > Add folder to workspace: shows the folder dialog and adds the
/// choice beside the workspace's own folder. Returns it, or nothing when the
/// dialog was cancelled.
#[tauri::command]
async fn add_root(app: AppHandle, window: WebviewWindow, windows: State<'_, Windows>, sessions: State<'_, Sessions>, path: Option<String>) -> Result<Option<RootInfo>, String> {
    use tauri_plugin_dialog::DialogExt;
    let ws = windows.get(&window)?;
    let path = match path {
        Some(p) => PathBuf::from(p),
        None => {
            let (tx, rx) = tokio::sync::oneshot::channel();
            window.dialog().file().set_parent(&window).set_title("Add folder to workspace").set_directory(&ws.root).pick_folder(move |path| {
                let _ = tx.send(path);
            });
            let Some(path) = rx.await.map_err(|e| e.to_string())? else { return Ok(None) };
            path.into_path().map_err(|e| e.to_string())?
        }
    };
    let extra = ws.add_root(path)?;
    let _ = app.asset_protocol_scope().allow_directory(&extra.path, true);
    save_roots(&sessions, &ws)?;
    Ok(Some(root_info(extra)))
}

/// Take an added folder out of the workspace (nothing on disk changes).
#[tauri::command]
fn remove_root(window: WebviewWindow, windows: State<Windows>, sessions: State<Sessions>, id: u32) -> Result<(), String> {
    let ws = windows.get(&window)?;
    ws.remove_root(id);
    ws.docs.lock().unwrap().retain(|k, _| !k.starts_with(&format!("@{id}/")));
    save_roots(&sessions, &ws)
}

/// One side of the diff window: a workspace file or pasted text (E5).
#[derive(serde::Deserialize)]
struct DiffSide {
    path: Option<String>,
    text: Option<String>,
}

impl DiffSide {
    fn read(&self, ws: &Workspace) -> Result<String, String> {
        match (&self.path, &self.text) {
            (_, Some(text)) => Ok(text.clone()),
            (Some(path), None) => read_file(ws, path)?.1.ok_or_else(|| format!("{path} is a binary file")),
            (None, None) => Ok(String::new()),
        }
    }
}

#[tauri::command]
async fn diff_lines(window: WebviewWindow, windows: State<'_, Windows>, left: DiffSide, right: DiffSide) -> Result<Vec<diff::Row>, String> {
    let ws = windows.get(&window)?;
    Ok(diff::lines(&left.read(&ws)?, &right.read(&ws)?))
}

/// Two Markdown texts compared as rendered, block by block. Images resolve
/// from each side's file, or the workspace folder for pasted text.
#[tauri::command]
async fn diff_rendered(window: WebviewWindow, windows: State<'_, Windows>, left: DiffSide, right: DiffSide) -> Result<Vec<diff::BlockRow>, String> {
    let ws = windows.get(&window)?;
    let parse = |side: &DiffSide| -> Result<Document, String> {
        let at = match &side.path {
            Some(p) => ws.resolve(p)?,
            None => ws.root.join("pasted.md"),
        };
        let root = ws.root_for(&at);
        Ok(Document::parse(side.read(&ws)?, &at, &root))
    };
    let (a, b) = (parse(&left)?, parse(&right)?);
    Ok(diff::blocks(&a.blocks, &b.blocks))
}

/// A tab closed: forget the document.
#[tauri::command]
fn close_document(window: WebviewWindow, windows: State<Windows>, path: String) -> Result<(), String> {
    windows.get(&window)?.docs.lock().unwrap().remove(&path);
    Ok(())
}

/// Highlighted `<code>` contents for each fenced block with a language in the
/// block `key` (by document order), None where the language is unknown.
#[tauri::command]
async fn highlight_block(window: WebviewWindow, windows: State<'_, Windows>, path: String, key: String) -> Result<Vec<Option<String>>, String> {
    let ws = windows.get(&window)?;
    let hash = key.split('.').next().unwrap_or("");
    let code = {
        let docs = ws.docs.lock().unwrap();
        let doc = docs.get(&path).ok_or("document not open")?;
        doc.blocks.iter().find(|b| b.hash == hash).map(|b| b.code.clone()).unwrap_or_default()
    };
    Ok(code.iter().map(|(lang, code)| highlight::highlight(lang, code)).collect())
}

#[tauri::command]
fn highlight_css() -> String {
    highlight::css()
}

#[derive(Serialize)]
struct Link {
    path: String,
    anchor: Option<String>,
}

#[tauri::command]
fn resolve_link(window: WebviewWindow, windows: State<Windows>, from: String, href: String) -> Result<Option<Link>, String> {
    Ok(windows.get(&window)?.resolve_link(&from, &href).map(|(path, anchor)| Link { path, anchor }))
}

/// Web and mail links open outside maido; nothing else does (F3).
#[tauri::command]
fn open_external(app: AppHandle, url: String) -> Result<(), String> {
    let lower = url.to_ascii_lowercase();
    if !["https://", "http://", "mailto:"].iter().any(|s| lower.starts_with(s)) {
        return Err(format!("not opening {url}"));
    }
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command]
fn load_session(window: WebviewWindow, windows: State<Windows>, sessions: State<Sessions>) -> Result<serde_json::Value, String> {
    Ok(sessions.get(&windows.get(&window)?.root.to_string_lossy()))
}

#[tauri::command]
fn save_session(window: WebviewWindow, windows: State<Windows>, sessions: State<Sessions>, session: serde_json::Value) -> Result<(), String> {
    sessions.set(&windows.get(&window)?.root.to_string_lossy(), session)
}

/// App-wide settings (the remote-image answer, F4) live in the same file
/// under an empty key, which no workspace path can be.
#[tauri::command]
fn load_settings(sessions: State<Sessions>) -> serde_json::Value {
    sessions.get("")
}

#[tauri::command]
fn save_settings(sessions: State<Sessions>, settings: serde_json::Value) -> Result<(), String> {
    sessions.set("", settings)
}

#[tauri::command]
async fn render_mermaid(app: AppHandle, window: WebviewWindow, mermaid: State<'_, Mermaid>, source: String, theme: String) -> Result<String, String> {
    let page = window.url().map_err(|e| e.to_string())?;
    mermaid.render(&app, page, source, theme).await
}

#[tauri::command]
fn mermaid_jobs(window: WebviewWindow, mermaid: State<Mermaid>) -> Vec<mermaid::Job> {
    if window.label() != mermaid::LABEL {
        return Vec::new();
    }
    mermaid.jobs()
}

#[tauri::command]
fn mermaid_done(app: AppHandle, window: WebviewWindow, mermaid: State<Mermaid>, key: String, svg: Option<String>, error: Option<String>) {
    if window.label() != mermaid::LABEL {
        return;
    }
    mermaid.done(&app, key, svg.ok_or_else(|| error.unwrap_or_else(|| "Mermaid failed".into())));
}

/// Save an export (HTML, SVG, PNG; the bytes are the request body) where you
/// choose in the system's save dialog, starting in the folder of the
/// workspace file named by the `x-near` header (L1). The only way maido writes
/// outside the workspace, and only to the path you picked. Returns that path,
/// or None if you cancelled.
#[tauri::command]
async fn export_file(window: WebviewWindow, windows: State<'_, Windows>, request: tauri::ipc::Request<'_>) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    let tauri::ipc::InvokeBody::Raw(data) = request.body() else { return Err("export: expected bytes".into()) };
    let data = data.clone();
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(doc::percent_decode);
    let name = header("x-name").unwrap_or_default();
    let name = Path::new(&name).file_name().map_or_else(|| "export".into(), |n| n.to_string_lossy().into_owned());
    let ext = Path::new(&name).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let ws = windows.get(&window)?;
    let dir = header("x-near").and_then(|n| ws.resolve(&n).ok()).and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_else(|| ws.root.clone());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = window.dialog().file().set_parent(&window).set_title("Export").set_directory(dir).set_file_name(&name);
    if !ext.is_empty() {
        dialog = dialog.add_filter(ext.to_uppercase(), &[ext.as_str()]);
    }
    dialog.save_file(move |path| {
        let _ = tx.send(path);
    });
    let Some(path) = rx.await.map_err(|e| e.to_string())? else { return Ok(None) };
    let path = path.into_path().map_err(|e| e.to_string())?;
    std::fs::write(&path, data).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// A workspace image as a `data:` URL, for HTML export (images are embedded so
/// the file stands alone). Only images, only inside the workspace.
#[tauri::command]
async fn image_data(window: WebviewWindow, windows: State<'_, Windows>, path: String) -> Result<String, String> {
    use base64::Engine;
    let ws = windows.get(&window)?;
    let rel = ws.relative(Path::new(&path)).ok_or("not in the workspace")?;
    let full = ws.resolve(&rel)?;
    let ext = full.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        _ => return Err(format!("{rel}: not an image maido embeds")),
    };
    let bytes = std::fs::read(&full).map_err(|e| e.to_string())?;
    Ok(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// The system print dialog for this window ("Print to file" makes a PDF, L2).
#[tauri::command]
fn print_page(window: WebviewWindow) -> Result<(), String> {
    window.print().map_err(|e| e.to_string())
}

/// Quick open (Ctrl+P): the best matches for `query` among the workspace's files.
#[tauri::command]
async fn quick_open(window: WebviewWindow, windows: State<'_, Windows>, query: String) -> Result<Vec<find::FileMatch>, String> {
    let ws = windows.get(&window)?;
    Ok(find::quick_open(&ws.files(), &query, 50))
}

/// Full-text search; results arrive on `results` file by file. Starting a
/// search (an empty one too) stops the one before it.
#[tauri::command]
fn search(window: WebviewWindow, windows: State<Windows>, query: String, results: Channel<find::SearchEvent>) -> Result<(), String> {
    let ws = windows.get(&window)?;
    let mine = ws.search.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if query.is_empty() {
        return Ok(());
    }
    let (roots, generation) = (ws.roots(), ws.search.clone());
    std::thread::spawn(move || {
        find::search(&roots, &query, &generation, mine, |e| {
            let _ = results.send(e);
        })
    });
    Ok(())
}

/// A new terminal session in the workspace folder; its output arrives on `output`.
#[tauri::command]
fn term_open(window: WebviewWindow, windows: State<Windows>, terminals: State<Terminals>, cols: u16, rows: u16, output: Channel<Response>) -> Result<u64, String> {
    let ws = windows.get(&window)?;
    terminals.open(window.label(), &ws.root, cols, rows, output)
}

#[tauri::command]
fn term_write(window: WebviewWindow, terminals: State<Terminals>, id: u64, data: String) -> Result<(), String> {
    terminals.write(window.label(), id, data.as_bytes())
}

#[tauri::command]
fn term_resize(window: WebviewWindow, terminals: State<Terminals>, id: u64, cols: u16, rows: u16) -> Result<(), String> {
    terminals.resize(window.label(), id, cols, rows)
}

#[tauri::command]
fn term_close(window: WebviewWindow, terminals: State<Terminals>, id: u64) {
    terminals.close(window.label(), id);
}

#[derive(Serialize)]
struct TermLink {
    path: String,
    line: Option<u32>,
}

/// Text under the pointer in terminal `id` that looks like a path (D2): the
/// workspace file it names, read against the session's current folder, if
/// that file exists.
#[tauri::command]
async fn term_link(window: WebviewWindow, windows: State<'_, Windows>, terminals: State<'_, Terminals>, id: u64, text: String) -> Result<Option<TermLink>, String> {
    let ws = windows.get(&window)?;
    let Some(base) = terminals.cwd(window.label(), id) else { return Ok(None) };
    let (path, line, _col) = pty::split_location(&text);
    let full = doc::normalize(&base.join(path));
    let Some(rel) = ws.relative(&full) else { return Ok(None) };
    Ok(ws.resolve(&rel).ok().filter(|p| p.is_file()).and_then(|p| ws.relative(&p)).map(|path| TermLink { path, line }))
}

/// Open a window on `root`, or bring forward the one already showing it.
fn open_window(app: &AppHandle, root: PathBuf, initial: Option<String>) -> Result<(), String> {
    let windows = app.state::<Windows>();
    let label = {
        let map = windows.0.lock().unwrap();
        if let Some((label, _)) = map.iter().find(|(_, ws)| ws.root == root) {
            if let Some(w) = app.get_webview_window(label) {
                let _ = w.unminimize();
                let _ = w.set_focus();
                // A file named on the command line opens as a tab there.
                if let Some(file) = initial {
                    use tauri::Emitter;
                    let _ = w.emit_to(label.as_str(), "open-file", file);
                }
            }
            return Ok(());
        }
        (1..).map(|n| if n == 1 { "main".to_string() } else { format!("main-{n}") }).find(|l| !map.contains_key(l)).unwrap()
    };
    let ws = new_workspace(app, root.clone(), initial);
    windows.0.lock().unwrap().insert(label.clone(), ws.clone());
    WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(format!("{} — maido", folder_name(&root)))
        .inner_size(1200.0, 800.0)
        .build()
        .map_err(|e| e.to_string())?;
    ws.watch(app.clone(), label);
    Ok(())
}

/// `maido [path]`: with no path, the folder maido was started in (M2).
fn open_from_args(app: &AppHandle, args: &[String], cwd: &Path) {
    match workspace::target(args.get(1).map(String::as_str), cwd) {
        Ok((root, file)) => {
            if let Err(e) = open_window(app, root, file) {
                eprintln!("maido: {e}");
            }
        }
        Err(e) => eprintln!("{e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // An AppImage starts inside its own mount, where its WebKit finds its
    // helper processes by relative path; paths on the command line are
    // relative to where it was launched, which the AppImage runtime keeps in
    // $OWD. Stay there until the window opens: a second launch sends its
    // folder to the running maido (single instance) from here.
    let mount = std::env::current_dir().ok();
    if std::env::var_os("APPIMAGE").is_some()
        && let Some(owd) = std::env::var_os("OWD")
    {
        let _ = std::env::set_current_dir(owd);
    }
    let args: Vec<String> = std::env::args().collect();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // Fail before any window if the path is wrong.
    if let Err(e) = workspace::target(args.get(1).map(String::as_str), &cwd) {
        eprintln!("{e}");
        std::process::exit(1);
    }

    let context = tauri::generate_context!();
    // Only release builds name an update server and key (M3).
    let updater = context.config().plugins.0.contains_key("updater");
    let mut builder = tauri::Builder::default();
    if updater {
        builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    }
    #[cfg(desktop)]
    {
        // A second `maido <path>` opens its window in this process (E1).
        builder = builder.plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            open_from_args(app, &argv, Path::new(&cwd));
        }));
    }
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(Windows::default())
        .manage(Mermaid::default())
        .manage(Terminals::default())
        .manage(Updates::new(updater))
        .setup(move |app| {
            // Back into the AppImage's mount before WebKit starts (see above).
            if let Some(dir) = &mount {
                let _ = std::env::set_current_dir(dir);
            }
            let data = app.path().app_data_dir()?;
            app.manage(Sessions::new(data.join("sessions.json")));
            open_from_args(app.handle(), &args, &cwd);
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::Destroyed = event {
                let app = window.app_handle();
                if window.label() == mermaid::LABEL {
                    app.state::<Mermaid>().closed();
                    return;
                }
                app.state::<Terminals>().close_window(window.label());
                let windows = app.state::<Windows>();
                let mut map = windows.0.lock().unwrap();
                map.remove(window.label());
                // The hidden helper alone must not keep maido running.
                if map.is_empty() {
                    app.exit(0);
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            workspace_info,
            list_dir,
            load_document,
            preview_document,
            load_source,
            save_file,
            diff_lines,
            diff_rendered,
            close_document,
            highlight_block,
            highlight_css,
            resolve_link,
            open_external,
            load_session,
            save_session,
            load_settings,
            save_settings,
            render_mermaid,
            mermaid_jobs,
            mermaid_done,
            term_open,
            term_write,
            term_resize,
            term_close,
            term_link,
            quick_open,
            search,
            export_file,
            image_data,
            print_page,
            fs_create,
            fs_rename,
            fs_trash,
            fs_paste,
            clipboard_read,
            clipboard_write,
            open_folder,
            add_root,
            remove_root,
            update::check_update,
            update::install_update,
        ])
        .run(context)
        .expect("error while running maido");
}
