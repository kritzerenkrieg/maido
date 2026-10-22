# maido

A path-based Markdown and Mermaid reader and editor (Tauri 2, Svelte 5 + TypeScript, `comrak`, `syntect`, CodeMirror 6, `portable-pty` + xterm.js) with a terminal beside the docs. Reader first, editor second. No language servers, git UI, plugins, WYSIWYG or sync. No agent detection: an agent is just a program in a terminal. Design: `ARCHITECTURE.md`. Threat model: `SECURITY.md`.

## Git

- AI MUST NEVER co-author lines. Human will forever carry the responsibilities of their creation.

## Toolchain and build

- Rust and pnpm are not on the PATH of non-interactive shells: `PATH=$HOME/.cargo/bin:$HOME/.local/share/pnpm/bin:$PATH`.
- `pnpm install`, then `pnpm build` (release, frontend embedded, no bundle) or `pnpm dev`; `pnpm release` also makes the `.deb`. A plain `cargo build` does not embed the frontend.

## Memory budget

Measured against a bare Tauri window in the same sitting: idle with one document at most 60 MB above it, fully loaded at most 90 MB above it, one document at most 25 MB above maido's empty window. Measure PSS (not RSS) summed over maido and all child processes (WebKit web and network processes) from `/proc/<pid>/smaps_rollup`, on a fresh workspace copy, ~10 s after start. Only one maido process runs at a time (single instance), so close any open window first.

## Saved state

Sessions and settings: `~/.local/share/dev.maido.viewer/sessions.json`. Mermaid SVG cache: `~/.cache/dev.maido.viewer/mermaid/`.

## Screenshots

No ImageMagick or Python imaging here; `xwd -silent -name <title>` captures a window, and a small standard-library script can convert the XWD to PNG.
