<script lang="ts">
  // The window: workspace tree and outline on the left, tabs and the open
  // document on the right (E1), terminals beside or below it (D1). A document
  // can be edited with a live preview beside it (B5); diff tabs compare files
  // and text (E5). Ctrl+O opens files, Ctrl+P prints, Ctrl+F finds in the
  // document, Ctrl+Shift+F searches the workspace, Ctrl+Shift+P runs commands
  // (E1, L6). Tabs, scroll positions, toggles and the panel's place
  // are remembered per workspace; terminal sessions, edits and diffs are not.
  // Settings (theme, reading, terminals, images) are global.
  import { getCurrentWindow } from '@tauri-apps/api/window'
  import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'
  import { onMount, tick } from 'svelte'
  import Document from './Document.svelte'
  import type DiffViewT from './DiffView.svelte'
  import type { DiffSpec } from './DiffView.svelte'
  import type EditorT from './Editor.svelte'
  import type PaletteT from './Palette.svelte'
  import type { Command } from './Palette.svelte'
  import type SearchT from './Search.svelte'
  import Tree from './Tree.svelte'
  import { api, isDoc, type Entry, type FsChange, type OutlineItem, type RootInfo, type Session, type Settings, type UpdateInfo, type WorkspaceInfo } from './lib/api'
  import type { Buffer } from './lib/buffers'
    import type { LinkTarget } from './lib/terminal'
  import { applyTheme } from './lib/theme'
  import { expandFolder, parentOf, refreshFolder } from './lib/tree'
  import type TerminalsPanel from './Terminals.svelte'

  let info = $state<WorkspaceInfo | null>(null)
  let session = $state<Session>({ tabs: [], active: null, scroll: {}, follow: false, all: false, termPos: 'bottom', termSize: 0.35 })
  let settings = $state<Settings>({
    remoteImages: 'ask',
    scrollback: 2000,
    // The DOM renderer: 23 MB less per window than WebGL (K1).
    terminalGpu: false,
    theme: 'system',
    docScale: 1,
    docFont: 'sans',
    docMargin: { top: 24, right: 64, bottom: 24, left: 64 },
    checkUpdates: true,
    updateCheckedAt: 0,
  })
  /** Folders added beside the workspace's own, and which of them are folded in the Explorer (0 is the own one). */
  let roots = $state<RootInfo[]>([])
  let folded = $state<Record<number, boolean>>({})
  let ready = $state(false)
  let sidebar = $state<'files' | 'outline' | 'search'>('files')
  let versions = $state<Record<string, number>>({})
  let deleted = $state<Record<string, boolean>>({})
  let outline = $state<OutlineItem[]>([])
  let remoteImages = $state(0)
  /** This run's answers to the remote-image question, per document (F4). */
  let remoteAnswer = $state<Record<string, boolean>>({})
  let dontAsk = $state(false)
  let anchor = $state<string | null>(null)
  let doc = $state<ReturnType<typeof Document>>()
  // The terminal panel's code (xterm.js) loads with the first terminal.
  let Panel = $state<typeof TerminalsPanel | null>(null)
  let terms = $state<ReturnType<typeof TerminalsPanel>>()
  let termOpen = $state(false)
  let work = $state<HTMLElement>()
  let notice = $state('')
  let noticeTimer: ReturnType<typeof setTimeout> | undefined

  // Editing (B5). CodeMirror loads with the first edit. A buffer lives while
  // its tab is open; `latest` is its text as typed, `preview` the same text a
  // moment later (what the preview renders).
  let Ed = $state<typeof EditorT | null>(null)
  let editorLib: typeof import('./lib/editor') | null = null
  let editor = $state<ReturnType<typeof EditorT>>()
  const buffers = new Map<string, Buffer>()
  const latest = new Map<string, string>()
  const previewTimers = new Map<string, ReturnType<typeof setTimeout>>()
  let editing = $state<Record<string, boolean>>({})
  let dirty = $state<Record<string, boolean>>({})
  let preview = $state<Record<string, string>>({})
  /** Files whose disk copy changed under unsaved edits, or whose save was refused (E3). */
  let conflict = $state<Record<string, boolean>>({})
  /** Asking before unsaved edits are lost: closing one tab, or the window. */
  let pending = $state<{ kind: 'close'; path: string } | { kind: 'quit' } | null>(null)
  let replacing = ''
  // Which side drives scroll sync: the one the pointer is over.
  let syncFrom: 'editor' | 'preview' = 'editor'

  // Quick open and commands (one box), and search: loaded when first used.
  let Pal = $state<typeof PaletteT | null>(null)
  let palette = $state<string | null>(null)
  let Find = $state<typeof SearchT | null>(null)
  let finder = $state<ReturnType<typeof SearchT>>()

  // Diff tabs (E5): ids `diff:<n>`, never saved with the session.
  let Diff = $state<typeof DiffViewT | null>(null)
  let diffs = $state<Record<string, { left: DiffSpec; right: DiffSpec }>>({})
  let diffCount = 0

  const active = $derived(session.active)
  const isDiff = (t: string | null) => !!t && t.startsWith('diff:')
  const activeFile = $derived(active && !isDiff(active) ? active : null)
  const allowRemote = $derived(settings.remoteImages === 'always' || (active != null && remoteAnswer[active] === true))
  const askRemote = $derived(activeFile != null && remoteImages > 0 && settings.remoteImages === 'ask' && remoteAnswer[activeFile] === undefined)
  const anyDirty = $derived(Object.values(dirty).some(Boolean))

  function name(path: string) {
    if (isDiff(path)) return `Diff ${path.slice(5)}`
    return path.slice(path.lastIndexOf('/') + 1)
  }

  function hasPreview(path: string) {
    return isDoc(path)
  }

  function open(path: string, at: string | null = null, line: number | null = null) {
    if (!session.tabs.includes(path)) {
      const i = active ? session.tabs.indexOf(active) + 1 : session.tabs.length
      session.tabs.splice(i, 0, path)
    }
    anchor = at
    if (line) {
      session.scroll[path] = line
      const b = buffers.get(path)
      if (b) b.scroll = line
    }
    if (path === active && editing[path] && line) editor?.goToLine(line)
    else if (path === active && at) doc?.scrollToId(at)
    else if (path === active && line) doc?.scrollToLine(line)
    session.active = path
  }

  function say(text: string) {
    notice = text
    clearTimeout(noticeTimer)
    noticeTimer = setTimeout(() => (notice = ''), 6000)
  }

  /** Show the panel; start a session if there is none (or `fresh` asks for one). */
  async function showTerminal(fresh = false) {
    Panel ??= (await import('./Terminals.svelte')).default
    termOpen = true
    await tick()
    if (!terms) return
    if (fresh || terms.count() === 0) await terms.create()
    else terms.focus()
  }

  /** "Send to terminal" (D3): paste into the open session, never start one (F5). */
  function sendToTerminal(code: string) {
    const result = termOpen && terms ? terms.paste(code) : 'none'
    if (result === 'none') say('Open a terminal first (Ctrl+`), then send the command again.')
    else if (result === 'multiline') say('Not pasted: this shell does not take multi-line pastes safely (no bracketed paste), so each line would run.')
  }

  function dragSplit(e: PointerEvent) {
    const handle = e.currentTarget as HTMLElement
    handle.setPointerCapture(e.pointerId)
    const move = (m: PointerEvent) => {
      if (!work) return
      const r = work.getBoundingClientRect()
      const frac = session.termPos === 'right' ? (r.right - m.clientX) / r.width : (r.bottom - m.clientY) / r.height
      session.termSize = Math.min(0.85, Math.max(0.15, frac))
    }
    const up = () => {
      handle.removeEventListener('pointermove', move)
      handle.removeEventListener('pointerup', up)
    }
    handle.addEventListener('pointermove', move)
    handle.addEventListener('pointerup', up)
  }

  // ---- Settings (L4, L5) ----

  const SIDES = ['top', 'right', 'bottom', 'left'] as const
  const FONTS = { sans: "system-ui, -apple-system, 'Segoe UI', sans-serif", serif: "Charter, Georgia, 'Noto Serif', 'DejaVu Serif', serif" }

  $effect(() => {
    const css = document.documentElement.style
    css.setProperty('--doc-scale', String(settings.docScale))
    css.setProperty('--doc-font', FONTS[settings.docFont] ?? FONTS.sans)
    for (const side of SIDES) css.setProperty(`--doc-m-${side}`, `${settings.docMargin[side]}px`)
    applyTheme(settings.theme)
  })

  function saveSettings() {
    void api.saveSettings($state.snapshot(settings))
  }

  function setSetting<K extends keyof Settings>(key: K, value: Settings[K]) {
    settings[key] = value
    saveSettings()
  }

  /** Font size is typed as a percentage, 70 to 200. */
  function setFontPercent(e: Event) {
    const el = e.currentTarget as HTMLInputElement
    const v = Math.min(200, Math.max(70, parseFloat(el.value) || settings.docScale * 100))
    el.value = String(v)
    setSetting('docScale', v / 100)
  }

  /** Margins are whole pixels, 0 to 400. */
  function setMargin(side: (typeof SIDES)[number], e: Event) {
    const el = e.currentTarget as HTMLInputElement
    const v = Math.min(400, Math.max(0, Math.round(parseFloat(el.value))))
    const px = Number.isNaN(v) ? settings.docMargin[side] : v
    el.value = String(px)
    setSetting('docMargin', { ...settings.docMargin, [side]: px })
  }

  function zoom(step: number) {
    // 70% to 200%, in steps of 10%.
    setSetting('docScale', step === 0 ? 1 : Math.min(2, Math.max(0.7, Math.round((settings.docScale + step * 0.1) * 10) / 10)))
  }

  // ---- Finding (E1, L6) ----

  async function showPalette(start: string) {
    Pal ??= (await import('./Palette.svelte')).default
    palette = start
  }

  function findInDocument() {
    if (!activeFile) return
    if (editing[activeFile]) editor?.find()
    else doc?.find()
  }

  async function showSearch() {
    Find ??= (await import('./Search.svelte')).default
    sidebar = 'search'
    await tick()
    finder?.focus()
  }

  // ---- Export (L1–L3) ----

  function isMarkdown(path: string) {
    return /\.(md|markdown)$/i.test(path)
  }

  async function printDoc() {
    if (!activeFile || !isMarkdown(activeFile)) return say('Only Markdown documents print.')
    await doc?.prepareExport()
    await api.printPage().catch((e) => say(`Not printed: ${e}`))
  }

  function commands(): Command[] {
    const f = activeFile
    const list: (Command | false)[] = [
      { title: 'Open file…', key: 'Ctrl+O', run: () => void showPalette('') },
      { title: 'Open folder…', run: () => void openFolder() },
      { title: 'Add folder to workspace…', run: () => void addFolder() },
      { title: 'New file…', key: 'Ctrl+N', run: () => void newItem(here(), false) },
      { title: 'New folder…', run: () => void newItem(here(), true) },
      !!f && { title: 'Find in document', key: 'Ctrl+F', run: findInDocument },
      { title: 'Search in workspace', key: 'Ctrl+Shift+F', run: () => void showSearch() },
      !!f && { title: f && editing[f] ? 'Read this file' : 'Edit this file', key: 'Ctrl+E', run: toggleEdit },
      !!f && buffers.has(f) && { title: 'Save', key: 'Ctrl+S', run: () => void save(f!) },
      anyDirty && { title: 'Save all', run: () => void saveAll() },
      !!active && { title: 'Close tab', key: 'Ctrl+W', run: () => active && close(active) },
      !!f && { title: 'Compare this file with…', run: () => void openDiff({ mode: 'file', path: f!, text: '' }, { mode: 'file', path: '', text: '' }) },
      { title: 'New diff', run: () => void openDiff({ mode: 'file', path: '', text: '' }, { mode: 'file', path: '', text: '' }) },
      !!f && isMarkdown(f) && { title: 'Print as PDF…', key: 'Ctrl+P', run: () => void printDoc() },
      { title: termOpen ? 'Hide terminal' : 'Show terminal', key: 'Ctrl+`', run: () => (termOpen ? (termOpen = false) : void showTerminal()) },
      { title: 'New terminal', key: 'Ctrl+Shift+`', run: () => void showTerminal(true) },
      { title: 'Move terminal panel ' + (session.termPos === 'right' ? 'below' : 'to the right'), run: () => (session.termPos = session.termPos === 'right' ? 'bottom' : 'right') },
      { title: (session.follow ? 'Stop following' : 'Follow') + ' changes', run: () => (session.follow = !session.follow) },
      { title: session.all ? 'Show only documents in the Explorer' : 'Show all files in the Explorer', run: () => (session.all = !session.all) },
      { title: 'Check for updates', run: () => void checkUpdates(true) },
      { title: 'Theme: system', run: () => setSetting('theme', 'system') },
      { title: 'Theme: light', run: () => setSetting('theme', 'light') },
      { title: 'Theme: dark', run: () => setSetting('theme', 'dark') },
      { title: 'Font size: bigger', key: 'Ctrl+=', run: () => zoom(1) },
      { title: 'Font size: smaller', key: 'Ctrl+-', run: () => zoom(-1) },
      { title: 'Font size: normal', key: 'Ctrl+0', run: () => zoom(0) },
      { title: 'Font: sans', run: () => setSetting('docFont', 'sans') },
      { title: 'Font: serif', run: () => setSetting('docFont', 'serif') },
      { title: 'Outline', run: () => (sidebar = 'outline') },
      { title: 'Explorer', run: () => (sidebar = 'files') },
    ]
    return list.filter((c): c is Command => !!c)
  }

  // ---- Editing ----

  async function startEdit(path: string) {
    if (!buffers.has(path)) {
      const [lib, comp] = await Promise.all([import('./lib/editor'), import('./Editor.svelte')])
      editorLib = lib
      Ed ??= comp.default
      let src: { text: string; hash: string }
      try {
        src = await api.loadSource(path)
      } catch (e) {
        say(`Cannot edit: ${e}`)
        return
      }
      buffers.set(path, { path, state: newState(path, src.text), base: src.hash, scroll: session.scroll[path] ?? 1 })
      latest.set(path, src.text)
    }
    editing[path] = true
  }

  function newState(path: string, text: string) {
    const lib = editorLib!
    return lib.createState(text, lib.kindOf(path), { onchange: (t) => edited(path, t), onsave: () => void save(path) })
  }

  function edited(path: string, text: string) {
    latest.set(path, text)
    if (replacing !== path) dirty[path] = true
    clearTimeout(previewTimers.get(path))
    previewTimers.set(
      path,
      setTimeout(() => (preview[path] = text), 120),
    )
  }

  function toggleEdit() {
    if (!activeFile) return
    if (editing[activeFile]) editing[activeFile] = false
    else void startEdit(activeFile)
  }

  /** Save; false if it did not happen (a conflict shows its banner). */
  async function save(path: string, force = false): Promise<boolean> {
    const b = buffers.get(path)
    if (!b) return true
    const text = latest.get(path) ?? ''
    try {
      b.base = await api.saveFile(path, text, b.base, force)
      dirty[path] = false
      delete conflict[path]
      return true
    } catch (e) {
      if (String(e).includes('conflict')) conflict[path] = true
      else say(`Not saved: ${e}`)
      return false
    }
  }

  /** Put the file's text from disk into the buffer (nothing unsaved is lost: callers check). */
  function replaceBuffer(path: string, src: { text: string; hash: string }) {
    const b = buffers.get(path)
    if (!b) return
    b.base = src.hash
    if (path === active && editing[path] && editor) {
      replacing = path
      editor.setText(src.text)
      replacing = ''
    } else b.state = newState(path, src.text)
    latest.set(path, src.text)
    preview[path] = src.text
    dirty[path] = false
    delete conflict[path]
  }

  /** The file changed on disk while open in the editor: reload it, or ask (E3). */
  async function diskChanged(path: string) {
    const b = buffers.get(path)
    if (!b) return
    let src: { text: string; hash: string }
    try {
      src = await api.loadSource(path)
    } catch {
      return
    }
    // Our own save, or no real change.
    if (src.hash === b.base) return
    if (dirty[path]) conflict[path] = true
    else replaceBuffer(path, src)
  }

  async function keepMine(path: string) {
    const b = buffers.get(path)
    if (!b) return
    // The next save overwrites the disk copy: you chose so.
    b.base = (await api.loadSource(path)).hash
    delete conflict[path]
  }

  async function loadTheirs(path: string) {
    replaceBuffer(path, await api.loadSource(path))
  }

  function showConflictDiff(path: string) {
    openDiff({ mode: 'file', path, text: '', label: `${path} (on disk)` }, { mode: 'text', path: '', text: latest.get(path) ?? '', label: 'Your unsaved edits' })
  }

  // ---- Diffs ----

  async function openDiff(left: DiffSpec, right: DiffSpec) {
    Diff ??= (await import('./DiffView.svelte')).default
    const id = `diff:${++diffCount}`
    diffs[id] = { left, right }
    open(id)
  }

  // ---- Tabs ----

  function close(path: string) {
    if (dirty[path]) {
      session.active = path
      pending = { kind: 'close', path }
      return
    }
    forget(path)
  }

  function forget(path: string) {
    const i = session.tabs.indexOf(path)
    if (i < 0) return
    session.tabs.splice(i, 1)
    delete session.scroll[path]
    if (isDiff(path)) delete diffs[path]
    else void api.closeDocument(path)
    buffers.delete(path)
    latest.delete(path)
    delete editing[path]
    delete dirty[path]
    delete preview[path]
    delete conflict[path]
    if (active === path) session.active = session.tabs[Math.min(i, session.tabs.length - 1)] ?? null
  }

  async function answerPending(choice: 'save' | 'discard' | 'cancel') {
    const p = pending
    pending = null
    if (!p || choice === 'cancel') return
    if (p.kind === 'close') {
      if (choice === 'save' && !(await save(p.path))) return
      forget(p.path)
      return
    }
    if (choice === 'save') {
      for (const path of Object.keys(dirty).filter((k) => dirty[k])) {
        if (!(await save(path))) return
      }
    }
    dirty = {}
    await getCurrentWindow().destroy()
  }

  function onchange(c: FsChange) {
    if (c.tree) refreshFolder(parentOf(c.path))
    if (c.kind === 'remove') {
      if (session.tabs.includes(c.path)) deleted[c.path] = true
      return
    }
    delete deleted[c.path]
    if (buffers.has(c.path)) void diskChanged(c.path)
    if (session.tabs.includes(c.path)) versions[c.path] = (versions[c.path] ?? 0) + 1
    // Follow mode: show what was just written (E4).
    if (session.follow && isDoc(c.path) && c.path !== active) open(c.path)
  }

  function answerRemote(allow: boolean) {
    if (!activeFile) return
    remoteAnswer[activeFile] = allow
    if (dontAsk) {
      setSetting('remoteImages', allow ? 'always' : 'never')
    }
  }

  // ---- Updates (M3) ----

  let update = $state<UpdateInfo | null>(null)
  let updating = $state(false)
  const DAY = 24 * 60 * 60 * 1000

  /** At most once a day unless asked; a failed automatic check stays quiet. */
  async function checkUpdates(asked = false) {
    if (!asked && (!settings.checkUpdates || Date.now() - settings.updateCheckedAt < DAY)) return
    settings.updateCheckedAt = Date.now()
    saveSettings()
    try {
      update = await api.checkUpdate()
      if (!update && asked) say('maido is up to date.')
    } catch (e) {
      if (asked) say(`Could not check for updates: ${e}`)
    }
  }

  async function installUpdate() {
    // maido restarts: edits must be saved first.
    if (Object.values(dirty).some(Boolean)) {
      say('Save or discard your edits first: maido restarts to update.')
      return
    }
    updating = true
    try {
      await api.installUpdate()
    } catch (e) {
      updating = false
      say(`The update failed: ${e}`)
    }
  }

  // ---- Menus (N2) ----

  type MenuItem = { label: string; key?: string; off?: boolean; run: () => void } | 'sep'
  type EditTarget = { kind: 'editor' } | { kind: 'field'; el: HTMLInputElement | HTMLTextAreaElement } | { kind: 'terminal' } | { kind: 'page' }

  let menu = $state<'file' | 'edit' | 'settings' | null>(null)
  /** What the Edit menu acts on, taken as it opens (menus never take the focus). */
  let target = $state<EditTarget>({ kind: 'page' })

  function editTarget(): EditTarget {
    const el = document.activeElement
    if (el?.closest('.terms')) return { kind: 'terminal' }
    if ((el instanceof HTMLInputElement && (el.type === 'text' || el.type === 'search')) || el instanceof HTMLTextAreaElement) return { kind: 'field', el }
    if (activeFile && editing[activeFile] && editor && (!el || el === document.body || el.closest('.cm-editor'))) return { kind: 'editor' }
    return { kind: 'page' }
  }

  function openMenu(m: 'file' | 'edit' | 'settings') {
    ctx = null
    if (m === 'edit') target = editTarget()
    menu = m
  }

  /** The folder File > New makes things in: the open file's. */
  function here() {
    return activeFile ? parentOf(activeFile) : ''
  }

  async function saveAll() {
    for (const path of Object.keys(dirty).filter((k) => dirty[k])) {
      if (!(await save(path))) return
    }
  }

  function closeWindow() {
    if (anyDirty) pending = { kind: 'quit' }
    else void getCurrentWindow().destroy()
  }

  /** File > Open folder: this window switches to it (the page starts over). */
  async function openFolder() {
    if (anyDirty) return say('Save or discard your edits first: the window switches folders.')
    clearTimeout(saveTimer)
    await api.saveSession(sessionSnapshot()).catch(() => {})
    try {
      if (await api.openFolder()) {
        ready = false
        location.reload()
      }
    } catch (e) {
      say(`Could not open the folder: ${e}`)
    }
  }

  /** File > Add folder to workspace: the Explorer shows it below the others. */
  async function addFolder() {
    try {
      const r = await api.addRoot()
      if (r) roots.push(r)
    } catch (e) {
      say(`Not added: ${e}`)
    }
  }

  async function removeFolder(r: RootInfo) {
    const path = `@${r.id}`
    if (unsavedUnder([path])) return
    if ((await ask(`Remove ${r.name} from the workspace? Nothing is deleted.`, 'Remove')) === null) return
    try {
      await api.removeRoot(r.id)
      for (const t of tabsUnder(path)) forget(t)
      roots = roots.filter((x) => x.id !== r.id)
    } catch (e) {
      say(`Not removed: ${e}`)
    }
  }

  /** Copy (or cut) the selection to the system clipboard. */
  async function copySelection(cut: boolean) {
    const t = target
    let text = ''
    let remove = () => {}
    if (t.kind === 'editor' && editor) {
      text = editor.selected()
      remove = () => editor?.insert('')
    } else if (t.kind === 'field') {
      const { el } = t
      const [from, to] = [el.selectionStart ?? 0, el.selectionEnd ?? 0]
      text = el.value.slice(from, to)
      remove = () => {
        el.setRangeText('', from, to, 'end')
        el.dispatchEvent(new Event('input', { bubbles: true }))
      }
    } else text = window.getSelection()?.toString() ?? ''
    if (!text) return
    try {
      await api.clipboardWrite(text)
    } catch (e) {
      return say(`Not copied: ${e}`)
    }
    if (cut) remove()
    if (t.kind === 'field') t.el.focus()
  }

  async function pasteText() {
    const t = target
    let text: string
    try {
      text = await api.clipboardRead()
    } catch (e) {
      return say(`Nothing to paste: ${e}`)
    }
    if (t.kind === 'editor') editor?.insert(text)
    else if (t.kind === 'terminal') sendToTerminal(text)
    else if (t.kind === 'field') {
      const { el } = t
      el.setRangeText(text, el.selectionStart ?? el.value.length, el.selectionEnd ?? el.value.length, 'end')
      el.dispatchEvent(new Event('input', { bubbles: true }))
      el.focus()
    }
  }

  function undoRedo(again: boolean) {
    const t = target
    if (t.kind === 'editor') {
      if (again) editor?.redoEdit()
      else editor?.undoEdit()
    } else if (t.kind === 'field') {
      t.el.focus()
      document.execCommand(again ? 'redo' : 'undo')
    }
  }

  function fileMenu(): MenuItem[] {
    const f = activeFile
    return [
      { label: 'New file…', key: 'Ctrl+N', run: () => void newItem(here(), false) },
      { label: 'New folder…', run: () => void newItem(here(), true) },
      'sep',
      { label: 'Open file…', key: 'Ctrl+O', run: () => void showPalette('') },
      { label: 'Open folder…', run: () => void openFolder() },
      { label: 'Add folder to workspace…', run: () => void addFolder() },
      'sep',
      { label: 'Save', key: 'Ctrl+S', off: !f || !dirty[f], run: () => f && void save(f) },
      { label: 'Save all', off: !anyDirty, run: () => void saveAll() },
      'sep',
      { label: 'Print as PDF…', key: 'Ctrl+P', off: !f || !isMarkdown(f), run: () => void printDoc() },
      'sep',
      { label: 'Close tab', key: 'Ctrl+W', off: !active, run: () => active && close(active) },
      { label: 'Close window', run: closeWindow },
    ]
  }

  function editMenu(): MenuItem[] {
    const f = activeFile
    const typing = target.kind === 'editor' || target.kind === 'field'
    return [
      { label: 'Undo', key: 'Ctrl+Z', off: !typing, run: () => undoRedo(false) },
      { label: 'Redo', key: 'Ctrl+Shift+Z', off: !typing, run: () => undoRedo(true) },
      'sep',
      { label: 'Cut', key: 'Ctrl+X', off: !typing, run: () => void copySelection(true) },
      { label: 'Copy', key: 'Ctrl+C', run: () => void copySelection(false) },
      { label: 'Paste', key: 'Ctrl+V', off: target.kind === 'page', run: () => void pasteText() },
      'sep',
      { label: 'Find in document', key: 'Ctrl+F', off: !f, run: findInDocument },
      { label: 'Search in workspace', key: 'Ctrl+Shift+F', run: () => void showSearch() },
      'sep',
      { label: f && editing[f] ? 'Read this file' : 'Edit this file', key: 'Ctrl+E', off: !f, run: toggleEdit },
    ]
  }

  // ---- Explorer file actions (N4) ----

  let ctx = $state<{ x: number; y: number; entry: Entry | null } | null>(null)
  /** Files copied or cut in the Explorer (kept here, not on the system clipboard). */
  let fileClip = $state<{ paths: string[]; cut: boolean } | null>(null)

  function showContext(e: MouseEvent, entry: Entry | null) {
    e.preventDefault()
    menu = null
    ctx = { x: e.clientX, y: e.clientY, entry }
  }

  /** The folder an action on `entry` works in: the folder itself, or the file's. */
  function folderOf(entry: Entry | null) {
    return !entry ? '' : entry.dir ? entry.path : parentOf(entry.path)
  }

  /** Open tabs at `path` or inside it. */
  function tabsUnder(path: string) {
    return session.tabs.filter((t) => !isDiff(t) && (t === path || t.startsWith(path + '/')))
  }

  /** Moving or deleting files with unsaved edits would lose them: refused. */
  function unsavedUnder(paths: string[]) {
    const t = paths.flatMap(tabsUnder).find((t) => dirty[t])
    if (t) say(`Save or close ${name(t)} first: it has unsaved edits.`)
    return !!t
  }

  /** Tabs follow a file or folder that moved from `from` to `to`. */
  function moved(from: string, to: string) {
    for (const t of tabsUnder(from)) {
      const i = session.tabs.indexOf(t)
      const was = active === t
      const next = to + t.slice(from.length)
      forget(t)
      session.tabs.splice(i, 0, next)
      if (was) session.active = next
    }
  }

  async function newItem(dir: string, folder: boolean) {
    const n = await ask(folder ? 'New folder' : 'New file', 'Create', folder ? '' : 'untitled.md')
    if (!n) return
    try {
      const path = await api.fsCreate(dir, n, folder)
      refreshFolder(dir)
      expandFolder(dir)
      if (!folder) {
        open(path)
        void startEdit(path)
      }
    } catch (e) {
      say(`Not created: ${e}`)
    }
  }

  async function renameEntry(entry: Entry) {
    const n = await ask(`Rename ${entry.name}`, 'Rename', entry.name)
    if (!n || n === entry.name || unsavedUnder([entry.path])) return
    try {
      moved(entry.path, await api.fsRename(entry.path, n))
      refreshFolder(parentOf(entry.path))
    } catch (e) {
      say(`Not renamed: ${e}`)
    }
  }

  async function trashEntry(entry: Entry) {
    if (unsavedUnder([entry.path])) return
    if ((await ask(`Move ${entry.name} to the trash?`, 'Move to trash')) === null) return
    try {
      await api.fsTrash([entry.path])
      for (const t of tabsUnder(entry.path)) forget(t)
      refreshFolder(parentOf(entry.path))
    } catch (e) {
      say(`Not moved to the trash: ${e}`)
    }
  }

  async function pasteFiles(dir: string) {
    const clip = fileClip
    if (!clip || (clip.cut && unsavedUnder(clip.paths))) return
    try {
      const to = await api.fsPaste(clip.paths, dir, clip.cut)
      if (clip.cut) {
        clip.paths.forEach((p, i) => {
          if (to[i] !== p) moved(p, to[i])
          refreshFolder(parentOf(p))
        })
        fileClip = null
      }
      refreshFolder(dir)
      expandFolder(dir)
    } catch (e) {
      say(`Not pasted: ${e}`)
    }
  }

  function contextMenu(entry: Entry | null): MenuItem[] {
    const dir = folderOf(entry)
    const clip = fileClip
    const root = entry && /^@\d+$/.test(entry.path) ? roots.find((r) => `@${r.id}` === entry.path) : undefined
    if (root) {
      return [
        { label: 'New file…', run: () => void newItem(dir, false) },
        { label: 'New folder…', run: () => void newItem(dir, true) },
        { label: 'Paste', off: !clip, run: () => void pasteFiles(dir) },
        'sep',
        { label: 'Remove from workspace', run: () => void removeFolder(root) },
        { label: 'Copy path', run: () => void api.clipboardWrite(root.path).catch((e) => say(`Not copied: ${e}`)) },
      ]
    }
    const items: MenuItem[] = [{ label: 'New file…', run: () => void newItem(dir, false) }, { label: 'New folder…', run: () => void newItem(dir, true) }, 'sep']
    if (entry) {
      items.push({ label: 'Cut', run: () => (fileClip = { paths: [entry.path], cut: true }) })
      items.push({ label: 'Copy', run: () => (fileClip = { paths: [entry.path], cut: false }) })
    }
    items.push({ label: clip ? `Paste ${name(clip.paths[0])}` : 'Paste', off: !clip, run: () => void pasteFiles(dir) })
    if (entry) {
      items.push('sep')
      items.push({ label: 'Rename…', key: 'F2', run: () => void renameEntry(entry) })
      items.push({ label: 'Move to trash', key: 'Delete', run: () => void trashEntry(entry) })
      items.push('sep')
      items.push({ label: 'Copy path', run: () => void api.clipboardWrite(entry.path).catch((e) => say(`Not copied: ${e}`)) })
    }
    return items
  }

  /** The tree row with the focus, as an entry (F2 and Delete act on it). */
  function focusedEntry(el: Element | null): Entry | null {
    const row = el?.closest<HTMLElement>('.tree-row')
    const path = row?.dataset.path
    return path ? { path, name: path.slice(path.lastIndexOf('/') + 1), dir: row.classList.contains('dir') } : null
  }

  // A question in a small dialog: a name (`value`), or OK alone (null `value`,
  // answered with ''). Cancel answers null.
  let asking = $state<{ title: string; ok: string; input: boolean; value: string; resolve: (v: string | null) => void } | null>(null)

  function ask(title: string, ok: string, value: string | null = null): Promise<string | null> {
    asking?.resolve(null)
    return new Promise((resolve) => (asking = { title, ok, input: value !== null, value: value ?? '', resolve }))
  }

  function answer(v: string | null) {
    const a = asking
    asking = null
    a?.resolve(v === null ? null : v.trim())
  }

  /** Focus the name, its extension left out of the selection. */
  function pickName(node: HTMLInputElement) {
    node.focus()
    const dot = node.value.lastIndexOf('.')
    node.setSelectionRange(0, dot > 0 ? dot : node.value.length)
  }

  function onpointerdown(e: PointerEvent) {
    const el = e.target as Element
    if (menu && !el.closest('.menubar')) menu = null
    if (ctx && !el.closest('.ctx-menu')) ctx = null
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && (menu || ctx)) {
      menu = null
      ctx = null
      return
    }
    const row = !e.ctrlKey && !e.altKey && (e.key === 'F2' || e.key === 'Delete') ? focusedEntry(e.target as Element) : null
    if (row) {
      e.preventDefault()
      void (e.key === 'F2' ? renameEntry(row) : trashEntry(row))
      return
    }
    if (!e.ctrlKey || e.defaultPrevented) return
    if (e.code === 'Backquote') {
      e.preventDefault()
      if (e.shiftKey) void showTerminal(true)
      else if (termOpen && (e.target as Element).closest?.('.terms')) termOpen = false
      else void showTerminal()
      return
    }
    // Inside a terminal, Ctrl+W and the like belong to the shell.
    if ((e.target as Element).closest?.('.terms')) return
    if (e.code === 'KeyO' && !e.shiftKey) {
      e.preventDefault()
      void showPalette('')
    } else if (e.code === 'KeyN' && !e.shiftKey) {
      e.preventDefault()
      void newItem(here(), false)
    } else if (e.code === 'KeyP') {
      e.preventDefault()
      if (e.shiftKey) void showPalette('>')
      else void printDoc()
    } else if (e.code === 'KeyF') {
      // Inside the editor, Ctrl+F is CodeMirror's (it never gets here).
      e.preventDefault()
      if (e.shiftKey) void showSearch()
      else findInDocument()
    } else if ((e.code === 'Equal' || e.code === 'NumpadAdd' || e.code === 'Minus' || e.code === 'NumpadSubtract' || e.code === 'Digit0') && !e.altKey) {
      e.preventDefault()
      zoom(e.code === 'Digit0' ? 0 : e.code === 'Equal' || e.code === 'NumpadAdd' ? 1 : -1)
    } else if (e.key === 'w' && active) {
      e.preventDefault()
      close(active)
    } else if (e.key === 'e' && activeFile) {
      e.preventDefault()
      toggleEdit()
    } else if (e.key === 's' && activeFile && buffers.has(activeFile)) {
      e.preventDefault()
      void save(activeFile)
    } else if (e.key === 'Tab' && session.tabs.length > 1 && active) {
      e.preventDefault()
      const n = session.tabs.length
      session.active = session.tabs[(session.tabs.indexOf(active) + (e.shiftKey ? n - 1 : 1)) % n]
    }
  }

  onMount(() => {
    const win = getCurrentWebviewWindow()
    const unlisten = [
      win.listen<FsChange>('fs-change', (e) => onchange(e.payload)),
      win.listen<string>('open-file', (e) => open(e.payload)),
      // Unsaved edits are never lost by closing the window (E3).
      getCurrentWindow().onCloseRequested((e) => {
        if (Object.values(dirty).some(Boolean)) {
          e.preventDefault()
          pending = { kind: 'quit' }
        }
      }),
    ]
    void (async () => {
      const style = document.createElement('style')
      style.textContent = await api.highlightCss()
      document.head.append(style)

      info = await api.workspaceInfo()
      roots = info.roots
      const saved = await api.loadSession().catch(() => null)
      if (saved) session = { ...session, ...saved, scroll: saved.scroll ?? {} }
      const s = await api.loadSettings().catch(() => null)
      if (s) settings = { ...settings, ...s }
      if (info.initial) open(info.initial)
      else if (!session.tabs.length) {
        const top = await api.listDir('', false).catch(() => [])
        const readme = top.find((e) => !e.dir && /^readme\.(md|markdown)$/i.test(e.name))
        if (readme) open(readme.path)
      }
      if (session.active && !session.tabs.includes(session.active)) session.active = session.tabs[0] ?? null
      ready = true
      void checkUpdates()
    })()
    return () => unlisten.forEach((u) => u.then((f) => f()))
  })

  let saveTimer: ReturnType<typeof setTimeout> | undefined

  /** The session as saved: diff tabs are not kept. */
  function sessionSnapshot(): Session {
    const snapshot = $state.snapshot(session)
    snapshot.tabs = snapshot.tabs.filter((t) => !isDiff(t))
    if (isDiff(snapshot.active)) snapshot.active = snapshot.tabs[0] ?? null
    return snapshot
  }

  $effect(() => {
    const snapshot = sessionSnapshot()
    if (!ready) return
    clearTimeout(saveTimer)
    saveTimer = setTimeout(() => void api.saveSession(snapshot), 400)
  })
