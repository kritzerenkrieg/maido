![maido](assets/maido-logo.svg)

# maido

A quiet Markdown and Mermaid reader with a terminal beside it. Read your docs, make quick fixes, and run your shell, CLI or agent right next to them.

---

## Why maido

Most of what an agent or a CLI writes ends up as Markdown. maido is for reading that well: open a folder, watch documents update as files change on disk, and keep a terminal one keystroke away. It reads first and edits second, and it stays small.

## Features

- **Reader first.** Rendered Markdown with code highlighting, math (KaTeX), an outline, tabs and links between documents. Only the blocks that changed are redrawn, so live reload stays smooth.
- **Mermaid built in.** Diagrams in fenced blocks or `.mmd` files, with zoom, pan and export to PDF, PNG or JPG.
- **A terminal beside the docs.** Real PTY sessions in your workspace folder. Paths in the output are clickable, and code blocks have a "send to terminal" button. An agent is just a program in a terminal.
- **An editor for quick fixes.** CodeMirror with a live preview beside it, scroll-synced, loaded only when you start editing.
- **Workspace tools.** Quick open, full-text search, a diff window for files or text, and print to PDF.
- **Follow mode.** Switch to each document as it changes on disk, to watch an agent work.
- **Light on memory.** Tauri 2 and Svelte 5, with parsing, highlighting, search and terminals done in Rust.

## Usage

```sh
maido .            # open the current folder
maido ~/notes      # open a folder
maido README.md    # open a file
```

### Keys

| Key | Action |
|---|---|
| `Ctrl+O` | Quick open a file |
| `Ctrl+Shift+F` | Search in the workspace |
| `Ctrl+F` | Find in the document |
| `Ctrl+E` | Switch between viewer and editor |
| `Ctrl+S` | Save |
| `` Ctrl+` `` | Show or hide the terminal |
| `` Ctrl+Shift+` `` | New terminal |
| `Ctrl+=` `Ctrl+-` `Ctrl+0` | Font size: bigger, smaller, normal |
| `Ctrl+P` | Print as PDF |
| `Ctrl+W` | Close the tab |

## Install

Prebuilt packages (`.deb` and AppImage) are on the [Releases tab](https://github.com/kritzerenkrieg/maido/releases) of the GitHub repository. Download the latest one from there, or build it yourself below.

## Build

Requires Rust, Node, pnpm and Tauri's platform libraries (on Debian and Ubuntu: `libwebkit2gtk-4.1-dev` and friends).

```sh
pnpm install
pnpm dev        # development
pnpm build      # release binary, no bundle
pnpm release    # also builds the packages
```

## Learn more

- [`ARCHITECTURE.md`](ARCHITECTURE.md) explains the design, and [`ARCHITECTURE.mmd`](ARCHITECTURE.mmd) draws it. Open it in maido.
- [`SECURITY.md`](SECURITY.md) is the threat model.

## License

[MIT](LICENSE)
