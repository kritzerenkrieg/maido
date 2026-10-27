// The Rust commands the frontend uses, typed.
import { Channel, invoke } from '@tauri-apps/api/core'

export interface WorkspaceInfo {
  root: string
  name: string
  initial: string | null
  /** Folders added beside this one; their paths read `@<id>/…`. */
  roots: RootInfo[]
}

export interface RootInfo {
  id: number
  name: string
  path: string
}

export interface Entry {
  name: string
  path: string
  dir: boolean
}

export interface BlockView {
  key: string
  line: number
  end: number
  /** Null when the key was in `have`: the element on screen stays. */
  html: string | null
  code: boolean
  /** Has math: KaTeX typesets it when the block comes into view (C3). */
  math: boolean
}

export interface OutlineItem {
  level: number
  id: string
  text: string
  line: number
}

export type DocView =
  | { kind: 'markdown'; blocks: BlockView[]; outline: OutlineItem[]; remote_images: number }
  | { kind: 'mermaid'; source: string }
  | { kind: 'text'; text: string }
  | { kind: 'code'; lang: string; html: string }
  | { kind: 'image'; path: string }
  | { kind: 'pdf' }
  | { kind: 'binary' }

export interface FsChange {
  path: string
  kind: 'change' | 'remove'
  tree: boolean
}

/** Per workspace, stored by Rust as opaque JSON. */
export interface Session {
  tabs: string[]
  active: string | null
  /** Source line at the top of the view, per tab. */
  scroll: Record<string, number>
  follow: boolean
  all: boolean
  /** The terminal panel: beside or below the document, and its share of the space. */
  termPos: 'right' | 'bottom'
  termSize: number
}

export interface Settings {
  remoteImages: 'ask' | 'always' | 'never'
  /** Lines each terminal keeps (D5). */
  scrollback: number
  /** Terminals drawn with WebGL (else the DOM renderer). */
  terminalGpu: boolean
  /** Colours (L4). */
  theme: 'system' | 'light' | 'dark'
  /** Reading settings (L5): text size as a factor, font, page margins in px. */
  docScale: number
  docFont: 'sans' | 'serif'
  docMargin: { top: number; right: number; bottom: number; left: number }
  /** Look for a new version at most once a day (M3), and when it last did (ms). */
  checkUpdates: boolean
  updateCheckedAt: number
  /** Folders opened before, newest first (File > recent). */
  recentFolders: string[]
}

/** A newer release: `install` when maido can update itself (an AppImage), else `url` is the package. */
export interface UpdateInfo {
  version: string
  notes: string
  install: boolean
  url: string
}

/** One side of the diff window: a workspace file, or text (pasted, or an unsaved buffer). */
export interface DiffSide {
  path?: string
  text?: string
}

export type DiffKind = 'equal' | 'delete' | 'insert' | 'replace'

export interface DiffRow {
  kind: DiffKind
  left: { line: number; segments: { text: string; changed: boolean }[] } | null
  right: { line: number; segments: { text: string; changed: boolean }[] } | null
}

export interface BlockRow {
  kind: DiffKind
  left: string | null
  right: string | null
}

/** A quick-open match: `indices` are the matched characters (code points) of `path`. */
export interface FileMatch {
  path: string
  indices: number[]
}

export interface LineMatch {
  line: number
  text: string
  /** Matched ranges in `text` (JavaScript string offsets). */
  ranges: [number, number][]
}

export type SearchEvent =
  | { kind: 'file'; path: string; matches: LineMatch[] }
  | { kind: 'done'; files: number; matches: number; truncated: boolean }
  | { kind: 'error'; message: string }

