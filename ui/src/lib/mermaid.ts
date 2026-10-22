// Diagrams in the page: Rust returns SVG (cached, or rendered in the hidden
// helper, H3); it is sanitized again here before insertion (F3). Mermaid
// itself never loads in this page. Each diagram can be saved as PDF, PNG or
// JPG (L3), and is drawn again when the theme changes (L4).
import { api } from './api'
import { downloadButton } from './diagram-export'
import { isDark } from './theme'

type Purify = typeof import('dompurify').default
let purify: Promise<Purify> | null = null

export function theme(): string {
  return isDark() ? 'dark' : 'default'
}

/** Sanitized SVG markup for `source` (in the current theme unless `as` says); throws Mermaid's error message. */
export async function diagram(source: string, as = theme()): Promise<string> {
  purify ??= import('dompurify').then((m) => m.default)
  const [svg, DOMPurify] = await Promise.all([api.renderMermaid(source, as), purify])
  // Flowchart labels are HTML inside <foreignObject>.
  return DOMPurify.sanitize(svg, {
    USE_PROFILES: { svg: true, svgFilters: true, html: true },
    ADD_TAGS: ['foreignObject'],
    HTML_INTEGRATION_POINTS: { foreignobject: true },
  })
}

interface Drawn {
  source: string
  /** The workspace file the diagram is in (exports are offered beside it). */
  path: string
  theme: string
  done: Promise<void>
}
const drawn = new WeakMap<HTMLElement, Drawn>()

/** Replace `pre` (a mermaid fence in `path`) with its diagram, or its source and the error. */
export async function replaceFence(pre: HTMLElement, source: string, path: string): Promise<void> {
  const box = document.createElement('div')
  box.className = 'diagram pending'
  box.textContent = 'Rendering diagram…'
  pre.replaceWith(box)
  await draw(box, source, path)
}

function draw(box: HTMLElement, source: string, path: string): Promise<void> {
  const t = theme()
  const done = (async () => {
    try {
      const svg = await diagram(source)
      if (drawn.get(box)?.theme !== t) return
      box.innerHTML = svg
      box.className = 'diagram'
      box.append(tools(box))
    } catch (e) {
      if (drawn.get(box)?.theme !== t) return
      box.className = 'diagram error'
      box.textContent = ''
      const msg = document.createElement('p')
      msg.textContent = `Mermaid: ${String(e)}`
      const pre = document.createElement('pre')
      pre.textContent = source
      box.append(msg, pre)
    }
  })()
  drawn.set(box, { source, path, theme: t, done })
  return done
}

/** Draw every diagram under `root` again if the theme changed; resolves when all are done. */
export function redrawDiagrams(root: ParentNode): Promise<void> {
  const t = theme()
  const jobs: Promise<void>[] = []
  for (const box of root.querySelectorAll<HTMLElement>('.diagram')) {
    const d = drawn.get(box)
    if (!d) continue
    jobs.push(d.theme === t ? d.done : draw(box, d.source, d.path))
  }
  return Promise.all(jobs).then(() => {})
}

/** With a dark theme, give each diagram under `root` a light copy that only prints (L2). */
export async function printCopies(root: ParentNode): Promise<void> {
  if (theme() === 'default') return
  const jobs = [...root.querySelectorAll<HTMLElement>('.diagram')].map(async (box) => {
    const d = drawn.get(box)
    if (!d || box.querySelector(':scope > .print-only') || box.classList.contains('error')) return
    const copy = document.createElement('div')
    copy.className = 'print-only'
    copy.innerHTML = await diagram(d.source, 'default').catch(() => '')
    if (copy.innerHTML) box.append(copy)
  })
  await Promise.all(jobs)
}

function tools(box: HTMLElement): HTMLElement {
  const bar = document.createElement('div')
  bar.className = 'diagram-tools'
  bar.append(downloadButton(() => ({ svg: box.querySelector('svg'), path: drawn.get(box)?.path ?? '' })))
  return bar
}
