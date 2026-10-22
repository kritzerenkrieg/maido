<script lang="ts">
  // A `.mmd` / `.mermaid` file: one diagram filling the view, with zoom
  // (wheel, around the cursor) and pan (drag); double-click resets (C2).
  import { onDestroy, tick } from 'svelte'
  import { DOWNLOAD_ICON, closeExportMenu, openExportMenu } from './lib/diagram-export'
  import { diagram } from './lib/mermaid'
  import { onThemeChange } from './lib/theme'

  /** `path` is the file, for exports (L3). */
  let { source, path }: { source: string; path: string } = $props()

  let svg = $state('')
  let error = $state('')
  let scale = $state(1)
  let x = $state(0)
  let y = $state(0)
  let drag: { x: number; y: number } | null = null
  let page = $state<HTMLElement>()
  let canvas = $state<HTMLElement>()

  // Bumped when the theme changes: the diagram is drawn again (L4).
  let themed = $state(0)
  const unTheme = onThemeChange(() => themed++)
  onDestroy(() => {
    unTheme()
    closeExportMenu()
  })

  function download(e: MouseEvent) {
    e.stopPropagation()
    const el = canvas?.querySelector('svg')
    if (el) openExportMenu(e.currentTarget as HTMLElement, el, path)
  }

  $effect(() => {
    const s = source
    void themed
    diagram(s).then(
      (out) => {
        if (s !== source) return
        const first = !svg
        svg = out
        error = ''
        if (first) void tick().then(fit)
      },
      (e) => {
        if (s === source) error = String(e)
      },
    )
  })

  function onwheel(e: WheelEvent) {
    e.preventDefault()
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect()
    const factor = Math.exp(-e.deltaY / 500)
    const next = Math.min(20, Math.max(0.1, scale * factor))
    const px = e.clientX - r.left
    const py = e.clientY - r.top
    x = px - ((px - x) * next) / scale
    y = py - ((py - y) * next) / scale
    scale = next
  }

  function onpointerdown(e: PointerEvent) {
    drag = { x: e.clientX - x, y: e.clientY - y }
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
  }
  function onpointermove(e: PointerEvent) {
    if (!drag) return
    x = e.clientX - drag.x
    y = e.clientY - drag.y
  }
  /** Fit the diagram in the view (up to 2×), centred. */
  function fit() {
    const el = canvas?.querySelector('svg')
    if (!page || !el) return
    // Mermaid sizes its SVG to the container (width 100%); here it gets its
    // own size, from the viewBox.
    const { width: w, height: h } = el.viewBox.baseVal
    if (!w || !h) return
    el.style.maxWidth = 'none'
    el.style.width = `${w}px`
    el.style.height = `${h}px`
    scale = Math.min(2, (page.clientWidth - 48) / w, (page.clientHeight - 48) / h)
    x = (page.clientWidth - w * scale) / 2
    y = (page.clientHeight - h * scale) / 2
  }

  function reset() {
    fit()
  }
</script>

<div class="mmd-page" bind:this={page} {onwheel} {onpointerdown} {onpointermove} onpointerup={() => (drag = null)} ondblclick={reset} role="presentation">
  {#if error}
    <div class="diagram error"><p>Mermaid: {error}</p><pre>{source}</pre></div>
  {:else if svg}
    <div class="mmd-canvas" bind:this={canvas} style:transform="translate({x}px, {y}px) scale({scale})">{@html svg}</div>
  {:else}
    <p class="doc-note">Rendering diagram…</p>
  {/if}
  <div class="mmd-zoom" onpointerdown={(e) => e.stopPropagation()} role="toolbar" tabindex="-1">
    <button onclick={() => (scale = Math.min(20, scale * 1.25))} title="Zoom in">+</button>
    <button onclick={reset} title="Fit (double-click)">{Math.round(scale * 100)}%</button>
    <button onclick={() => (scale = Math.max(0.1, scale / 1.25))} title="Zoom out">−</button>
    <button class="diagram-download" onclick={download} title="Download this diagram" aria-label="Download this diagram">{@html DOWNLOAD_ICON}</button>
  </div>
</div>
