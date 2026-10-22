# Security

maido's threat model.

Last updated: 2026-10-22.

Documents and diagrams are untrusted input: a downloaded README must not be able to run code or reach the shell. The shell itself is full access to the machine by design; it is only ever driven by you.

## Untrusted documents

- Raw HTML in Markdown is off by default, with a setting to allow it (the setting is not built yet; Phase 1 always drops raw HTML).
- All HTML is sanitized in Rust (`ammonia`) before it reaches the webview; Mermaid's SVG is sanitized again in the frontend (DOMPurify), and Mermaid runs in strict mode in a separate hidden webview.
- A strict Content Security Policy: only maido's own bundled scripts run; nothing remote. Images may come from the app, `data:`, the asset protocol and http(s) (remote ones only after you allow them, below).
- File access is limited to the open workspace: every path the frontend asks for is resolved in Rust and refused if it leaves the workspace, symlinks included; the asset protocol (local images) is scoped to the workspace folder, and image paths outside it are blocked.
- Links: `#heading` scrolls, relative links open workspace files in maido, `http(s):` and `mailto:` open in your browser (checked again in Rust), and every other scheme (`javascript:`, `file:`, custom) is never followed.

(F3)

Saving (Phase 3) goes through the same workspace check: the editor can only write a file inside the open workspace, and only the file it opened. A save is refused when the file changed on disk since the editor loaded it, until you choose "Keep mine"; it is written beside the original and renamed over it, so a crash never leaves half a file. The diff window only reads.

Exports (diagrams as images) are the one way maido writes outside the workspace, and only to a path you pick in the system's save dialog, which Rust opens itself: the page hands over the bytes, never a path. Diagram exports are images; workspace images are read by Rust, images only, from inside the workspace. Math is typeset by KaTeX with `trust` off, so TeX cannot make links or load images.

## Remote images

Loading a remote image tells its server you opened the document (tracking pixels). maido asks per document; the prompt has "don't ask again", which remembers the answer for good and can be changed in settings (for now the "Remote images" control in the sidebar). Until allowed, a remote image has no `src` at all, so nothing is requested. (F4)

## Documents and the shell

Document content can never start a terminal session or type into one. The "send to terminal" button on code blocks pastes only when you click it, and never presses Enter: you run the command yourself. (F5, D3)

As built: the button is added by maido after sanitizing, so a document cannot make one or click one. It pastes only into a session you already opened and never starts one. Multi-line text is pasted only as a bracketed paste, which the shell keeps as one edit instead of running line by line; without bracketed paste nothing is pasted. A session takes input only from the window that opened it. Links in terminal output open only workspace files (checked in Rust) or http(s) URLs, and only on Ctrl+click.

## Updates

maido's only network request of its own is the update check (Phase 5, M3): at most once a day, and only in release builds that name an update server, it fetches one JSON file over HTTPS; the request carries no data about you or your documents. A setting turns it off. Every update is signed: maido has the release key's public half built in and refuses a package whose signature does not match, so a compromised server or download cannot install anything. The AppImage replaces only itself, as you. maido never asks for a root password: for a `.deb` it only opens the new package's download link, and installing it is up to you.
