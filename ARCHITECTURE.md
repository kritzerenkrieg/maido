# Architecture

How maido is built.

Last updated: 2026-10-22.

## Code layout

| Path | What |
|---|---|
| `Cargo.toml` | Cargo workspace; release profile (LTO, one codegen unit, `panic = "abort"`, stripped) |
| `src-tauri/src/lib.rs` | Tauri commands, windows (one per workspace), `maido <path>`, single instance |
| `src-tauri/src/doc.rs` | Markdown → blocks: parsing, sanitizing, heading ids, image rewriting, incremental re-parse (H5) and its tests |
| `src-tauri/src/latex.rs` | `.tex` / `.latex` → Markdown with math, for the same block pipeline (C3) |
| `src-tauri/src/highlight.rs` | `syntect` highlighting and its cache (H4) |
| `src-tauri/src/mermaid.rs` | Mermaid SVG disk cache and the hidden helper webview (H3) |
| `src-tauri/src/workspace.rs` | Tree listing, link resolution, file watcher, saved sessions (E1, E2) |
| `ui/` | The frontend (Svelte 5, Vite); a pnpm workspace package |
| `ui/src/App.svelte` | The window: top bar and menus, sidebar (Explorer, outline, search), tabs, banners, follow mode |
| `src-tauri/src/files.rs` | The Explorer's file actions: create, rename, trash, copy and move, kept inside the workspace (N4) |
| `ui/src/Document.svelte` | One document: keyed blocks, scroll keeping, lazy highlighting and diagrams, links |
| `ui/src/PdfPage.svelte` | A PDF file: pdf.js, loaded with the first PDF, draws pages into canvases as they near the view |
| `ui/src/MermaidPage.svelte` | `.mmd` / `.mermaid` files: one diagram with zoom and pan |
| `ui/src/mermaid-frame.ts` | The hidden Mermaid helper's page |
| `src-tauri/src/pty.rs` | Terminal sessions (PTY, output channel, current folder) and `path:line` parsing (D1, D2) |
| `ui/src/Terminals.svelte`, `ui/src/lib/terminal.ts` | The terminal panel and one xterm.js session, with path links and paste (D1–D3) |
| `src-tauri/src/diff.rs` | Line and word diffs, and rendered block diffs (E5) |
| `ui/src/Editor.svelte`, `ui/src/lib/editor.ts`, `ui/src/lib/buffers.ts` | The editor for one file: CodeMirror setup, colouring, keys, buffer state (B5) |
| `ui/src/DiffView.svelte` | A diff tab (E5) |
| `ARCHITECTURE.mmd` | This document's structure as a Mermaid diagram |
| `ui/src/lib/html.ts` | Block HTML made ready for the page (local and remote images), shared by documents and diffs |
| `src-tauri/src/find.rs` | Quick open (file list, fuzzy matching) and full-text search (E1, L6) |
| `ui/src/Palette.svelte`, `ui/src/Search.svelte` | Quick open and commands; the search tab (L6) |
| `ui/src/FindBar.svelte` | Find in the document (L6) |
| `ui/src/lib/math.ts` | KaTeX, loaded with the first math (C3) |
| `ui/src/lib/export.ts` | Saving exports through Rust (L1) |
| `ui/src/lib/diagram-export.ts`, `ui/src/lib/pdf.ts` | The diagram download panel; a one-image PDF writer (L3) |
| `ui/src/lib/theme.ts` | The theme setting, and who follows it (L4) |
| `package.json` | Root scripts (`pnpm dev`, `pnpm build`) and the Tauri CLI |

## Stack

| Part | Choice |
|---|---|
| App framework | Tauri 2 (Rust backend, system webview) |
| Frontend | Svelte 5 + TypeScript on plain Vite (no SvelteKit) |
| Markdown parser | `comrak`, in Rust |
| Code highlighting | `syntect`, in Rust; no auto-detection, a fence without a language is plain text |
| Editor | CodeMirror 6, loaded on first edit |
| Terminal | `portable-pty` in Rust, xterm.js in the frontend |