</script>

<svelte:window {onkeydown} {onpointerdown} />

{#snippet menuItems(list: MenuItem[], done: () => void)}
  {#each list as item, i (i)}
    {#if item === 'sep'}
      <hr />
    {:else}
      <button
        class="menu-item"
        disabled={item.off}
        onmousedown={(e) => e.preventDefault()}
        onclick={() => {
          done()
          item.run()
        }}
      >
        <span>{item.label}</span>{#if item.key}<kbd>{item.key}</kbd>{/if}
      </button>
    {/if}
  {/each}
{/snippet}

{#snippet menuButton(id: 'file' | 'edit' | 'settings', label: string)}
  <button
    class="menu-btn"
    class:on={menu === id}
    aria-expanded={menu === id}
    onmousedown={(e) => e.preventDefault()}
    onclick={() => (menu === id ? (menu = null) : openMenu(id))}
    onmouseenter={() => menu && menu !== id && openMenu(id)}>{label}</button
  >
{/snippet}

<div class="app">
  <header class="topbar">
    <div class="menubar">
      <div class="menu">
        {@render menuButton('file', 'File')}
        {#if menu === 'file'}
          <div class="menu-list">{@render menuItems(fileMenu(), () => (menu = null))}</div>
        {/if}
      </div>
      <div class="menu">
        {@render menuButton('edit', 'Edit')}
        {#if menu === 'edit'}
          <div class="menu-list">{@render menuItems(editMenu(), () => (menu = null))}</div>
        {/if}
      </div>
      <div class="menu">
        {@render menuButton('settings', 'Settings')}
        {#if menu === 'settings'}
          <div class="menu-list settings-body">
            <h4>General</h4>
            <label class="set-row" title="List every file in the Explorer, hidden ones too (not .git), not only Markdown and Mermaid">
              <span>All files in the Explorer</span>
              <input type="checkbox" bind:checked={session.all} />
            </label>
            <label class="set-row" title="Switch to each document as it changes on disk">
              <span>Follow changes</span>
              <input type="checkbox" bind:checked={session.follow} />
            </label>
            <label class="set-row">
              <span>Theme</span>
              <select bind:value={settings.theme} onchange={saveSettings}>
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
            <label class="set-row">
              <span>Remote images</span>
              <select bind:value={settings.remoteImages} onchange={saveSettings}>
                <option value="ask">Ask</option>
                <option value="always">Always</option>
                <option value="never">Never</option>
              </select>
            </label>
            <h4>Reading</h4>
            <label class="set-row">
              <span>Font</span>
              <select bind:value={settings.docFont} onchange={saveSettings}>
                <option value="sans">Sans</option>
                <option value="serif">Serif</option>
              </select>
            </label>
            <label class="set-row" title="Percent (Ctrl+= / Ctrl+- / Ctrl+0)">
              <span>Font size</span>
              <span class="unit"><input type="number" min="70" max="200" step="0.1" value={Math.round(settings.docScale * 1000) / 10} onchange={setFontPercent} />%</span>
            </label>
            <div class="set-row top">
              <span>Margin</span>
              <div class="margins">
                {#each SIDES as side (side)}
                  <label>
                    <span>{side[0].toUpperCase() + side.slice(1)}</span>
                    <span class="unit"><input type="number" min="0" max="400" step="1" value={settings.docMargin[side]} onchange={(e) => setMargin(side, e)} />px</span>
                  </label>
                {/each}
              </div>
            </div>
            <h4>Terminal</h4>
            <label class="set-row" title="Lines each terminal keeps">
              <span>Terminal backlog</span>
              <span class="unit"><input type="number" min="100" max="100000" step="500" bind:value={settings.scrollback} onchange={saveSettings} />lines</span>
            </label>
            <label class="set-row" title="Recommended for heavy output or rendering, such as build logs or an agent streaming a lot of text: terminals are drawn with WebGL, which is faster. It uses about 23 MB more memory per terminal and applies to terminals opened after you change it.">
              <span>GPU terminals</span>
              <input type="checkbox" bind:checked={settings.terminalGpu} onchange={saveSettings} />
            </label>
            <h4>Updates</h4>
            <label class="set-row" title="Look for a new version once a day">
              <span>Check for updates</span>
              <input type="checkbox" bind:checked={settings.checkUpdates} onchange={saveSettings} />
            </label>
            <button class="set-action" onclick={() => void checkUpdates(true)}>Check now</button>
          </div>
        {/if}
      </div>
    </div>
    <nav class="tabs">
      {#each session.tabs as t (t)}
        <div class="tab" class:on={t === active} class:gone={deleted[t]} title={t}>
          <button class="tab-name" onclick={() => (session.active = t)} onauxclick={(e) => e.button === 1 && close(t)}>
            {#if dirty[t]}<span class="dot" title="Unsaved changes">●</span>{/if}{name(t)}
          </button>
          <button class="tab-close" onclick={() => close(t)} title="Close (Ctrl+W)">×</button>
        </div>
      {/each}
      <span class="tabs-gap"></span>
    </nav>
    <div class="topbar-tools">
      <button class="tabs-btn" onclick={() => openDiff({ mode: 'file', path: activeFile ?? '', text: '' }, { mode: 'file', path: '', text: '' })} title={activeFile ? 'Compare this file with another file or text' : 'Compare two files or texts'}>Diff</button>
      <button class="tabs-btn" onclick={() => (termOpen ? (termOpen = false) : void showTerminal())} title="Terminal (Ctrl+`)">Terminal</button>
      {#if activeFile}
        <span class="tools-sep" aria-hidden="true">|</span>
        <button class="mode-switch" role="switch" aria-checked={!!editing[activeFile]} onclick={toggleEdit} title="Viewer or editor (Ctrl+E)">
          <span class:on={!editing[activeFile]}>Viewer</span>
          <span class:on={editing[activeFile]}>Editor</span>
        </button>
      {/if}
    </div>
  </header>

  <aside class="sidebar">
    <header class="sidebar-head">
      <strong title={info?.root}>{info?.name ?? ''}{#if roots.length} + {roots.length}{/if}</strong>
    </header>
    <div class="sidebar-tabs" role="tablist">
      <button role="tab" aria-selected={sidebar === 'files'} class:on={sidebar === 'files'} onclick={() => (sidebar = 'files')}>Explorer</button>
      <button role="tab" aria-selected={sidebar === 'outline'} class:on={sidebar === 'outline'} onclick={() => (sidebar = 'outline')}>Outline</button>
      <button role="tab" aria-selected={sidebar === 'search'} class:on={sidebar === 'search'} onclick={() => void showSearch()} title="Search the workspace (Ctrl+Shift+F)">Search</button>
    </div>
    <div class="sidebar-body" role="presentation" oncontextmenu={(e) => sidebar === 'files' && showContext(e, null)}>
      {#if sidebar === 'files'}
        {#if ready}
          <button class="root-head" onclick={() => (folded[0] = !folded[0])} title={info?.root}><span class="twisty">{folded[0] ? '▸' : '▾'}</span>{info?.name}</button>
          {#if !folded[0]}
            <Tree path="" all={session.all} {active} marked={ctx?.entry?.path ?? null} onopen={(p) => open(p)} oncontext={showContext} />
          {/if}
          {#each roots as r (r.id)}
            <button class="root-head" onclick={() => (folded[r.id] = !folded[r.id])} oncontextmenu={(e) => showContext(e, { name: r.name, path: `@${r.id}`, dir: true })} class:marked={ctx?.entry?.path === `@${r.id}`} title={r.path}>
              <span class="twisty">{folded[r.id] ? '▸' : '▾'}</span>{r.name}
            </button>
            {#if !folded[r.id]}
              <Tree path={`@${r.id}`} all={session.all} {active} marked={ctx?.entry?.path ?? null} onopen={(p) => open(p)} oncontext={showContext} />
            {/if}
          {/each}
        {/if}
      {:else if sidebar === 'search'}
        <!-- kept below, so results survive switching tabs -->
      {:else if outline.length}
        <ul class="outline">
          {#each outline as h (h.id)}
            <li><button style:padding-left="{h.level - 0.25}em" onclick={() => doc?.scrollToId(h.id)}>{h.text}</button></li>
          {/each}
        </ul>
      {:else}
        <p class="doc-note">No headings.</p>
      {/if}
      {#if Find}
        <div class="search-pane" class:hidden={sidebar !== 'search'}>
          <Find bind:this={finder} onopen={(p, l) => open(p, null, l)} />
        </div>
      {/if}
    </div>
  </aside>

  <main class="main">

    {#if pending?.kind === 'close'}
      <div class="banner warn">
        <span>{name(pending.path)} has unsaved changes.</span>
        <button onclick={() => answerPending('save')}>Save</button>
        <button onclick={() => answerPending('discard')}>Discard</button>
        <button onclick={() => answerPending('cancel')}>Cancel</button>
      </div>
    {:else if pending?.kind === 'quit'}
      <div class="banner warn">
        <span>Unsaved changes in {Object.values(dirty).filter(Boolean).length} file(s).</span>
        <button onclick={() => answerPending('save')}>Save all and close</button>
        <button onclick={() => answerPending('discard')}>Discard and close</button>
        <button onclick={() => answerPending('cancel')}>Cancel</button>
      </div>
    {/if}
    {#if activeFile && conflict[activeFile]}
      <div class="banner warn">
        <span>{name(activeFile)} changed on disk while you were editing it.</span>
        <button onclick={() => keepMine(activeFile!)}>Keep mine</button>
        <button onclick={() => loadTheirs(activeFile!)}>Load theirs</button>
        <button onclick={() => showConflictDiff(activeFile!)}>Show diff</button>
      </div>
    {/if}
    {#if askRemote}
      <div class="banner">
        This document loads {remoteImages} image{remoteImages === 1 ? '' : 's'} from the internet.
        <button onclick={() => answerRemote(true)}>Load images</button>
        <button onclick={() => answerRemote(false)}>Not now</button>
        <label><input type="checkbox" bind:checked={dontAsk} /> Don't ask again</label>
      </div>
    {/if}
    {#if active && deleted[active]}
      <div class="banner">This file was deleted on disk.</div>
    {/if}
    {#if update}
      <div class="banner" title={update.notes}>
        <span>maido {update.version} is available.</span>
        {#if update.install}
          <button onclick={installUpdate} disabled={updating}>{updating ? 'Updating…' : 'Update and restart'}</button>
        {:else}
          <button onclick={() => void api.openExternal(update!.url)}>Download</button>
        {/if}
        <button onclick={() => (update = null)} disabled={updating}>Later</button>
      </div>
    {/if}
    {#if notice}
      <div class="banner">{notice} <button onclick={() => (notice = '')}>OK</button></div>
    {/if}

    <div class="work {session.termPos}" class:with-term={termOpen} bind:this={work} style:--term-size="{session.termSize * 100}%">
      <div class="doc-pane">
        {#if active && isDiff(active) && Diff && diffs[active]}
          {#key active}
            <Diff left={diffs[active].left} right={diffs[active].right} files={session.tabs.filter((t) => !isDiff(t))} />
          {/key}
        {:else if activeFile && editing[activeFile] && Ed && buffers.has(activeFile)}
          <div class="edit-split" class:solo={!hasPreview(activeFile)}>
            <div class="edit-pane" onpointerenter={() => (syncFrom = 'editor')} role="presentation">
              {#key activeFile}
                <Ed
                  bind:this={editor}
                  buffer={buffers.get(activeFile)!}
                  onscrollline={(l) => {
                    if (!activeFile) return
                    session.scroll[activeFile] = l
                    if (syncFrom === 'editor') doc?.scrollToLine(l, true)
                  }}
                />
              {/key}
            </div>
            {#if hasPreview(activeFile)}
              <div class="preview-pane" onpointerenter={() => (syncFrom = 'preview')} role="presentation">
                <Document
                  bind:this={doc}
                  path={activeFile}
                  version={versions[activeFile] ?? 0}
                  remote={allowRemote}
                  line={session.scroll[activeFile] ?? 1}
                  anchor={null}
                  source={preview[activeFile] ?? null}
                  onloaded={(d) => {
                    outline = d.outline
                    remoteImages = d.remoteImages
                  }}
                  onlink={(p, a) => open(p, a)}
                  onscrollline={(l) => {
                    if (syncFrom === 'preview') editor?.scrollToLine(l)
                  }}
                  onsend={sendToTerminal}
                />
              </div>
            {/if}
          </div>
        {:else if activeFile}
          <Document
            bind:this={doc}
            path={activeFile}
            version={versions[activeFile] ?? 0}
            remote={allowRemote}
            line={session.scroll[activeFile] ?? 1}
            {anchor}
            source={dirty[activeFile] ? (preview[activeFile] ?? null) : null}
            onloaded={(d) => {
              outline = d.outline
              remoteImages = d.remoteImages
            }}
            onlink={(p, a) => open(p, a)}
            onscrollline={(l) => {
              if (activeFile) session.scroll[activeFile] = l
            }}
            onsend={sendToTerminal}
          />
        {:else if ready}
          <div class="empty">
            <p>Open a file from the tree.</p>
          </div>
        {/if}
      </div>
      {#if Panel}
        <div class="split" class:hidden={!termOpen} onpointerdown={dragSplit} role="separator" aria-orientation={session.termPos === 'right' ? 'vertical' : 'horizontal'}></div>
        <div class="term-pane" class:hidden={!termOpen}>
          <Panel
            bind:this={terms}
            scrollback={settings.scrollback}
            gpu={settings.terminalGpu}
            position={session.termPos}
            onpath={(t: LinkTarget) => open(t.path, null, t.line)}
            onurl={(u: string) => void api.openExternal(u)}
            onposition={() => (session.termPos = session.termPos === 'right' ? 'bottom' : 'right')}
            onhide={() => (termOpen = false)}
            onempty={() => (termOpen = false)}
          />
        </div>
      {/if}
    </div>
  </main>
</div>

{#if ctx}
  <div class="menu-list ctx-menu" style:left="{Math.min(ctx.x, innerWidth - 230)}px" style:top="{Math.min(ctx.y, innerHeight - 300)}px">
    {@render menuItems(contextMenu(ctx.entry), () => (ctx = null))}
  </div>
{/if}

{#if asking}
  <div class="palette-back" role="presentation" onclick={() => answer(null)}></div>
  <form
    class="palette ask"
    onsubmit={(e) => {
      e.preventDefault()
      answer(asking?.value ?? '')
    }}
  >
    <p>{asking.title}</p>
    {#if asking.input}
      <input bind:value={asking.value} use:pickName spellcheck="false" onkeydown={(e) => e.key === 'Escape' && answer(null)} />
    {/if}
    <div class="ask-buttons">
      <button type="button" onclick={() => answer(null)}>Cancel</button>
      <!-- svelte-ignore a11y_autofocus -->
      <button type="submit" class="primary" autofocus={!asking.input}>{asking.ok}</button>
    </div>
  </form>
{/if}

{#if Pal && palette !== null}
  {#key palette}
    <Pal start={palette} commands={commands()} onopen={(p) => open(p)} onclose={() => (palette = null)} />
  {/key}
{/if}
