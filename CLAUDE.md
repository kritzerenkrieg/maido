# maido

A path-based Markdown and Mermaid reader and editor (Tauri 2, Svelte 5 + TypeScript, `comrak`, `syntect`, CodeMirror 6, `portable-pty` + xterm.js) with a terminal beside the docs. Reader first, editor second. No language servers, plugins or WYSIWYG. Git is one simple panel (changes, commit, sync) over the system's `git`; no history, branches or staging UI. No agent detection: an agent is just a program in a terminal. Design: `ARCHITECTURE.md`. Threat model: `SECURITY.md`.

## Git

- AI MUST NEVER co-author lines. Human will forever carry the responsibilities of their creation.
- Commits follow Conventional Commits: `type(scope): subject`, scope optional.
  - Types: `feat` (something you can now do or see), `fix` (something that was wrong), `docs`, `ci`, `chore` (scaffolding, assets, vendoring, releases). No `refactor`, `style` or `test` so far: tests ride with the change they cover.
  - Scopes name the part of maido a user would recognise: `app`, `doc`, `viewer`, `editor`, `diff`, `terminal`, `find`, `search`, `explorer`, `workspace`, `tabs`, `mermaid`, `latex`, `update`, `git`, `release`. Leave it out when the change spans the whole project (`docs: add README…`).
  - Subject: lowercase, imperative, no final period, says what changed as the user meets it ("open pictures and PDFs as tabs", "treat a URL wrapped over several rows as one link"), not which function changed. Keys, menus and file names as they appear (`Ctrl+Shift+C`, `File > Open recent`, `.tex`).
  - Body optional: plain prose paragraphs, not bullets, not hard-wrapped, saying how it works or why, in ARCHITECTURE.md's voice. For a fix, say what went wrong and what happens now.
  - One feature or fix per commit; code, its tests and its doc updates (ARCHITECTURE.md, SECURITY.md, GUIDE.md) go together.
  - Releases: `chore(release): X.Y.Z`, changing only the version in `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` and `Cargo.lock`.

## Toolchain and build

- Rust and pnpm are not on the PATH of non-interactive shells: `PATH=$HOME/.cargo/bin:$HOME/.local/share/pnpm/bin:$PATH`.
- `pnpm install`, then `pnpm build` (release, frontend embedded, no bundle) or `pnpm dev`; `pnpm release` also makes the `.deb`. A plain `cargo build` does not embed the frontend.

## Memory budget

Measured against a bare Tauri window in the same sitting: idle with one document at most 60 MB above it, fully loaded at most 90 MB above it, one document at most 25 MB above maido's empty window. Measure PSS (not RSS) summed over maido and all child processes (WebKit web and network processes) from `/proc/<pid>/smaps_rollup`, on a fresh workspace copy, ~10 s after start. Only one maido process runs at a time (single instance), so close any open window first.

## Saved state

Sessions and settings: `~/.local/share/dev.maido.viewer/sessions.json`. Mermaid SVG cache: `~/.cache/dev.maido.viewer/mermaid/`.

## Screenshots

No ImageMagick or Python imaging here; `xwd -silent -name <title>` captures a window, and a small standard-library script can convert the XWD to PNG.
