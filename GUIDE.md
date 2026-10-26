# maido guide

Every command and shortcut. Shortcuts use `Ctrl` (maido is tested on Linux first; macOS and Windows are not tested yet).

## Opening things

| Shortcut | Action |
|---|---|
| `Ctrl+O` | Quick open: type part of a file name (fuzzy, over every folder in the workspace) |
| `Ctrl+Shift+P` | Command palette; or type `>` in quick open |
| `Ctrl+N` | New file in the current folder |
| `Ctrl+Shift+F` | Search the text of every file in the workspace |

From a shell, `maido .`, `maido ~/notes` and `maido README.md` open a folder or a file; a second call opens a window in the running maido.

## Documents and tabs

| Shortcut | Action |
|---|---|
| `Ctrl+F` | Find in the document (see below); in the editor it is the editor's find and replace |
| `Ctrl+E` | Switch the tab between reading and editing |
| `Ctrl+S` | Save (refused if the file changed on disk since you opened it) |
| `Ctrl+W` | Close the tab (asks first when it has unsaved edits) |
| `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous tab |
| `Ctrl+P` | Print as PDF (Markdown and LaTeX documents) |
| `Ctrl+=` `Ctrl+-` `Ctrl+0` | Text size: bigger, smaller, normal |
| Middle click on a tab | Close it |
| Mouse wheel over the tabs | Scroll the tab row |

### Find in the document

The bar shows which match you are on and how many there are (`3 of 12`).

| Key | Action |
|---|---|
| `Enter` / `Shift+Enter` | Next / previous match |
| Click the number, type another, `Enter` | Jump to that match (a number past either end goes to the nearest) |
| `Escape` | Close the bar (inside the number field: back to the search text) |

### Editor

CodeMirror 6. Besides the usual keys (`Ctrl+Z` undo, `Ctrl+Shift+Z` redo, `Tab` indent, `Ctrl+A` select all):

| Shortcut | Action |
|---|---|
| `Ctrl+F` | Find and replace |
| `Ctrl+G` / `Ctrl+Shift+G` | Next / previous match |
| `Alt+G` | Go to line |
| `Ctrl+D` | Select the next occurrence of the selected word |

The editor's find panel shows the match you are on and the total (`3 of 12`); `<` and `>` (or `Shift+Enter` / `Enter`) step, and typing another number in the field and pressing `Enter` jumps to that match. It also has *Aa* (match case), `.*` (regular expression), Replace and All.

Markdown, Mermaid and LaTeX are coloured. The preview beside the editor follows what you type and scrolls with you.

## Explorer

| Key | Action |
|---|---|
| `F2` | Rename the focused file or folder |
| `Delete` | Move it to the system trash (restorable) |
| Right click | New file or folder, cut, copy, paste, rename, trash, copy path. On an added folder's name: *Remove from workspace* (nothing on disk changes) |

Each folder of the workspace sits under its own tinted header; click it to fold the folder.

## Terminal

| Shortcut | Action |
|---|---|
| `` Ctrl+` `` | Show, hide or focus the terminal |
| `` Ctrl+Shift+` `` | New terminal session |
| `Ctrl`+click on a path | Open it in maido (`file.md:42` scrolls to that line) |
| `Ctrl`+click on a URL | Open it in the browser |

Inside a terminal `Ctrl+W` and `Ctrl+Tab` go to the shell, not to maido. A `bash`/`sh` code block in a document has a *Send to terminal* button that pastes without pressing `Enter`.

## Menus

**File:** New file, New folder, Open file, Open folder, Add folder to workspace, Open recent (a side menu of the last folders; each shows its two parent folders and `…/` when more lie above), Save, Save all, Print as PDF, Close tab, Close window.
**Edit:** Undo, Redo, Cut, Copy, Paste, Find in document, Search in workspace, Edit/Read this file. Edit acts on whatever had the focus: the editor, a text field, a terminal (paste) or the selected text of the page.
**Settings:** theme, font, text size, page margins, remote images, terminal scrollback and renderer, update checks.

## Command palette

`Ctrl+Shift+P` lists these (a few appear only when they apply, such as *Save* with a file being edited):

| Command | Shortcut |
|---|---|
| Open file… | `Ctrl+O` |
| Open folder… | |
| Add folder to workspace… | |
| Open recent folder: *name* | |
| New file… | `Ctrl+N` |
| New folder… | |
| Find in document | `Ctrl+F` |
| Search in workspace | `Ctrl+Shift+F` |
| Edit this file / Read this file | `Ctrl+E` |
| Save | `Ctrl+S` |
| Save all | |
| Close tab | `Ctrl+W` |
| Compare this file with… | |
| New diff | |
| Print as PDF… | `Ctrl+P` |
| Show terminal / Hide terminal | `` Ctrl+` `` |
| New terminal | `` Ctrl+Shift+` `` |
| Move terminal panel below / to the right | |
| Follow changes / Stop following | |
| Show all files / only documents in the Explorer | |
| Check for updates | |
| Theme: system, light, dark | |
| Font size: bigger, smaller, normal | `Ctrl+=` `Ctrl+-` `Ctrl+0` |
| Font: sans, serif | |
| Outline, Explorer | |

In the palette `↑` `↓` choose, `Enter` opens, `Escape` closes.

## File types

| Files | How they open |
|---|---|
| `.md`, `.markdown` | Rendered; editable |
| `.mmd`, `.mermaid` | One diagram with zoom and pan; editable |
| `.tex`, `.latex` | Rendered as a document with KaTeX math (see below); editable |
| PNG, JPEG, WebP, GIF, BMP, AVIF, ICO, SVG | Shown as a picture |
| PDF (up to 50 MB) | Drawn page by page |
| Any other text file | Plain text, editable |

### LaTeX

A `.tex` file is read as a document, not compiled: sections, emphasis, lists, quotes, `verbatim` and `lstlisting` code, simple `tabular`s, `\includegraphics` pictures, `\href`/`\url` links, `\cite`/`\ref` (shown as `[key]`) and all math (`$…$`, `\(…\)`, `$$…$$`, `\[…\]`, `equation`, `align`, `gather`, …) are shown. Macros you define, packages, page layout and bibliographies are not run; an unknown command loses its name and keeps its text. Line positions can shift by a few lines in lists and tables, so scroll sync is approximate.
