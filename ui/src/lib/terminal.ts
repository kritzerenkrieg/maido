// One terminal session: xterm.js drawing a PTY that Rust runs (D1). This
// module, xterm.js and its CSS load with the first terminal.
import { Channel, invoke } from '@tauri-apps/api/core'
import { FitAddon } from '@xterm/addon-fit'
import { Terminal, type ILink } from '@xterm/xterm'
import '@xterm/xterm/css/xterm.css'

export interface LinkTarget {
  path: string
  line: number | null
}

export interface SessionOptions {
  scrollback: number
  /** Draw with WebGL; false keeps xterm.js's DOM renderer. */
  gpu: boolean
  /** Ctrl+click on a workspace path in the output (D2). */
  onpath: (target: LinkTarget) => void
  onurl: (url: string) => void
  ontitle: (title: string) => void
  onexit: () => void
}

const URL_RE = /https?:\/\/[^\s"'`<>]+/g
// Words that could be paths: something with a slash or a dot-extension,
// optionally followed by :line or :line:col.
const PATH_RE = /[^\s"'`<>()[\]{}|;,]*[\w~][^\s"'`<>()[\]{}|;,]*/g
const MAX_CHECKS_PER_ROW = 16

function colors(): Record<string, string> {
  const css = getComputedStyle(document.documentElement)
  const v = (name: string) => css.getPropertyValue(name).trim()
  return { background: v('--term-bg'), foreground: v('--fg'), cursor: v('--fg'), selectionBackground: v('--term-selection') }
}

export class Session {
  readonly term: Terminal
  private fit = new FitAddon()
  private id = 0
  exited = false

  constructor(private opts: SessionOptions) {
    this.term = new Terminal({
      scrollback: opts.scrollback,
      fontFamily: "ui-monospace, 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace",
      fontSize: 13,
      theme: colors(),
      allowProposedApi: false,
      // Hyperlinks a program writes itself (OSC 8, as Claude Code does): xterm's
      // own handler would call window.open, which does nothing here.
      linkHandler: { activate: (e, uri) => (e.ctrlKey || e.metaKey) && opts.onurl(uri), allowNonHttpProtocols: false },
    })
    this.term.loadAddon(this.fit)
    this.term.onTitleChange(opts.ontitle)
    this.term.registerLinkProvider({ provideLinks: (y, done) => void this.links(y).then(done, () => done(undefined)) })
  }

  async start(el: HTMLElement): Promise<void> {
    this.term.open(el)
    if (this.opts.gpu) try {
      const { WebglAddon } = await import('@xterm/addon-webgl')
      const webgl = new WebglAddon()
      webgl.onContextLoss(() => webgl.dispose())
      this.term.loadAddon(webgl)
    } catch {
      // No WebGL: xterm's DOM renderer stays.
    }
    this.fit.fit()
    const output = new Channel<ArrayBuffer | number[]>()
    output.onmessage = (data) => {
      const bytes = data instanceof ArrayBuffer ? new Uint8Array(data) : new Uint8Array(data)
      if (bytes.length) this.term.write(bytes)
      else this.exit()
    }
    this.id = await invoke<number>('term_open', { cols: this.term.cols, rows: this.term.rows, output })
    this.term.onData((data) => void invoke('term_write', { id: this.id, data }).catch(() => {}))
    this.term.onResize(({ cols, rows }) => void invoke('term_resize', { id: this.id, cols, rows }).catch(() => {}))
  }

  private exit() {
    if (this.exited) return
    this.exited = true
    this.term.write('\r\n\x1b[2m[process exited]\x1b[0m\r\n')
    this.opts.onexit()
  }

  resize(): void {
    if (this.term.element?.offsetParent) this.fit.fit()
  }

  setScrollback(lines: number): void {
    this.term.options.scrollback = lines
  }

  /**
   * Paste text without pressing Enter (D3). Multi-line text is pasted only
   * when the shell takes bracketed paste; otherwise each newline would run
   * a line. Returns false when it was not pasted.
   */
  paste(text: string): boolean {
    const t = text.replace(/\n+$/, '')
    if (this.exited) return false
    if (t.includes('\n') && !this.term.modes.bracketedPasteMode) return false
    this.term.paste(t)
    this.term.focus()
    return true
  }

  focus(): void {
    this.term.focus()
  }

  /** Take the page's colours again (the theme changed, L4). */
  retheme(): void {
    this.term.options.theme = colors()
  }

  dispose(): void {
    void invoke('term_close', { id: this.id }).catch(() => {})
    this.term.dispose()
  }

  /** Links on row `y` (1-based), found when the pointer is over it (D2). */
  private async links(y: number): Promise<ILink[] | undefined> {
    const text = this.term.buffer.active.getLine(y - 1)?.translateToString(true) ?? ''
    const out: ILink[] = []
    const taken: [number, number][] = []
    const range = (start: number, len: number) => ({ start: { x: start + 1, y }, end: { x: start + len, y } })
    const decorations = { underline: true, pointerCursor: true }

    for (const m of text.matchAll(URL_RE)) {
      const url = m[0].replace(/[.,;:!?)\]]+$/, '')
      taken.push([m.index, m.index + url.length])
      out.push({ range: range(m.index, url.length), text: url, decorations, activate: (e) => (e.ctrlKey || e.metaKey) && this.opts.onurl(url) })
    }

    const candidates: { at: number; text: string }[] = []
    for (const m of text.matchAll(PATH_RE)) {
      const word = m[0].replace(/[.:]+$/, '')
      if (!/[/.]/.test(word) || /^\d+(\.\d+)*$/.test(word)) continue
      if (taken.some(([a, b]) => m.index < b && m.index + word.length > a)) continue
      candidates.push({ at: m.index, text: word })
      if (candidates.length >= MAX_CHECKS_PER_ROW) break
    }
    const found = await Promise.all(candidates.map((c) => invoke<LinkTarget | null>('term_link', { id: this.id, text: c.text }).catch(() => null)))
    found.forEach((target, i) => {
      if (!target) return
      const c = candidates[i]
      out.push({ range: range(c.at, c.text.length), text: c.text, decorations, activate: (e) => (e.ctrlKey || e.metaKey) && this.opts.onpath(target) })
    })
    return out.length ? out : undefined
  }
}
