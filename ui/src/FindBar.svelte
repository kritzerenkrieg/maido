<script lang="ts">
  // Find in the document (Ctrl+F, L6): matches are marked with CSS custom
  // highlights, so the document's DOM is never changed. Case-insensitive
  // unless the query has a capital, like search in files. A match must sit
  // inside one run of text (not across bold or a link edge).
  import { onDestroy, onMount } from 'svelte'

  interface Props {
    /** The element whose text is searched. */
    root: HTMLElement
    onclose: () => void
  }
  let { root, onclose }: Props = $props()

  let input = $state<HTMLInputElement>()
  let query = $state('')
  let ranges: Range[] = []
  let current = $state(-1)
  let total = $state(0)
  const supported = typeof CSS !== 'undefined' && 'highlights' in CSS

  /** Text that is not the document's own: maido's buttons, and math (its text is markup). */
  const SKIP = '.find-bar, .send-term, .diagram-tools, .export-menu, .katex, svg, script, style'

  export function focus() {
    input?.focus()
    input?.select()
  }

  function collect() {
    ranges = []
    const q = query
    if (q) {
      const fold = q === q.toLowerCase()
      const needle = fold ? q.toLowerCase() : q
      const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
        acceptNode: (n) => (n.parentElement?.closest(SKIP) ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT),
      })
      for (let n = walker.nextNode(); n; n = walker.nextNode()) {
        const text = fold ? (n.nodeValue ?? '').toLowerCase() : (n.nodeValue ?? '')
        for (let at = text.indexOf(needle); at >= 0; at = text.indexOf(needle, at + needle.length)) {
          const r = document.createRange()
          r.setStart(n, at)
          r.setEnd(n, at + needle.length)
          ranges.push(r)
        }
      }
    }
    total = ranges.length
    paint()
  }

  function paint() {
    if (!supported) return
    set('find', ranges)
    const r = ranges[current]
    set('find-current', r ? [r] : [])
  }

  /** Changes the named highlight in place: WebKitGTK leaves the old marks
   * painted when a highlight is replaced by a new one. */
  function set(name: string, rs: Range[]) {
    let h = CSS.highlights.get(name)
    if (!h) CSS.highlights.set(name, (h = new Highlight()))
    h.clear()
    for (const r of rs) h.add(r)
  }

  function show() {
    const r = ranges[current]
    if (!r) return
    r.startContainer.parentElement?.scrollIntoView({ block: 'center' })
  }

  function step(by: number) {
    // The document reloaded under us: the old ranges point at removed text.
    if (ranges.some((r) => !r.startContainer.isConnected)) collect()
    if (!total) return
    current = (current + by + total) % total
    paint()
    show()
  }

  function oninput() {
    collect()
    // Jump to the first match below the top of the view, as you type.
    const top = root.closest('.doc-scroll')?.getBoundingClientRect().top ?? 0
    const first = ranges.findIndex((r) => r.getBoundingClientRect().bottom > top)
    current = total ? Math.max(0, first) : -1
    paint()
    show()
  }

  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') step(e.shiftKey ? -1 : 1)
    else if (e.key === 'Escape') onclose()
    else return
    e.preventDefault()
    e.stopPropagation()
  }

  onMount(() => focus())
  onDestroy(() => {
    if (!supported) return
    // Emptied first, for the same WebKitGTK repaint as in `set`.
    set('find', [])
    set('find-current', [])
    CSS.highlights.delete('find')
    CSS.highlights.delete('find-current')
  })
</script>

<div class="find-bar" role="search">
  <input bind:this={input} bind:value={query} {oninput} {onkeydown} placeholder="Find in document" spellcheck="false" aria-label="Find in document" />
  <span class="find-count">{query ? (total ? `${current + 1} of ${total}` : 'No matches') : ''}</span>
  <button onclick={() => step(-1)} disabled={!total} title="Previous (Shift+Enter)" aria-label="Previous match">↑</button>
  <button onclick={() => step(1)} disabled={!total} title="Next (Enter)" aria-label="Next match">↓</button>
  <button onclick={onclose} title="Close (Escape)" aria-label="Close">×</button>
</div>
