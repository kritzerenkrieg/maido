<script lang="ts">
  // A PDF: pdf.js (loaded with the first PDF) draws each page into a canvas as
  // it nears the view and frees the pages far away. Each drawn page gets
  // pdf.js's text layer over it, so text can be selected and copied. Zoom
  // (Ctrl+wheel, Ctrl+= / Ctrl+- / Ctrl+0, the toolbar), page steps
  // (Ctrl+Down / Ctrl+Up), autoscroll (middle click), select all (Ctrl+A) and
  // print; the right-click menu has them all. `load` gives the bytes when
  // they do not come from the file itself (a compiled `.tex`).
  import { onDestroy, tick, untrack } from 'svelte'
  import { api, onPinch } from './lib/api'

  interface Props {
    path: string
    /** Bumped when the file changes on disk (or, with `load`, when there are new bytes). */
    version: number
    load?: () => Promise<ArrayBuffer>
  }
  let { path, version, load }: Props = $props()

  type Lib = typeof import('pdfjs-dist')
  type PdfDoc = import('pdfjs-dist').PDFDocumentProxy
  type PdfTask = import('pdfjs-dist').PDFDocumentLoadingTask
  let lib: Lib | null = null
  let pdf: PdfDoc | null = null
  /** pdf.js frees the document and its worker through the loading task. */
  let task: PdfTask | null = null
  /** Each page's size in points. */
  let pages = $state<{ n: number; w: number; h: number }[]>([])
  let error = $state('')
  let scroller = $state<HTMLElement>()
  let wrap = $state<HTMLElement>()

  // ---- Zoom ----

  /** The view's width, which "100%" fits (up to 900 px). */
  let room = $state(0)
  let zoom = $state(1)
  const fitWidth = $derived(Math.max(200, Math.min(900, room - 48)))
  /** Every page is drawn this wide, in CSS pixels. */
  const pageWidth = $derived(Math.round(fitWidth * zoom))
  const ZOOM_MIN = 0.25
  const ZOOM_MAX = 5
  /** A canvas holds at most this many pixels; zoomed past it, the page is stretched. */
  const MAX_PIXELS = 8_000_000

  $effect(() => {
    const p = path
    void version
    untrack(() => void open(p))
  })

  /** The latest open: an older one finishing late is dropped. */
  let request = 0
  let gone = false

  async function open(p: string) {
    const mine = ++request
    let loading: PdfTask | undefined
    try {
      const [l, worker, bytes] = await Promise.all([import('pdfjs-dist'), import('pdfjs-dist/build/pdf.worker.min.mjs?url'), load ? load() : api.readPdf(p)])
      l.GlobalWorkerOptions.workerSrc = worker.default
      lib = l
      if (mine !== request || gone) return
      loading = l.getDocument({ data: new Uint8Array(bytes) })
      const next = await loading.promise
      if (mine !== request || gone) return void loading.destroy()
      const sizes = await Promise.all(
        Array.from({ length: next.numPages }, async (_, i) => {
          const v = (await next.getPage(i + 1)).getViewport({ scale: 1 })
          return { n: i + 1, w: v.width, h: v.height }
        }),
      )
      if (mine !== request || gone) return void loading.destroy()
      release()
      task = loading
      pdf = next
      error = ''
      pages = sizes
      clearPrint()
    } catch (e) {
      void loading?.destroy().catch(() => {})
      if (mine !== request || gone) return
      // A file that does not open (cut short, still being written) shows the
      // error alone, not the pages of what was open before.
      release()
      pages = []
      error = String(e)
    }
  }

  function release() {
    for (const slot of drawn.values()) slot.cancel()
    drawn.clear()
    void task?.destroy()
    task = pdf = null
  }

  // ---- Drawing ----

  interface Slot {
    cancel: () => void
    /** The page width it was drawn at. */
    width: number
  }
  const drawn = new Map<number, Slot>()
  /** Pages within 800 px of the view, and their elements. */
  const near = new Set<number>()
  const elements = new Map<number, { canvas: HTMLCanvasElement; text: HTMLElement }>()

  async function draw(n: number) {
    const el = elements.get(n)
    if (!pdf || !lib || !el || drawn.get(n)?.width === pageWidth) return
    drawn.get(n)?.cancel()
    const { canvas, text } = el
    const width = pageWidth
    // Marked first, so the same page is not drawn twice while it is being fetched.
    let cancelled = false
    const parts: { cancel: () => void }[] = []
    const slot: Slot = {
      width,
      cancel: () => {
        cancelled = true
        parts.forEach((p) => p.cancel())
      },
    }
    drawn.set(n, slot)
    try {
      const page = await pdf.getPage(n)
      if (cancelled) return
      const base = page.getViewport({ scale: 1 })
      const css = width / base.width
      const dpr = Math.min(window.devicePixelRatio || 1, Math.sqrt(MAX_PIXELS / (base.width * css * base.height * css)))
      const viewport = page.getViewport({ scale: css * dpr })
      // Drawn off screen and swapped in, so a page being redrawn (zoom, a new
      // compile) keeps showing its old picture until the new one is ready.
      const off = document.createElement('canvas')
      off.width = Math.floor(viewport.width)
      off.height = Math.floor(viewport.height)
      const render = page.render({ canvas: off, viewport })
      parts.push(render)
      await render.promise
      if (cancelled) return
      canvas.width = off.width
      canvas.height = off.height
      canvas.getContext('2d')?.drawImage(off, 0, 0)
      off.width = off.height = 0
      // The text layer: invisible text over the picture, for selecting.
      text.replaceChildren()
      const layer = new lib.TextLayer({ textContentSource: page.streamTextContent(), container: text, viewport: page.getViewport({ scale: css }) })
      parts.push(layer)
      await layer.render()
    } catch (e) {
      // Cancelled (scrolled far away, zoomed, file reloaded) is expected;
      // anything else leaves the page free to be drawn again when it next comes near.
      if (drawn.get(n) === slot) drawn.delete(n)
      if (!cancelled && !/cancel/i.test(String(e))) error = `Page ${n}: ${e}`
    }
  }

  function free(n: number) {
    drawn.get(n)?.cancel()
    drawn.delete(n)
    const el = elements.get(n)
    if (!el) return
    el.canvas.width = el.canvas.height = 0
    el.text.replaceChildren()
  }

  /** One page: drawn within 800 px of the view, emptied beyond 2400 px. */
  function page(box: HTMLElement, n: number) {
    const canvas = box.querySelector('canvas')!
    elements.set(n, { canvas, text: box.querySelector<HTMLElement>('.textLayer')! })
    const close = new IntersectionObserver(
      (e) => {
        if (e.some((x) => x.isIntersecting)) {
          near.add(n)
          void draw(n)
        } else near.delete(n)
      },
      { root: scroller, rootMargin: '800px' },
    )
    const far = new IntersectionObserver((e) => e.every((x) => !x.isIntersecting) && free(n), { root: scroller, rootMargin: '2400px' })
    close.observe(box)
    far.observe(box)
    return {
      destroy() {
        close.disconnect()
        far.disconnect()
        // A new document's page n may already have taken the slot.
        if (elements.get(n)?.canvas !== canvas) return
        near.delete(n)
        free(n)
        elements.delete(n)
      },
    }
  }

  // Pages near the view are drawn again at the new size once zooming pauses;
  // until then the old pictures are stretched.
  let redrawTimer: ReturnType<typeof setTimeout> | undefined
  $effect(() => {
    void pageWidth
    clearTimeout(redrawTimer)
    redrawTimer = setTimeout(() => untrack(() => near.forEach((n) => void draw(n))), 180)
  })

  /** Zoom to `next`, keeping the point at (`x`, `y`) in the view (its centre by default) where it is. */
  async function setZoom(next: number, x?: number, y?: number) {
    next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(next * 100) / 100))
    if (!scroller || next === zoom) return
    const r = scroller.getBoundingClientRect()
    const px = x ?? r.left + r.width / 2
    const py = y ?? r.top + r.height / 2
    const at = pageAt(py)
    let keep: { n: number; fx: number; fy: number } | null = null
    if (at) {
      const b = at.el.getBoundingClientRect()
      keep = { n: at.n, fx: (px - b.left) / b.width, fy: (py - b.top) / b.height }
    }
    zoom = next
    await tick()
    const el = keep && pageElement(keep.n)
    if (!el || !keep) return
    const b = el.getBoundingClientRect()
    scroller.scrollLeft += b.left + keep.fx * b.width - px
    scroller.scrollTop += b.top + keep.fy * b.height - py
  }

  const zoomBy = (factor: number, x?: number, y?: number) => void setZoom(zoom * factor, x, y)

  // A touchpad pinch (sent here by App.svelte), around the fingers.
  $effect(() => {
    if (scroller) return onPinch(scroller, () => zoom, (next, x, y) => void setZoom(next, x, y))
  })

  function onwheel(e: WheelEvent) {
    if (!e.ctrlKey) return
    e.preventDefault()
    zoomBy(Math.exp(-e.deltaY / 400), e.clientX, e.clientY)
  }

  // ---- Pages ----

  function pageElement(n: number): HTMLElement | null {
    return scroller?.querySelector<HTMLElement>(`.pdf-page[data-n="${n}"]`) ?? null
  }

  /** The page at height `y` on screen, or the nearest above it. */
  function pageAt(y: number): { n: number; el: HTMLElement } | null {
    if (!scroller) return null
    let found: { n: number; el: HTMLElement } | null = null
    for (const el of scroller.querySelectorAll<HTMLElement>('.pdf-page')) {
      if (el.getBoundingClientRect().top > y) break
      found = { n: Number(el.dataset.n), el }
    }
    return found ?? (pages.length ? { n: 1, el: pageElement(1)! } : null)
  }

  let current = $state(1)
  function trackPage() {
    if (!scroller) return
    const r = scroller.getBoundingClientRect()
    current = pageAt(r.top + r.height / 3)?.n ?? 1
  }

  function goToPage(n: number) {
    const el = pageElement(Math.min(pages.length, Math.max(1, n)))
    if (!el || !scroller) return
    scroller.scrollTop += el.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 12
  }

  /** The next or previous page from the one at the top of the view. */
  function step(by: 1 | -1) {
    if (!scroller) return
    const top = scroller.getBoundingClientRect().top
    const at = pageAt(top + 13)
    if (!at) return
    // Part way into a page, "previous" first goes back to its top.
    const offset = at.el.getBoundingClientRect().top - top - 12
    goToPage(by < 0 && offset < -2 ? at.n : at.n + by)
  }

  function pageInput(e: Event) {
    const el = e.currentTarget as HTMLInputElement
    const n = parseInt(el.value)
    if (n) goToPage(n)
    el.value = String(current)
    el.blur()
  }

  // ---- Selecting and copying ----

  /** Ctrl+A selected the whole document: copy takes every page's text, drawn or not. */
  let all = false

  function selectAll() {
    if (!scroller) return
    const range = document.createRange()
    range.selectNodeContents(scroller.querySelector('.pdf-pages')!)
    const sel = window.getSelection()
    sel?.removeAllRanges()
    sel?.addRange(range)
    all = true
  }

  async function allText(): Promise<string> {
    if (!pdf) return ''
    const doc = pdf
    const out: string[] = []
    for (let n = 1; n <= doc.numPages; n++) {
      const content = await (await doc.getPage(n)).getTextContent()
      out.push(content.items.map((i) => ('str' in i ? i.str + (i.hasEOL ? '\n' : '') : '')).join(''))
    }
    return out.join('\n\n').trim()
  }

  async function copy() {
    const text = all ? await allText() : (window.getSelection()?.toString() ?? '')
    if (!text) return
    await api.clipboardWrite(text).catch(() => {})
  }

  function hasSelection(): boolean {
    const sel = window.getSelection()
    return all || (!!sel && !sel.isCollapsed && !!scroller?.contains(sel.anchorNode))
  }

  // ---- Autoscroll (middle click) ----

  /** Where autoscroll started, and where the pointer is now. */
  let auto = $state<{ x: number; y: number } | null>(null)
  let pointer = { x: 0, y: 0 }
  let autoStart = 0
  let autoFrame = 0

  function startAuto(x: number, y: number) {
    auto = { x, y }
    pointer = { x, y }
    autoStart = performance.now()
    let last = performance.now()
    const run = (now: number) => {
      if (!auto || !scroller) return
      const dt = Math.min(64, now - last)
      last = now
      // A small still zone around the start; past it, speed grows with distance.
      const speed = (d: number) => (Math.abs(d) < 12 ? 0 : Math.sign(d) * ((Math.abs(d) - 12) / 8) ** 1.4)
      scroller.scrollTop += (speed(pointer.y - auto.y) * dt) / 16
      scroller.scrollLeft += (speed(pointer.x - auto.x) * dt) / 16
      autoFrame = requestAnimationFrame(run)
    }
    autoFrame = requestAnimationFrame(run)
  }

  function stopAuto() {
    auto = null
    cancelAnimationFrame(autoFrame)
  }

  function onmousedown(e: MouseEvent) {
    menu = null
    if (auto) {
      e.preventDefault()
      stopAuto()
      return
    }
    if (e.button === 1) {
      // No paste of the primary selection, no other middle-click default.
      e.preventDefault()
      startAuto(e.clientX, e.clientY)
      return
    }
    if (e.button === 0) all = false
  }

  function onmouseup(e: MouseEvent) {
    // Held and dragged: scrolling stops on release. A click keeps it going until the next click.
    if (auto && e.button === 1 && performance.now() - autoStart > 300) stopAuto()
  }

  function onmousemove(e: MouseEvent) {
    pointer = { x: e.clientX, y: e.clientY }
  }

  // ---- Keys and the menu ----

  function onkeydown(e: KeyboardEvent) {
    if (auto) {
      e.preventDefault()
      stopAuto()
      return
    }
    if (e.key === 'Escape' && menu) {
      menu = null
      return
    }
    const mod = e.ctrlKey || e.metaKey
    if (!mod || e.altKey) {
      if (e.key !== 'Shift' && e.key !== 'Control') all = false
      return
    }
    let done = true
    if (e.code === 'KeyA' && !e.shiftKey) selectAll()
    else if (e.code === 'KeyC' && !e.shiftKey) void copy()
    else if (e.key === 'ArrowDown' || e.key === 'PageDown') step(1)
    else if (e.key === 'ArrowUp' || e.key === 'PageUp') step(-1)
    else if (e.code === 'Equal' || e.code === 'NumpadAdd') zoomBy(1.25)
    else if (e.code === 'Minus' || e.code === 'NumpadSubtract') zoomBy(0.8)
    else if (e.code === 'Digit0' || e.code === 'Numpad0') void setZoom(1)
    else if (e.code === 'KeyP' && !e.shiftKey) void print()
    else done = false
    if (done) e.preventDefault()
  }

  type Item = { label: string; key?: string; off?: boolean; run: () => void } | 'sep'
  let menu = $state<{ x: number; y: number; items: Item[] } | null>(null)

  function oncontextmenu(e: MouseEvent) {
    e.preventDefault()
    stopAuto()
    const [x, y] = [e.clientX, e.clientY]
    menu = {
      x: Math.min(x, innerWidth - 240),
      y: Math.min(y, innerHeight - 330),
      items: [
        { label: 'Copy', key: 'Ctrl+C', off: !hasSelection(), run: () => void copy() },
        { label: 'Select all', key: 'Ctrl+A', run: selectAll },
        'sep',
        { label: 'Previous page', key: 'Ctrl+Up', off: current <= 1, run: () => step(-1) },
        { label: 'Next page', key: 'Ctrl+Down', off: current >= pages.length, run: () => step(1) },
        { label: 'Auto scroll', key: 'Middle click', run: () => startAuto(x, y) },
        'sep',
        { label: 'Zoom in', key: 'Ctrl+=', off: zoom >= ZOOM_MAX, run: () => zoomBy(1.25, x, y) },
        { label: 'Zoom out', key: 'Ctrl+-', off: zoom <= ZOOM_MIN, run: () => zoomBy(0.8, x, y) },
        { label: 'Fit width', key: 'Ctrl+0', off: zoom === 1, run: () => void setZoom(1) },
        'sep',
        { label: 'Print…', key: 'Ctrl+P', run: () => void print() },
      ],
    }
  }

  function closeMenu(e: PointerEvent) {
    const el = e.target as Element
    if (menu && !el.closest?.('.pdf-menu')) menu = null
    // A click outside the page ends autoscroll too (inside, `onmousedown` does, and swallows the click).
    if (auto && !scroller?.contains(el)) stopAuto()
  }

  // ---- Printing ----

  /** Every page as a picture that only prints (the canvases on screen are only the pages near the view). */
  let printed = $state<string[]>([])
  let printing: Promise<void> | null = null
  /** Picture size for printing: 2.5 px per point, about 180 dpi. */
  const PRINT_SCALE = 2.5

  function clearPrint() {
    printed.forEach((u) => URL.revokeObjectURL(u))
    printed = []
  }

  /** Draw every page for printing (pages are drawn one at a time, to keep memory down). */
  export function preparePrint(): Promise<void> {
    if (printing) return printing
    const doc = pdf
    printing = (async () => {
      if (!doc) return
      const urls: string[] = []
      const canvas = document.createElement('canvas')
      try {
        for (let n = 1; n <= doc.numPages; n++) {
          const page = await doc.getPage(n)
          const base = page.getViewport({ scale: 1 })
          const viewport = page.getViewport({ scale: Math.min(PRINT_SCALE, Math.sqrt(MAX_PIXELS / (base.width * base.height))) })
          canvas.width = Math.floor(viewport.width)
          canvas.height = Math.floor(viewport.height)
          await page.render({ canvas, viewport }).promise
          const blob = await new Promise<Blob | null>((done) => canvas.toBlob(done, 'image/png'))
          if (blob) urls.push(URL.createObjectURL(blob))
        }
      } finally {
        canvas.width = canvas.height = 0
      }
      if (doc !== pdf) return urls.forEach((u) => URL.revokeObjectURL(u))
      clearPrint()
      printed = urls
      await tick()
      // Pictures decoded before the print dialog takes its copy of the page.
      await Promise.all([...(wrap?.querySelectorAll('.pdf-print img') ?? [])].map((img) => (img as HTMLImageElement).decode().catch(() => {})))
    })().finally(() => (printing = null))
    return printing
  }

  async function print() {
    try {
      await preparePrint()
      await api.printPage()
    } catch (e) {
      error = `Not printed: ${e}`
    }
  }

  // ---- Lifetime ----

  $effect(() => {
    if (!scroller) return
    // The pane's width, not the pages' (which is what zoom makes it).
    const ro = new ResizeObserver(() => (room = scroller!.clientWidth))
    ro.observe(scroller)
    // Keys work at once, unless something else (a terminal, a field) has the focus.
    if (document.activeElement === document.body || !document.activeElement) scroller.focus({ preventScroll: true })
    return () => ro.disconnect()
  })

  onDestroy(() => {
    gone = true
    stopAuto()
    clearTimeout(redrawTimer)
    release()
    clearPrint()
  })
