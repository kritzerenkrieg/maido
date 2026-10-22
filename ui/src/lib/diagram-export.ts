// Saving a diagram (L3): one download button opens a small panel: PDF, PNG or
// JPG; a transparent background (not for JPG); the size in percent of the
// diagram's own size, 100% by default. Saved through the save dialog (L1).
import { saveExport } from './export'
import { imagePdf } from './pdf'

export type Format = 'pdf' | 'png' | 'jpg'

/** The format and background last chosen, for the next diagram (this run only). */
const last: { format: Format; transparent: boolean } = { format: 'png', transparent: false }

/** Canvases beyond this many pixels fail in WebKit; refuse before trying. */
const MAX_PIXELS = 64_000_000

export const DOWNLOAD_ICON =
  '<svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true"><path d="M8 2.5v7.5M4.75 6.75 8 10l3.25-3.25M2.75 11.5v2h10.5v-2" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>'

/** The download button for a diagram; `get` gives its SVG and file when clicked. */
export function downloadButton(get: () => { svg: SVGSVGElement | null; path: string }): HTMLButtonElement {
  const b = document.createElement('button')
  b.className = 'diagram-download'
  b.title = 'Download this diagram'
  b.setAttribute('aria-label', 'Download this diagram')
  b.innerHTML = DOWNLOAD_ICON
  b.addEventListener('click', (e) => {
    e.stopPropagation()
    const { svg, path } = get()
    if (svg) openExportMenu(b, svg, path)
  })
  return b
}

let menu: HTMLElement | null = null
let unlisten: (() => void) | null = null

export function closeExportMenu(): void {
  menu?.remove()
  menu = null
  unlisten?.()
  unlisten = null
}

/** The panel under `anchor` for saving `svg` (a diagram in the workspace file `path`). */
export function openExportMenu(anchor: HTMLElement, svg: SVGSVGElement, path: string): void {
  closeExportMenu()
  const m = document.createElement('div')
  m.className = 'export-menu'
  m.setAttribute('role', 'dialog')
  m.setAttribute('aria-label', 'Download diagram')
  m.innerHTML = `
    <div class="export-formats" role="radiogroup" aria-label="Format">
      <label><input type="radio" name="export-format" value="pdf"> PDF</label>
      <label><input type="radio" name="export-format" value="png"> PNG</label>
      <label><input type="radio" name="export-format" value="jpg"> JPG</label>
    </div>
    <label class="export-row"><input type="checkbox" name="transparent"> Transparent background</label>
    <label class="export-row">Size <input type="number" name="size" min="10" max="1000" step="10" value="100"> %</label>
    <div class="export-actions"><button class="primary" name="go">Download</button></div>
    <p class="export-status" aria-live="polite"></p>`
  const radios = [...m.querySelectorAll<HTMLInputElement>('input[name="export-format"]')]
  const transparent = m.querySelector<HTMLInputElement>('input[name="transparent"]')!
  const size = m.querySelector<HTMLInputElement>('input[name="size"]')!
  const go = m.querySelector<HTMLButtonElement>('button[name="go"]')!
  const status = m.querySelector<HTMLElement>('.export-status')!
  const format = () => (radios.find((r) => r.checked)?.value ?? 'png') as Format
  const sync = () => {
    // JPG has no transparency.
    transparent.disabled = format() === 'jpg'
    transparent.parentElement!.classList.toggle('off', transparent.disabled)
  }
  radios.forEach((r) => {
    r.checked = r.value === last.format
    r.addEventListener('change', sync)
  })
  transparent.checked = last.transparent
  sync()

  const run = async () => {
    const pct = Number(size.value)
    if (!Number.isFinite(pct) || pct < 10 || pct > 1000) {
      status.textContent = 'Size: 10 to 1000%.'
      return
    }
    last.format = format()
    last.transparent = transparent.checked
    go.disabled = true
    status.textContent = 'Saving…'
    try {
      const where = await exportDiagram(svg, format(), transparent.checked && format() !== 'jpg', pct / 100, path)
      if (where) {
        status.textContent = `Saved ${where}`
        setTimeout(() => menu === m && closeExportMenu(), 1500)
      } else status.textContent = ''
    } catch (e) {
      status.textContent = `Not saved: ${e instanceof Error ? e.message : String(e)}`
    } finally {
      go.disabled = false
    }
  }
  go.addEventListener('click', () => void run())
  m.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && (e.target as HTMLElement).tagName === 'INPUT') {
      e.preventDefault()
      void run()
    }
  })
  m.addEventListener('click', (e) => e.stopPropagation())
  m.addEventListener('pointerdown', (e) => e.stopPropagation())

  document.body.append(m)
  // Under the button, right-aligned to it, kept inside the window.
  const r = anchor.getBoundingClientRect()
  const w = m.offsetWidth
  const h = m.offsetHeight
  const below = r.bottom + 4 + h <= innerHeight
  m.style.left = `${Math.max(8, Math.min(r.right - w, innerWidth - w - 8))}px`
  m.style.top = `${below ? r.bottom + 4 : Math.max(8, r.top - h - 4)}px`
  menu = m
  m.querySelector<HTMLInputElement>('input:checked')?.focus()

  const onkey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') closeExportMenu()
  }
  const outside = (e: PointerEvent) => {
    if (!m.contains(e.target as Node)) closeExportMenu()
  }
  document.addEventListener('keydown', onkey, true)
  document.addEventListener('pointerdown', outside, true)
  unlisten = () => {
    document.removeEventListener('keydown', onkey, true)
    document.removeEventListener('pointerdown', outside, true)
  }
}