Rust does the heavy work (parsing, highlighting, sanitizing, files, search, terminal sessions); the webview only displays results. (B1–B5)

## Rendering

**Blocks.** Rust parses a document into top-level blocks, each with a content hash, its source line range and its sanitized HTML. The frontend keeps hash → element and replaces only blocks whose hash changed, so an edit or an agent's change costs in proportion to what changed. Blocks use `content-visibility: auto`, so the browser skips layout off screen. Source lines drive scroll sync between editor and preview and keep the reading position when a file reloads. (C1)

As built: a block's hash covers its source lines with their final newline, plus its heading id (ids are unique per document, so they depend on the headings above). In documents with link reference or footnote definitions a block's HTML depends on the rest of the document, so there the hash is of the HTML itself. The frontend keys blocks by hash plus an occurrence count in a keyed Svelte `{#each}`, and sends the keys it already shows with each reload so Rust leaves their HTML out. Footnote definitions render together as the last block. Before a reload the frontend notes the first visible block and its offset and restores them after, since WebKit has no scroll anchoring.

**Re-parsing.** On a change (an edit or the file changing on disk, debounced), Rust re-parses from the start of the first changed block onward and stops once the new block boundaries line up with the old ones; everything before and after keeps its blocks. A changed link or footnote definition also re-renders the blocks that use it, wherever they are. Not the changed block alone: an edit can change blocks it does not touch (an opened code fence swallows the rest of the document, `---` under a paragraph makes it a heading, list items merge or split). (H5)

As built: the re-parse starts at the block before the one holding the first changed line, moved back until the line above it is blank (so no open paragraph or list reaches into it); blocks before that are kept. The parse then runs to the end of the document, and every block whose hash is unchanged reuses its HTML, which skips the expensive part (sanitizing). Stopping the parse itself once boundaries realign is left until a document is large enough for it to matter (5 MB parses in 300 ms). Documents with link reference or footnote definitions, and documents starting with `---` (a later `---` can close front matter), are always parsed whole. A randomized test checks that the incremental result equals a full parse, block for block; 160,000 random edits pass.

**Syntax highlighting** (`syntect`, ~60 µs per code line) is done per block and cached by block hash, so unchanged code is never highlighted twice. A block is first shown plain; its highlighted version replaces it when ready, visible blocks first. (H4)

As built: Rust keeps each block's fenced code (language, text) in document order, nested fences included. When a block comes within 400 px of the view, the frontend asks for it and replaces each `<code>` with the highlighted spans. The cache is keyed by a hash of language and code. syntect's default grammars lack TypeScript, so `ts`/`tsx`/`jsx` use JavaScript's. Colours come from syntect themes (InspiredGitHub, base16-ocean.dark) turned into CSS once at start.

**Mermaid.** Never loaded in the main page: Mermaid holds 90–200 MB that WebKit does not give back while the process lives. Diagrams render in a **hidden helper webview** with its own web process, created when a diagram without a cached SVG scrolls into view, and never shown. WebKit throttles timers in hidden pages (20 diagrams took 31.8 s), so the helper's throttling is switched off before it loads (WebKitGTK 2.42+ features `HiddenPageDOMTimerThrottling`, `DOMTimersThrottling` and `HiddenPageCSSAnimationSuspension`, set through `webkit_settings_set_feature_enabled`; macOS and Windows need their own equivalent when ported). Rust sends it the diagram sources and gets SVG strings back; it closes 3 s after its last diagram, or stays open while the cursor is inside a Mermaid block in the editor, and its memory goes with its process. (H3)

