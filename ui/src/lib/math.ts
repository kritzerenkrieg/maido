// Math (C3): Rust marks math spans while parsing (`data-math-style`), and
// KaTeX loads with the first block that has any. KaTeX runs untrusted: no
// \href, \url or \includegraphics, and errors render in place, never throw.
type Katex = typeof import('katex').default

let katex: Promise<Katex> | null = null

function load(): Promise<Katex> {
  katex ??= Promise.all([import('katex'), import('katex/dist/katex.min.css')]).then(([m]) => m.default)
  return katex
}

/** Typeset every math span under `root` (each is replaced, so a second call skips it). */
export async function renderMath(root: ParentNode): Promise<void> {
  const spans = [...root.querySelectorAll<HTMLElement>('[data-math-style]')]
  if (!spans.length) return
  const k = await load()
  for (const el of spans) {
    const display = el.dataset.mathStyle === 'display'
    // A ```math fence: typeset the whole <pre>, not the <code> inside it.
    const target = el.parentElement?.tagName === 'PRE' ? el.parentElement : el
    const tex = el.textContent ?? ''
    const out = document.createElement('span')
    out.className = 'math'
    out.title = tex
    k.render(tex, out, { displayMode: display, throwOnError: false, trust: false, strict: 'ignore', maxSize: 50, maxExpand: 500 })
    target.replaceWith(out)
  }
}