/** The diagram's own size, from its viewBox. */
function sizeOf(svg: SVGSVGElement): { w: number; h: number } {
  const vb = svg.viewBox.baseVal
  if (vb && vb.width && vb.height) return { w: vb.width, h: vb.height }
  const r = svg.getBoundingClientRect()
  return { w: r.width, h: r.height }
}

/** Standalone SVG markup for `svg`, at its own size. */
function svgMarkup(svg: SVGSVGElement): string {
  const copy = svg.cloneNode(true) as SVGSVGElement
  const { w, h } = sizeOf(svg)
  copy.setAttribute('xmlns', 'http://www.w3.org/2000/svg')
  copy.setAttribute('width', String(w))
  copy.setAttribute('height', String(h))
  copy.style.maxWidth = ''
  return new XMLSerializer().serializeToString(copy)
}

/** `svg` drawn at `scale`× its size; on the page's background unless `transparent`. */
async function draw(svg: SVGSVGElement, scale: number, transparent: boolean): Promise<HTMLCanvasElement> {
  const { w, h } = sizeOf(svg)
  const canvas = document.createElement('canvas')
  canvas.width = Math.max(1, Math.round(w * scale))
  canvas.height = Math.max(1, Math.round(h * scale))
  if (canvas.width * canvas.height > MAX_PIXELS) throw new Error('too large at this size; choose a smaller one')
  const img = new Image()
  img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svgMarkup(svg))}`
  await img.decode()
  const ctx = canvas.getContext('2d')!
  if (!transparent) {
    ctx.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--bg').trim() || '#fff'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
  }
  ctx.drawImage(img, 0, 0, canvas.width, canvas.height)
  return canvas
}

async function encode(canvas: HTMLCanvasElement, type: string): Promise<Uint8Array> {
  const blob = await new Promise<Blob | null>((ok) => canvas.toBlob(ok, type, 0.92))
  if (!blob) throw new Error('the diagram could not be drawn')
  return new Uint8Array(await blob.arrayBuffer())
}

/**
 * Save `svg` as `format` at `scale`× its own size (a diagram from the
 * workspace file `path`, so the dialog starts beside it). PDF pages are the
 * diagram's size in points, holding the image at twice that resolution so it
 * stays sharp when printed or zoomed.
 */
export async function exportDiagram(svg: SVGSVGElement, format: Format, transparent: boolean, scale: number, path: string): Promise<string | null> {
  const file = path.slice(path.lastIndexOf('/') + 1)
  const base = file.replace(/\.[^.]*$/, '') || 'diagram'
  const name = /\.(mmd|mermaid)$/i.test(file) ? `${base}.${format}` : `${base}-diagram.${format}`
  let data: Uint8Array
  if (format === 'pdf') {
    const canvas = await draw(svg, scale * 2, transparent)
    const pixels = canvas.getContext('2d')!.getImageData(0, 0, canvas.width, canvas.height).data
    const { w, h } = sizeOf(svg)
    // CSS pixels to points: 96 per inch to 72.
    data = await imagePdf(pixels, canvas.width, canvas.height, w * scale * 0.75, h * scale * 0.75, transparent)
  } else {
    const canvas = await draw(svg, scale, transparent)
    data = await encode(canvas, format === 'png' ? 'image/png' : 'image/jpeg')
  }
  return saveExport(name, path, data)
}