SVGs are cached on disk by Rust, keyed by a hash of the diagram source, the Mermaid version and the theme, so a diagram renders once ever and a document whose diagrams are cached loads no Mermaid at all. Mermaid runs with `securityLevel: "strict"`; the SVG is sanitized before insertion. A broken diagram shows its source and the error. `.mmd` / `.mermaid` files open as a full-page diagram with zoom and pan. Export as PDF, PNG or JPG (L3). (C2, H3)

As built: any `mermaid` fence, nested ones included, is replaced by its diagram when it comes near the view. Rust queues sources for the helper and wakes it with an event; the helper pulls the queue, renders, and returns each SVG or error; identical sources share one render. The theme follows maido's theme setting (L4), and diagrams on screen are drawn again when it changes. Each diagram has one download button on hover (`.mmd` pages: in their toolbar). It opens a panel: PDF, PNG or JPG; transparent background (not for JPG); size in percent, 100% by default. The sanitized SVG is drawn in a canvas at that size, on the page's background unless transparent (flowchart labels in `<foreignObject>` included; WebKitGTK does not taint the canvas). PDF draws at twice the pixels and wraps them in a one-page PDF sized in points to match (`lib/pdf.ts`, deflate through the browser's `CompressionStream`, alpha as a soft mask): an image, not vector. No SVG export (L3). Not built yet: keeping the helper open while the cursor is in a Mermaid block, and pruning the disk cache.

**Math.** KaTeX, Phase 4, loaded only when a document has math. Rust marks math spans while parsing, so the frontend never scans text for `$`. (C3)

As built (Phase 4): comrak's `$…$`, `$$…$$` and `` $`…`$ `` syntax and ```` ```math ```` fences; a dollar followed by a space or digit stays text ("$5 and $6"). The sanitizer lets `data-math-style` through (values `inline` and `display` only), and each block tells the frontend whether it has math. When such a block comes near the view, KaTeX (259 KB, plus its fonts) loads and typesets it. KaTeX runs with `trust: false` (no `\href`, `\url`, `\includegraphics`) and renders bad TeX as a red message in place.

**LaTeX files.** A `.tex` / `.latex` file is read, not compiled: `latex.rs` turns the source into Markdown line by line (sections to headings, emphasis, lists, quotes, `verbatim` and `lstlisting` to fences, simple `tabular`s to tables, `\includegraphics` to images, `\href` and `\url` to links, `\cite` and `\ref` to `[key]`) and every math form into comrak's math syntax (`$`…`$` for inline, so a leading digit is not mistaken for a price; a ```` ```math ```` fence for `equation`, `\[ \]` and `$$` blocks, with `align` wrapped in `aligned` and `gather` in `gathered`, which is what KaTeX has). The result goes through the Markdown path unchanged, so blocks, re-parse, KaTeX, outline, find, print and the editor's live preview all work. Not run: macros, packages, page layout, bibliographies. Lines stay in step with the source except where a nested list's opener is dropped, or a table, title block or display-math line adds one, so scroll sync is approximate there. The editor colours LaTeX (`editor.ts`). Tested through the real parser (`latex.rs` tests).

**Export.** (L1) PDF: the system print dialog ("Print to file") with a print stylesheet that shows the document alone, all of it, in light colours; with a dark theme, diagrams get a light copy that only prints. Every export goes through Rust, which opens the save dialog (starting in the document's folder) and writes only to the path chosen there.

**Themes and reading settings.** (L4, L5) System, Light or Dark: `data-theme` on the page root picks the colour variables; code colours (syntect's CSS, scoped the same way), diagrams and terminals follow. Dark is screen-only, so printing is always light. Font size (a percentage, 70–200, Ctrl+= / Ctrl+- / Ctrl+0), font (sans, serif) and the four page margins (whole px) are CSS variables; font size applies to the editor too. Settings are global.

## Editor

CodeMirror 6 with Markdown and Mermaid colouring, split view with live preview (through the block diffing above), save, find/replace. (B5)

When the open file changes on disk while you have unsaved edits, nothing is overwritten silently: a banner offers keep mine / load theirs / show diff. Your own saves are recognised and do not trigger a reload. "Show diff" opens the diff window with your buffer against the file on disk. (E3)

As built (Phase 3): Edit (Ctrl+E) turns a tab into the editor with the rendered preview beside it; Read turns it back. CodeMirror (548 KB) loads with the first edit, so it is not in the startup bundle (26 KB). Each open file being edited has a buffer holding CodeMirror's state (text, undo history, selection) and scroll, kept until its tab closes. The preview renders the buffer's text 120 ms after typing stops, through the same keyed blocks as reading (`preview_document`); a tab in read mode with unsaved edits also shows them. Scroll sync maps the editor's top line to the preview through each block's first and last source line, interpolating inside blocks and across the gaps between them; whichever pane the pointer is over leads. Ctrl+S saves: Rust refuses when the file's hash on disk is no longer the one the buffer started from, so a save cannot overwrite a change it has not seen; otherwise it writes a temporary file beside the original (permissions kept) and renames it over. The returned hash becomes the buffer's base, which is how the watcher's event for your own save is recognised and ignored. A file changed on disk while its buffer is clean is reloaded into the editor quietly; with unsaved edits, or when a save is refused, the E3 banner appears. "Keep mine" takes the disk version as the new base, so the next save overwrites it. Closing a tab or the window with unsaved edits asks first (Save / Discard / Cancel). Find and replace are CodeMirror's (Ctrl+F). `.mmd` files preview as diagrams; other text files get the editor alone. Not built yet: keeping the Mermaid helper open while the cursor is in a Mermaid block.

## Diff

A diff tab with two sides, each a file or pasted text: file ↔ file, file ↔ text, text ↔ text. Side by side or inline (toggle); a line diff with changed words highlighted inside lines, computed in Rust (`similar` crate). Opened from the command palette, from "compare with…" on a file, and from E3's "show diff". Markdown files can also be compared rendered: blocks marked added, removed or changed, using the block hashes from Rendering. Read-only: no merging. (E5)

As built (Phase 3): diff tabs (`ui/src/DiffView.svelte`) open from "Compare…" (left side is the current file), from "Diff" when no file is active, and from E3's "Show diff"; they are not saved with the session. Each side is a workspace path or pasted text. Lines are matched with the patience algorithm; a changed line paired with its replacement is diffed again by words. Unchanged runs longer than six lines fold to three lines of context each side, expandable. "Rendered" parses both sides into blocks and matches them by block hash, so changed blocks show as removed and added, side by side. The command palette entry waits for Phase 4.

## Shell

A real PTY per session in Rust (`portable-pty`), drawn with xterm.js and its DOM renderer (K1), loaded with the first terminal. Full-screen programs work: truecolor, mouse, resize, bracketed paste, alternate screen. Sessions are tabs; each starts in the workspace folder with `$SHELL` and your environment, keeps running while the panel is hidden, and dies with the window. Docs and terminal sit side by side or stacked, in resizable splits. (D1)

Scrollback is 2,000 lines per session by default (xterm.js keeps about 12 bytes per cell, so about 3 MB at 120 columns once full), with a setting to raise it. (D5)

**Clickable paths.** Text in terminal output that looks like a path, optionally with `:line` or `:line:col` (`docs/api.md:42`, `./README.md`, absolute paths), is checked in Rust against the session's folder; if the file exists, Ctrl+click opens it in maido: Markdown scrolls to the block holding that line, other files open in the raw view at that line. URLs open in the browser. Detection runs only on visible rows, on hover. (D2)

`bash` / `sh` code blocks have a "send to terminal" button: it pastes into the active session without pressing Enter, so you run it yourself. (D3)

As built (Phase 2): `src-tauri/src/pty.rs` runs each session (`portable-pty`, your login shell, `TERM=xterm-256color`, started in the workspace folder). A reader thread sends output to the page as raw bytes over a Tauri channel, not JSON. Keystrokes come back as strings, and a session only accepts them from the window that opened it. Sessions end when their tab or window closes; quitting maido leaves no shells behind. The panel (`ui/src/Terminals.svelte`, `ui/src/lib/terminal.ts`) loads with the first terminal, so xterm.js (338 KB) is not in the startup bundle (20 KB). It sits below or to the right of the document; its place and size are remembered per workspace. Ctrl+\` shows, hides or focuses it, and Ctrl+Shift+\` opens a new session. Inside a terminal, Ctrl+W and Ctrl+Tab go to the shell.

Paths: when the pointer is over a row, the words on it that could be paths (with optional `:line` or `:line:col`, at most 16 per row) are checked by Rust against the session's current folder. On Linux that is the foreground program's folder from `/proc`, so a `cd` is followed; elsewhere it is the folder the session started in. Only existing files inside the workspace become links. Ctrl+click opens them at that line: Markdown scrolls to the block holding it, other files open as plain text at that line. Ctrl+click on a URL opens the browser. Column positions assume single-width characters, so a row with wide characters (CJK, emoji) can misplace the underline.

Send to terminal: shell fences (`bash`, `sh`, `shell`, `zsh`, `console`, `fish`) get the button, added by the frontend after sanitizing. The trailing newline is dropped. Multi-line text is pasted only when the shell has bracketed paste on (bash 5.1+, zsh and fish do by default); otherwise nothing is pasted and maido says why, since each newline would run a line. With no terminal open, the button asks you to open one rather than starting a session itself (F5).

Renderer: the DOM renderer by default, with a setting ("GPU terminals") for WebGL, faster for very heavy output. Measured with one document open: +39 MB for a terminal with WebGL, +16 MB with the DOM renderer (K1).

## Workspace

Path-based, like VS Code. The file tree respects `.gitignore` (`ignore` crate) and shows `.md`, `.markdown`, `.mmd` and `.mermaid` by default, with a toggle for all files, which also shows hidden and ignored ones, `.git` included. Hidden files stay out of quick open, search and the file watcher, so the tree does not refresh by itself when one changes. Fuzzy quick open; full-text search (ripgrep crates). Relative links between documents open in maido; `#heading` links scroll. Open tabs and scroll positions are remembered per workspace. A second `maido <path>` while maido runs opens a window in the same process. (E1)

**More than one folder.** File > Add folder to workspace adds a folder beside the window's own; the Explorer shows each under its name (a folded or expanded header, with "Remove from workspace" in its right-click menu; nothing on disk changes). The window's own folder keeps plain relative paths; an added folder's paths read `@<id>/…` (ids are stable and never renumbered while the workspace lives), so tabs, links, search results and the session all stay plain strings. `Workspace::resolve` and `relative` map between the two, and every path is still checked to stay inside the folder it names. Quick open, search, the watcher and the `.gitignore` handling run over every folder; a terminal starts in the window's own folder. The added folders are saved per workspace (`roots:<folder>` in the sessions file) and restored with it.

**Recent folders.** The last ten folders a window opened are kept in the settings; File lists the latest five besides the current one, and the command palette has an entry for each. Choosing one switches the window as Open folder does.

**Render failures.** One document that fails to draw (a deleted file, a PDF cut short) must not leave the pane dead for the next. `Document.svelte` numbers its requests so a late answer never replaces a newer one, and after an error clears the old blocks and path so the next load starts fresh; `PdfPage.svelte` does the same for its loads, destroys a document that failed to open, and lets a page whose drawing failed be drawn again; `App.svelte` wraps the document in a `<svelte:boundary>` keyed by file, so an exception while drawing one file is shown in place and changing file starts clean. A Mermaid helper that stops answering is destroyed and forgotten after its timeout, so the next diagram opens a fresh one instead of waiting out the timeout again.

**Pictures and PDFs.** PNG, JPEG, WebP, GIF, BMP, AVIF, ICO and SVG files open for reading as a picture (the file itself, through the asset protocol, so it costs no memory in Rust). A PDF is read by Rust (`read_pdf`, up to 50 MB, workspace files only) and drawn by pdf.js, which loads with the first PDF: pages get canvases of the right shape at once, are drawn within 800 px of the view and emptied beyond 2,400 px. Neither can be edited.

**Source files.** A file in a language syntect knows (by its extension, or `Makefile`: Rust, Python, C, C++, C#, Java, Go, PHP, JSON, JavaScript, TypeScript, shell and about twenty more, `highlight::language`) opens highlighted whole, with its language named above it; over 1 MB it stays plain text. In the editor it is coloured by CodeMirror 5's mode for the language (`@codemirror/legacy-modes`), each loaded the first time a file in that language is edited; PHP has none and stays plain. Source files have their own icon in the tree.

**Files changed on disk** (`notify`, short debounce) reload through block diffing: only changed blocks update, the scroll position stays, and changed blocks are briefly highlighted so you see what an agent just did. New and deleted files update the tree live. (E2)

As built: the watcher watches each folder the tree could show (not ignored, not hidden) one level deep, so `node_modules` and the like cost no watches, and it adds folders as they appear. Changes reach the window as `fs-change` events (150 ms debounce). The tree lists one folder at a time when it is expanded, applying `.gitignore` even outside a repository. Tabs, the active tab, each tab's top source line and the toggles are saved per workspace in the app's data folder.

As built (Phase 4): **quick open** (Ctrl+O) lists the workspace's files once (`.gitignore` respected, hidden files left out, up to 200,000) and keeps the list until the watcher sees a file appear, go or move; matching is fuzzy on paths (`nucleo-matcher`, as in Helix), documents first on equal scores. The same box runs **commands** (Ctrl+Shift+P, or `>`): every action with its shortcut. **Search** (Ctrl+Shift+F) is a sidebar tab: ripgrep's own crates (`grep-searcher`, `grep-regex`) walk the same files on a thread; plain text, case-insensitive unless the query has a capital; binary files and files over 20 MB skipped. Results stream to the page file by file; a new search stops the old one; it stops at 5,000 matching lines (200 per file). Long lines are cut around the match. A result opens the file at its line. **Find in the document** (Ctrl+F) is a bar over the rendered page: matches inside one run of text, case-insensitive unless the query has a capital, marked with CSS custom highlights so the page is never changed; Enter and Shift+Enter step, Escape closes; the bar shows `n of total`, and the number is a field: type another and Enter to jump to that match. In the editor, Ctrl+F is CodeMirror's find and replace. **Ctrl+P prints** (L2, L6).

**Follow mode**, off by default, opens the file most recently changed on disk, to watch an agent work. (E4)

## Explorer

As built (N1–N4): the window's top row spans its whole width: **File, Edit and Settings** menus above the sidebar, the open tabs in the middle, and Edit/Compare and Terminal at the right. Menus open on click, move on hover and close on Escape or a click elsewhere; they never take the focus, so **Edit acts on what had it** (the editor, a text field, a terminal for paste, else the selected text of the page). Undo and redo are CodeMirror's in the editor and the browser's in text fields. The clipboard is read and written by Rust (`clipboard_read`, `clipboard_write`): WebKitGTK does not let a page read it. Settings is the old sidebar footer's content as a menu.

**Open folder** shows the system folder dialog and switches this window to the chosen folder: Rust ends the window's terminals, swaps its workspace (and watcher) and the page reloads from the saved session; a folder already open in another window is brought forward instead. It refuses while a file has unsaved edits.

The **Explorer** (the sidebar's first tab) has a right-click menu on every row and on empty space (`Tree.svelte` reports the click, `App.svelte` shows the menu and asks for names in the small dialog it shares with the trash confirmation). `files.rs` does the work: names may not hold a slash, an existing name is refused, a paste onto a taken name gets " copy" (or " copy 2"), a folder cannot be moved into itself, symlinks are copied as links and never followed, and delete is `trash::delete` (the system trash, restorable). Every path goes through `Workspace::resolve`, so nothing outside the workspace is touched. Each workspace folder's row has a folder icon, open or closed, and its own menu: an added folder's has **Close folder**, which takes it out of the workspace at once (nothing is deleted, and it stays out when the folder is opened again); only open files with unsaved edits in it make it ask first, to discard them. The page keeps the copied or cut paths itself (not on the system clipboard). Open tabs follow a rename or move; files with unsaved edits are refused rather than lost; the watcher refreshes the tree.

## Memory


- One window, one webview; never a webview per tab.
- Mermaid, KaTeX, CodeMirror and xterm.js load on first use.
- Background tabs keep their path, scroll position and block hashes, not their DOM.
- Rust owns file contents; the frontend keeps no second copy of large documents.
- No OS-specific memory switches: WebKit runs with its defaults on every platform. (H2)

(F2)

Where it goes (Phase 5, same sitting, total PSS): a bare Tauri 2.12 window showing one paragraph is 214 MB on this machine; maido's empty window 247 MB (+33); one document adds 19–23 MB; a diagram with the editor, its preview and one terminal 290 MB. Of maido's +33 MB, ~10 MB is its own process (mostly its larger binary's code) and ~24 MB the web process: WebKit, JavaScriptCore and ICU code a real interface pages in, the JS heap and DOM, and WebKit's compositing, which a page with one paragraph never enters (switching compositing off, a WebKitGTK-only setting, would save ~25 MB; H2). No single part of maido's own page is the cause: a static copy of the rendered window costs no more than the bare page, and every half of the app, script or markup, costs about the same. (M6)

## Release

Packages (M1) come from Tauri's bundler: `pnpm release` builds `target/release/bundle/deb/maido_<version>_amd64.deb` (7.5 MB; depends on the system's `libwebkit2gtk-4.1-0` and `libgtk-3-0`, installs `/usr/bin/maido`) and `…/appimage/maido_<version>_amd64.AppImage` (107 MB: WebKitGTK and GTK inside). Both carry an app-menu entry (`src-tauri/maido.desktop`, running `maido %F`) that opens Markdown files (`text/markdown`), and maido's logo: the icons in `src-tauri/icons` are made from `logo.png` with `pnpm tauri icon src-tauri/icons/logo.png` (then delete the `android` and `ios` folders it adds). With no path, maido opens the folder it was started in (M2). The AppImage's launcher (linuxdeploy's AppRun) runs maido inside its mount: maido reads command-line paths (and sends them to a running maido) from the folder it was launched from (`$OWD`), then returns into the mount before the window opens, because the bundled WebKit finds its helper processes by a path relative to it.

Updates (M3) use Tauri's updater plugin, registered only when the build's configuration names an update server and public key; `src-tauri/release.conf.json` (not in the repository until M4 is answered) adds them and `bundle.createUpdaterArtifacts`, and the build is signed with `TAURI_SIGNING_PRIVATE_KEY`. The page asks Rust (`update.rs`) at startup, at most once a day (`checkUpdates`, `updateCheckedAt` in settings), or when "Check for updates" is run. A newer release shows a banner: an AppImage downloads it, checks the signature, replaces itself and restarts; a `.deb` (the bundle type is written into the binary by the bundler) only offers the package's download link. The release server serves `latest.json`:

```json
{
  "version": "0.1.1",
  "notes": "What changed",
  "pub_date": "2026-10-22T00:00:00Z",
  "platforms": {
    "linux-x86_64-appimage": { "url": "https://…/maido_0.1.1_amd64.AppImage", "signature": "<contents of the .AppImage.sig>" },
    "linux-x86_64-deb": { "url": "https://…/maido_0.1.1_amd64.deb", "signature": "<contents of the .deb.sig>" }
  }
}
```