</script>

<svelte:window onpointerdown={closeMenu} />

<div class="pdf-wrap" class:auto={!!auto} bind:this={wrap}>
  <!-- Focusable, so its keys (Ctrl+A, Ctrl+Down, zoom) work after a click on the page. -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
  <div
    class="pdf-scroll"
    bind:this={scroller}
    tabindex="0"
    role="document"
    aria-label="PDF"
    onscroll={trackPage}
    {onwheel}
    {onmousedown}
    {onmouseup}
    {onmousemove}
    {onkeydown}
    {oncontextmenu}
    onauxclick={(e) => e.button === 1 && e.preventDefault()}
  >
    {#if error}
      <p class="doc-note">{error}</p>
    {/if}
    <div class="pdf-pages">
      {#key pages}
        {#each pages as p (p.n)}
          <div class="pdf-page" data-n={p.n} style:width="{pageWidth}px" style:height="{(pageWidth * p.h) / p.w}px" style:--total-scale-factor={pageWidth / p.w} use:page={p.n}>
            <canvas></canvas>
            <div class="textLayer"></div>
          </div>
        {/each}
      {/key}
    </div>
  </div>
  {#if pages.length}
    <div class="pdf-tools" role="toolbar" tabindex="-1" onmousedown={(e) => e.stopPropagation()}>
      <button onclick={() => step(-1)} disabled={current <= 1} title="Previous page (Ctrl+Up)">‹</button>
      <span class="pdf-pageno"><input value={current} onchange={pageInput} onkeydown={(e) => e.key === 'Escape' && (e.currentTarget as HTMLInputElement).blur()} aria-label="Page" /> / {pages.length}</span>
      <button onclick={() => step(1)} disabled={current >= pages.length} title="Next page (Ctrl+Down)">›</button>
      <span class="pdf-tools-sep"></span>
      <button onclick={() => zoomBy(0.8)} disabled={zoom <= ZOOM_MIN} title="Zoom out (Ctrl+-)">−</button>
      <button onclick={() => void setZoom(1)} title="Fit width (Ctrl+0)">{Math.round(zoom * 100)}%</button>
      <button onclick={() => zoomBy(1.25)} disabled={zoom >= ZOOM_MAX} title="Zoom in (Ctrl+=, Ctrl+wheel)">+</button>
    </div>
  {/if}
  {#if auto}
    <div class="pdf-auto-mark" style:left="{auto.x}px" style:top="{auto.y}px" aria-hidden="true"></div>
  {/if}
  {#if printed.length}
    <div class="pdf-print print-only">
      {#each printed as src, i (src)}
        <img {src} alt="Page {i + 1}" />
      {/each}
    </div>
  {/if}
</div>

{#if menu}
  <div class="menu-list ctx-menu pdf-menu" style:left="{menu.x}px" style:top="{menu.y}px" role="menu">
    {#each menu.items as item, i (i)}
      {#if item === 'sep'}
        <hr />
      {:else}
        <button
          class="menu-item"
          disabled={item.off}
          onmousedown={(e) => e.preventDefault()}
          onclick={() => {
            menu = null
            item.run()
          }}><span>{item.label}</span>{#if item.key}<kbd>{item.key}</kbd>{/if}</button
        >
      {/if}
    {/each}
  </div>
{/if}
