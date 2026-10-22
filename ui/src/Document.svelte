<script lang="ts">
  // The open document. Markdown is a keyed list of blocks (C1): a reload keeps
  // every element whose key is unchanged, so only changed blocks are replaced,
  // and those flash briefly (E2). Highlighting and diagrams are filled in as
  // blocks come into view (H3, H4).
  import { onDestroy, tick, untrack } from 'svelte'
  import FindBar from './FindBar.svelte'
  import { api, type BlockView, type DocView, type OutlineItem } from './lib/api'
  import { closeExportMenu } from './lib/diagram-export'
  import { fragment } from './lib/html'
  import { renderMath } from './lib/math'
  import { printCopies, redrawDiagrams, replaceFence } from './lib/mermaid'
  import { onThemeChange } from './lib/theme'
  import MermaidPage from './MermaidPage.svelte'

  interface Props {
    path: string
    /** Bumped when the file changes on disk. */
    version: number
    /** Remote images allowed for this document (F4). */
    remote: boolean
    /** Where to open: a source line, or a heading id. */
    line: number
    anchor: string | null
    onloaded: (info: { outline: OutlineItem[]; remoteImages: number }) => void
    onlink: (path: string, anchor: string | null) => void
    onscrollline: (line: number) => void
    /** "Send to terminal" on a shell code block (D3). */
    onsend: (code: string) => void
    /** The editor's unsaved text: show it instead of the file (live preview, B5). */
    source?: string | null
  }
  let { path, version, remote, line, anchor, onloaded, onlink, onscrollline, onsend, source = null }: Props = $props()

  const SHELL_LANGS = ['bash', 'sh', 'shell', 'zsh', 'console', 'fish']

  let view = $state<DocView | null>(null)
  let blocks = $state<BlockView[]>([])
  let loadedPath = $state('')
  let changed = $state(new Set<string>())
  let error = $state('')
  let container = $state<HTMLElement>()

  $effect(() => {
    const p = path
    void version
    const text = source
    untrack(() => void load(p, text))
  })

  async function load(p: string, text: string | null = null) {
    const same = p === loadedPath && view?.kind === 'markdown'
    const have = same ? blocks.map((b) => b.key) : []
    const keep = same ? topBlock() : null
    let v: DocView
    try {
      v = text == null ? await api.loadDocument(p, have) : await api.previewDocument(p, text, have)
    } catch (e) {
      if (p === path) error = String(e)
      return
    }
    if (p !== path) return
    error = ''
    if (v.kind === 'markdown') {
      const before = new Set(have)
      blocks = v.blocks
      // While typing, every keystroke changes a block: no flashing then.
      if (same && text == null) flash(v.blocks.map((b) => b.key).filter((k) => !before.has(k)))
      onloaded({ outline: v.outline, remoteImages: v.remote_images })
    } else {
      blocks = []
      onloaded({ outline: [], remoteImages: 0 })
    }
    view = v
    loadedPath = p
    await tick()
    if (keep) restore(keep)
    else if (!same) {
      if (anchor) scrollToId(anchor)
      else scrollToLine(line)
    }
  }

  function flash(keys: string[]) {
    if (!keys.length) return
    changed = new Set(keys)
    setTimeout(() => {
      if (keys.every((k) => changed.has(k))) changed = new Set()
    }, 1500)
  }

  function blockElements(): HTMLElement[] {
    return container ? [...container.querySelectorAll<HTMLElement>(':scope > .doc > .block')] : []
  }

  /** The first block at least partly on screen, and how far its top is from the view's top. */
  function topBlock(): { key: string; line: number; offset: number } | null {
    if (!container) return null
    const top = container.getBoundingClientRect().top
    for (const el of blockElements()) {
      const r = el.getBoundingClientRect()
      if (r.bottom > top) return { key: el.dataset.key!, line: Number(el.dataset.line), offset: r.top - top }
    }
    return null
  }

  function restore(at: { key: string; line: number; offset: number }) {
    if (!container) return
    const els = blockElements()
    const el = els.find((e) => e.dataset.key === at.key) ?? els.findLast((e) => Number(e.dataset.line) <= at.line) ?? els[0]
    if (!el) return
    container.scrollTop += el.getBoundingClientRect().top - container.getBoundingClientRect().top - at.offset
  }

  /**
   * Where source line `target` (a fraction goes part way into it) sits in the
   * view: inside its block in proportion, or in the gap between two blocks.
   * Scroll sync between editor and preview reads this (C1).
   */
  function lineTop(target: number): number {
    const els = blockElements()
    const i = els.findLastIndex((e) => Number(e.dataset.line) <= target)
    if (i < 0) return 0
    const el = els[i]
    const [start, end] = [Number(el.dataset.line), Number(el.dataset.end)]
    if (target < end + 1) return el.offsetTop + ((target - start) / (end - start + 1)) * el.offsetHeight
    const bottom = el.offsetTop + el.offsetHeight
    const next = els[i + 1]
    if (!next) return bottom
    const gap = Number(next.dataset.line) - (end + 1)
    return bottom + (gap > 0 ? (target - end - 1) / gap : 0) * (next.offsetTop - bottom)
  }

  /** The source line at the top of the view, with a fraction (the reverse of `lineTop`). */
  function topLine(): number {
    if (!container) return 1
    const raw = container.querySelector<HTMLElement>('pre.raw')
    if (raw) return 1 + Math.max(0, container.scrollTop - raw.offsetTop) / parseFloat(getComputedStyle(raw).lineHeight)
    const y = container.scrollTop
    const el = blockElements().find((e) => e.offsetTop + e.offsetHeight > y)
    if (!el) return 1
    const [start, end] = [Number(el.dataset.line), Number(el.dataset.end)]
    return start + Math.max(0, (y - el.offsetTop) / (el.offsetHeight || 1)) * (end - start + 1)
  }

  let quiet = false
  /** Scroll to source line `target`; `silent` keeps it from being reported back (scroll sync). */
  export function scrollToLine(target: number, silent = false) {
    if (!container) return
    if (silent) {
      quiet = true
      requestAnimationFrame(() => (quiet = false))
    }
    const raw = container.querySelector<HTMLElement>('pre.raw')
    if (raw) container.scrollTop = raw.offsetTop + (target - 1) * parseFloat(getComputedStyle(raw).lineHeight)
    else container.scrollTop = target <= 1 ? 0 : lineTop(target)
  }

  export function scrollToId(id: string) {
    container?.querySelector(`#${CSS.escape(id)}`)?.scrollIntoView({ block: 'start' })
  }

  let scrollQueued = false
  function onscroll() {
    if (scrollQueued || quiet) return
    scrollQueued = true
    requestAnimationFrame(() => {
      scrollQueued = false
      onscrollline(topLine())
    })
  }

  // Blocks with code or diagrams get filled in when they come near the view.
  const observer = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue
        observer.unobserve(e.target)
        void enhance(e.target as HTMLElement)
      }
    },
    { rootMargin: '400px 0px' },
  )

  async function enhance(el: HTMLElement) {
    if (el.dataset.enhanced) return
    el.dataset.enhanced = '1'
    const forPath = loadedPath
    const codes = [...el.querySelectorAll<HTMLElement>('pre > code[class*="language-"]')]
    const isMermaid = (c: HTMLElement) => c.classList.contains('language-mermaid')
    const isMath = (c: HTMLElement) => c.classList.contains('language-math')
    if (el.dataset.code) {
      const html = await api.highlightBlock(forPath, el.dataset.key!).catch(() => [])
      codes.forEach((c, i) => {
        const h = html[i]
        if (h != null && !isMermaid(c) && !isMath(c)) {
          c.innerHTML = h
          c.parentElement!.classList.add('hl-code')
        }
      })
    }
    const pending = codes.filter(isMermaid).map((c) => replaceFence(c.parentElement!, c.textContent ?? '', forPath))
    if (el.dataset.math) pending.push(renderMath(el))
    // The button is maido's, added after sanitizing; the document cannot
    // make one or press it (F5).
    for (const c of codes) {
      const lang = [...c.classList].find((k) => k.startsWith('language-'))?.slice('language-'.length)
      if (!lang || !SHELL_LANGS.includes(lang)) continue
      const send = document.createElement('button')
      send.className = 'send-term'
      send.textContent = 'Send to terminal'
      send.title = 'Paste into the active terminal without running it'
      send.addEventListener('click', (e) => {
        e.stopPropagation()
        onsend(c.textContent ?? '')
      })
      c.parentElement!.append(send)
    }
    await Promise.all(pending)
  }

  /** Every block highlighted, typeset and drawn (export and printing need them all, L2). */
  export async function prepareExport(): Promise<HTMLElement | null> {
    const els = blockElements()
    for (const el of els) observer.unobserve(el)
    await Promise.all(els.map((el) => enhance(el)))
    if (container) await redrawDiagrams(container)
    if (container) await printCopies(container)
    return container?.querySelector<HTMLElement>(':scope > .doc') ?? null
  }

  const unTheme = onThemeChange(() => {
    if (container) void redrawDiagrams(container)
  })
  onDestroy(() => {
    unTheme()
    closeExportMenu()
  })

  // Find in the document (Ctrl+F).
  let finding = $state(false)
  let finder = $state<ReturnType<typeof FindBar>>()
  export function find() {
    if (finding) finder?.focus()
    else finding = true
  }

  function fill(node: HTMLElement, b: BlockView) {
    if (b.html == null) return
    node.append(fragment(b.html, remote))
    if (b.code || b.math || node.querySelector('code.language-mermaid')) observer.observe(node)
    return { destroy: () => observer.unobserve(node) }
  }

  $effect(() => {
    if (!remote || !container) return
    void blocks
    for (const img of container.querySelectorAll<HTMLImageElement>('img[data-remote]')) if (!img.src) img.src = img.dataset.remote!
  })

  async function onclick(e: MouseEvent) {
    const a = (e.target as Element).closest('a')
    if (!a) return
    e.preventDefault()
    const href = a.getAttribute('href') ?? ''
    if (href.startsWith('#')) scrollToId(decodeURIComponent(href.slice(1)))
    else if (/^(https?|mailto):/i.test(href)) await api.openExternal(href)
    else if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return // other schemes are never followed (F3)
    else {
      const target = await api.resolveLink(path, href)
      if (target) onlink(target.path, target.anchor)
      else error = `Not found in the workspace: ${href}`
    }
  }
</script>

<div class="doc-scroll" bind:this={container} {onscroll} {onclick} role="presentation">
  {#if finding && container}
    <div class="find-anchor"><FindBar bind:this={finder} root={container} onclose={() => (finding = false)} /></div>
  {/if}
  {#if error}
    <div class="doc-error">{error}</div>
  {/if}
  {#if view?.kind === 'markdown'}
    {#key loadedPath}
      <article class="doc">
        {#each blocks as b (b.key)}
          <div class="block" class:changed={changed.has(b.key)} data-key={b.key} data-line={b.line} data-end={b.end} data-code={b.code ? '1' : undefined} data-math={b.math ? '1' : undefined} use:fill={b}></div>
        {/each}
      </article>
    {/key}
  {:else if view?.kind === 'mermaid'}
    <MermaidPage source={view.source} {path} />
  {:else if view?.kind === 'text'}
    <pre class="raw">{view.text}</pre>
  {:else if view?.kind === 'binary'}
    <p class="doc-note">Binary file: not shown.</p>
  {/if}
</div>