export const api = {
  workspaceInfo: () => invoke<WorkspaceInfo>('workspace_info'),
  listDir: (path: string, all: boolean) => invoke<Entry[]>('list_dir', { path, all }),
  loadDocument: (path: string, have: string[]) => invoke<DocView>('load_document', { path, have }),
  previewDocument: (path: string, text: string, have: string[]) => invoke<DocView>('preview_document', { path, text, have }),
  readPdf: (path: string) => invoke<ArrayBuffer>('read_pdf', { path }),
  loadSource: (path: string) => invoke<{ text: string; hash: string }>('load_source', { path }),
  /** Rejects with "conflict" when the file changed on disk since `base` (E3). */
  saveFile: (path: string, text: string, base: string, force = false) => invoke<string>('save_file', { path, text, base, force }),
  diffLines: (left: DiffSide, right: DiffSide) => invoke<DiffRow[]>('diff_lines', { left, right }),
  diffRendered: (left: DiffSide, right: DiffSide) => invoke<BlockRow[]>('diff_rendered', { left, right }),
  closeDocument: (path: string) => invoke<void>('close_document', { path }),
  highlightBlock: (path: string, key: string) => invoke<(string | null)[]>('highlight_block', { path, key }),
  highlightCss: () => invoke<string>('highlight_css'),
  resolveLink: (from: string, href: string) => invoke<{ path: string; anchor: string | null } | null>('resolve_link', { from, href }),
  openExternal: (url: string) => invoke<void>('open_external', { url }),
  loadSession: () => invoke<Partial<Session> | null>('load_session'),
  saveSession: (session: Session) => invoke<void>('save_session', { session }),
  loadSettings: () => invoke<Partial<Settings> | null>('load_settings'),
  saveSettings: (settings: Settings) => invoke<void>('save_settings', { settings }),
  /** A workspace image (absolute path) as a data: URL, for HTML export. */
  imageData: (path: string) => invoke<string>('image_data', { path }),
  /** The system print dialog ("Print to file" makes a PDF). */
  printPage: () => invoke<void>('print_page'),
  quickOpen: (query: string) => invoke<FileMatch[]>('quick_open', { query }),
  /** Starts a search (an empty query only stops the last one); events arrive on `onevent`. */
  search: (query: string, onevent: (e: SearchEvent) => void) => {
    const results = new Channel<SearchEvent>()
    results.onmessage = onevent
    return invoke<void>('search', { query, results })
  },
  renderMermaid: (source: string, theme: string) => invoke<string>('render_mermaid', { source, theme }),
  /** Rejects when this build has no update server. */
  checkUpdate: () => invoke<UpdateInfo | null>('check_update'),
  /** Replaces the AppImage and restarts maido. */
  installUpdate: () => invoke<void>('install_update'),
  // The Explorer's file actions (N4); paths are workspace-relative.
  fsCreate: (dir: string, name: string, folder: boolean) => invoke<string>('fs_create', { dir, name, folder }),
  fsRename: (path: string, name: string) => invoke<string>('fs_rename', { path, name }),
  /** To the system trash. */
  fsTrash: (paths: string[]) => invoke<void>('fs_trash', { paths }),
  /** Copies (or with `cut`, moves) into `dir`; returns where each went. */
  fsPaste: (paths: string[], dir: string, cut: boolean) => invoke<string[]>('fs_paste', { paths, dir, cut }),
  clipboardRead: () => invoke<string>('clipboard_read'),
  clipboardWrite: (text: string) => invoke<void>('clipboard_write', { text }),
  /** The system folder dialog; true when this window now shows another folder (reload the page). */
  /** Switch this window to a folder: the one at `path`, or one picked in the dialog. */
  openFolder: (path?: string) => invoke<boolean>('open_folder', { path }),
  /** Add a folder beside the workspace's own (picked in a dialog when no `path`). */
  addRoot: (path?: string) => invoke<RootInfo | null>('add_root', { path }),
  removeRoot: (id: number) => invoke<void>('remove_root', { id }),
  gitStatus: () => invoke<GitStatus>('git_status'),
  gitCommit: (message: string) => invoke<boolean>('git_commit', { message }),
  /** Commit, pull (rebasing) and push; resolves with what was done. */
  gitSync: (message: string) => invoke<string>('git_sync', { message }),
  gitInit: () => invoke<void>('git_init'),
  /** A file's text at the last commit ('' when it is new). */
  gitHeadText: (path: string) => invoke<string>('git_head_text', { path }),
}

export interface GitStatus {
  repo: boolean
  branch: string
  upstream: string | null
  ahead: number
  behind: number
  remote: boolean
  /** `code` is `git status --porcelain`'s two letters (staged, unstaged; '??' untracked). */
  changes: { path: string; code: string }[]
}

export const DOC_EXTENSIONS = ['md', 'markdown', 'mmd', 'mermaid', 'tex', 'latex']

export function isDoc(path: string): boolean {
  return DOC_EXTENSIONS.includes(path.split('.').pop()?.toLowerCase() ?? '')
}

/** Source files maido highlights (as `highlight::language` in Rust). */
const CODE_EXTENSIONS = new Set(
  'rs py pyw pyi c h cpp cc cxx c++ hpp hh hxx h++ ino cs java go php json jsonc json5 js mjs cjs jsx ts mts cts tsx sh bash zsh rb lua pl pm r scala hs ml mli erl clj d m html htm css xml xsd xsl yaml yml sql bat cmd'.split(' '),
)

export function isCode(path: string): boolean {
  return CODE_EXTENSIONS.has(path.split('.').pop()?.toLowerCase() ?? '') || /(^|\/)(GNU)?[Mm]akefile$/.test(path)
}
