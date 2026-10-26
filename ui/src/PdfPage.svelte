<script lang="ts">
  // A PDF file: pdf.js (loaded with the first PDF) draws each page into a
  // canvas as it nears the view, and frees the pages far away.
  import { onDestroy, untrack } from 'svelte'
  import { api } from './lib/api'

  interface Props {
    path: string
    /** Bumped when the file changes on disk. */
    version: number
  }
  let { path, version }: Props = $props()

  type PdfDoc = import('pdfjs-dist').PDFDocumentProxy
  type PdfTask = import('pdfjs-dist').PDFDocumentLoadingTask
  let pdf: PdfDoc | null = null
  /** pdf.js frees the document and its worker through the loading task. */
  let task: PdfTask | null = null
  let pages = $state<{ n: number; ratio: number }[]>([])
  let error = $state('')
  let el = $state<HTMLElement>()
  let width = $state(0)
  let observer: IntersectionObserver | undefined
  const drawn = new Map<number, { cancel: () => void }>()

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
      const [lib, worker, bytes] = await Promise.all([
        import('pdfjs-dist'),
        import('pdfjs-dist/build/pdf.worker.min.mjs?url'),
        api.readPdf(p),
      ])
      lib.GlobalWorkerOptions.workerSrc = worker.default
      if (mine !== request || gone) return
      loading = lib.getDocument({ data: new Uint8Array(bytes) })
      const next = await loading.promise
      if (mine !== request || gone) return void loading.destroy()
      const sizes = await Promise.all(Array.from({ length: next.numPages }, async (_, i) => {
        const v = (await next.getPage(i + 1)).getViewport({ scale: 1 })
        return { n: i + 1, ratio: v.height / v.width }
      }))
      if (mine !== request || gone) return void loading.destroy()
      release()
      task = loading
      pdf = next
      error = ''
      pages = sizes
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
    for (const render of drawn.values()) render.cancel()
    drawn.clear()
    void task?.destroy()
    task = pdf = null
  }

  async function draw(canvas: HTMLCanvasElement, n: number) {
    if (!pdf || drawn.has(n)) return
    // Marked first, so the same page is not drawn twice while it is being fetched.
    const slot = { cancel: () => {} }
    drawn.set(n, slot)
    try {
      const page = await pdf.getPage(n)
      const dpr = window.devicePixelRatio || 1
      const wide = width || canvas.clientWidth || 800
      const viewport = page.getViewport({ scale: (wide / page.getViewport({ scale: 1 }).width) * dpr })
      canvas.width = Math.floor(viewport.width)
      canvas.height = Math.floor(viewport.height)
      const render = page.render({ canvas, viewport })
      slot.cancel = () => render.cancel()
      await render.promise
    } catch (e) {
      // Cancelled (scrolled far away, file reloaded) is expected; anything else
      // leaves the page free to be drawn again when it next comes near.
      if (drawn.get(n) === slot) drawn.delete(n)
      if (!String(e).includes('cancel')) error = `Page ${n}: ${e}`
    }
  }

  function free(canvas: HTMLCanvasElement, n: number) {
    drawn.get(n)?.cancel()
    drawn.delete(n)
    canvas.width = canvas.height = 0
  }

  /** One page's canvas: drawn within 800 px of the view, emptied beyond 2400 px. */
  function page(canvas: HTMLCanvasElement, n: number) {
    const near = new IntersectionObserver((e) => e.some((x) => x.isIntersecting) && void draw(canvas, n), { rootMargin: '800px' })
    const far = new IntersectionObserver((e) => e.every((x) => !x.isIntersecting) && free(canvas, n), { rootMargin: '2400px' })
    near.observe(canvas)
    far.observe(canvas)
    return {
      destroy() {
        near.disconnect()
        far.disconnect()
      },
    }
  }

  onDestroy(() => {
    gone = true
    observer?.disconnect()
    release()
  })
</script>

<div class="pdf-pages" bind:this={el} bind:clientWidth={width}>
  {#if error}
    <p class="doc-note">{error}</p>
  {/if}
  {#key pages}
    {#each pages as p (p.n)}
      <canvas class="pdf-page" style:aspect-ratio="1 / {p.ratio}" use:page={p.n}></canvas>
    {/each}
  {/key}
</div>
