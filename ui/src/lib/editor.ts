// CodeMirror 6 set up for maido (B5): Markdown and Mermaid colouring, search
// and replace, undo history kept per file while its tab is open. This module
// loads with the first edit.
import { defaultKeymap, history, historyKeymap, indentWithTab, redo, undo } from '@codemirror/commands'
import { markdown } from '@codemirror/lang-markdown'
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from '@codemirror/language'
import { SearchQuery, closeSearchPanel, findNext, findPrevious, getSearchQuery, highlightSelectionMatches, openSearchPanel, replaceAll, replaceNext, search, searchKeymap, setSearchQuery } from '@codemirror/search'
import { EditorState, type Extension } from '@codemirror/state'
import { EditorView, type Panel, type ViewUpdate, drawSelection, highlightActiveLine, keymap, lineNumbers } from '@codemirror/view'
import { tags as t } from '@lezer/highlight'

const style = HighlightStyle.define([
  { tag: t.heading, fontWeight: '600', color: 'var(--accent)' },
  { tag: t.strong, fontWeight: '600' },
  { tag: t.emphasis, fontStyle: 'italic' },
  { tag: t.strikethrough, textDecoration: 'line-through' },
  { tag: [t.link, t.url], color: 'var(--accent)' },
  { tag: t.monospace, color: 'var(--code-fg)' },
  { tag: [t.quote, t.comment], color: 'var(--muted)' },
  { tag: [t.processingInstruction, t.meta, t.contentSeparator, t.list], color: 'var(--muted)' },
  { tag: t.keyword, color: 'var(--keyword)' },
  { tag: t.string, color: 'var(--string)' },
  { tag: t.operator, color: 'var(--muted)' },
])

// Mermaid has no CodeMirror grammar; this colours its keywords, arrows,
// labels and comments, which is what reading and editing a diagram needs.
const MERMAID_KEYWORDS =
  /^(flowchart|graph|sequenceDiagram|classDiagram|stateDiagram(-v2)?|erDiagram|gantt|pie|journey|gitGraph|mindmap|timeline|quadrantChart|sankey-beta|xychart-beta|block-beta|architecture-beta|subgraph|end|participant|actor|loop|alt|else|opt|par|and|critical|break|note|over|left of|right of|activate|deactivate|class|classDef|style|linkStyle|click|direction|section|title|dateFormat|axisFormat|TD|TB|BT|LR|RL)\b/
const mermaid = StreamLanguage.define<null>({
  token(stream) {
    if (stream.eatSpace()) return null
    if (stream.match(/^%%.*/)) return 'comment'
    if (stream.match(/^"[^"]*"?/)) return 'string'
    if (stream.match(/^\|[^|]*\|?/)) return 'string'
    if (stream.match(/^(-->|---|-\.->|==>|->>|-->>|--x|--o|<-->|-\)|::|:::?|-->|o--|x--)/)) return 'operator'
    if (stream.match(MERMAID_KEYWORDS)) return 'keyword'
    stream.next()
    return null
  },
  tokenTable: { comment: t.comment, string: t.string, operator: t.operator, keyword: t.keyword },
})

// LaTeX has no grammar here either: commands, comments, math, braces and the
// alignment and line-break marks, which is what reading a source needs.
const latex = StreamLanguage.define<{ math: boolean }>({
  startState: () => ({ math: false }),
  token(stream, state) {
    if (stream.eatSpace()) return null
    if (stream.match(/^%.*/)) return 'comment'
    if (stream.match(/^\\(begin|end)\b/)) return 'keyword'
    if (stream.match(/^\\([a-zA-Z@]+\*?|.)/)) return state.math ? 'string' : 'link'
    if (stream.match(/^\$\$?/)) {
      state.math = !state.math
      return 'string'
    }
    if (state.math) {
      stream.next()
      return 'string'
    }
    if (stream.match(/^[{}[\]&]/)) return 'operator'
    stream.next()
    return null
  },
  tokenTable: { comment: t.comment, keyword: t.keyword, string: t.string, operator: t.operator, link: t.link },
})

const COUNT_CAP = 10000

/** Find and replace panel: CodeMirror's, plus "n of total" (the number is a field: type another and Enter to jump), with < and > to step. */
function findPanel(view: EditorView): Panel {
  const make = <K extends keyof HTMLElementTagNameMap>(tag: K, cls: string, text = '', title = '') => {
    const e = document.createElement(tag)
    e.className = cls
    e.textContent = text
    if (title) e.title = title
    return e
  }
  const field = (placeholder: string, label: string) => {
    const i = make('input', 'cm-textfield')
    i.placeholder = placeholder
    i.setAttribute('aria-label', label)
    i.spellcheck = false
    return i
  }
  const button = (text: string, title: string, run: () => void) => {
    const b = make('button', 'cm-button', text, title)
    b.type = 'button'
    b.setAttribute('aria-label', title)
    b.addEventListener('click', run)
    return b
  }
  const toggle = (text: string, title: string) => {
    const label = make('label', 'cm-toggle', '', title)
    const box = make('input', '')
    box.type = 'checkbox'
    label.append(box, text)
    return { label, box }
  }

  const find = field('Find', 'Find')
  find.setAttribute('main-field', 'true')
  const replace = field('Replace', 'Replace with')
  const jump = field('', 'Match number')
  jump.classList.add('cm-jump')
  const total = make('span', 'cm-total')
  const nc = toggle('Aa', 'Match case')
  const re = toggle('.*', 'Regular expression')

  const dom = make('div', 'cm-find')
  dom.append(
    find,
    button('<', 'Previous match (Shift+Enter)', () => findPrevious(view)),
    button('>', 'Next match (Enter)', () => findNext(view)),
    make('span', 'cm-count'),
    nc.label,
    re.label,
    replace,
    button('Replace', 'Replace this match', () => replaceNext(view)),
    button('All', 'Replace every match', () => replaceAll(view)),
    button('×', 'Close (Escape)', () => {
      closeSearchPanel(view)
      view.focus()
    }),
  )
  const count = dom.querySelector<HTMLElement>('.cm-count')!
  count.append(jump, total)

  const commit = () => {
    const q = getSearchQuery(view.state)
    view.dispatch({
      effects: setSearchQuery.of(new SearchQuery({ search: find.value, caseSensitive: nc.box.checked, regexp: re.box.checked, replace: replace.value, wholeWord: q.wholeWord })),
    })
  }
  for (const e of [find, replace, nc.box, re.box]) e.addEventListener('input', commit)
  nc.box.addEventListener('change', commit)
  re.box.addEventListener('change', commit)

  // Which match the selection is, and how many there are (counting stops at COUNT_CAP).
  let shown = 0
  const refresh = (state = view.state) => {
    const q = getSearchQuery(state)
    let n = 0
    let at = 0
    if (q.valid) {
      const sel = state.selection.main
      const cursor = q.getCursor(state)
      for (let r = cursor.next(); !r.done; r = cursor.next()) {
        n++
        if (r.value.from === sel.from && r.value.to === sel.to) at = n
        if (n >= COUNT_CAP) break
      }
    }
    shown = n
    if (document.activeElement !== jump) jump.value = n && at ? String(at) : n ? '–' : '0'
    jump.size = Math.max(2, String(n).length)
    total.textContent = `of ${n}${n >= COUNT_CAP ? '+' : ''}`
    dom.classList.toggle('cm-none', !!q.search && !n)
  }
  const goto = (to: number) => {
    const q = getSearchQuery(view.state)
    if (!q.valid || !shown) return
    const want = Math.min(shown, Math.max(1, Math.round(to)))
    const cursor = q.getCursor(view.state)
    let i = 0
    for (let r = cursor.next(); !r.done; r = cursor.next()) {
      if (++i === want) {
        view.dispatch({ selection: { anchor: r.value.from, head: r.value.to }, effects: EditorView.scrollIntoView(r.value.from, { y: 'center' }) })
        return
      }
    }
  }

  jump.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') {
      const n = Number(jump.value)
      if (Number.isFinite(n)) goto(n)
      view.focus()
    } else if (e.key === 'Escape') find.focus()
    else return
    e.preventDefault()
    e.stopPropagation()
  })
  // A click focuses the field and then places the caret: keep the selection, so typing replaces the number.
  let fresh = false
  jump.addEventListener('focus', () => {
    jump.select()
    fresh = true
  })
  jump.addEventListener('mouseup', (e) => {
    if (fresh) e.preventDefault()
    fresh = false
  })
  jump.addEventListener('blur', () => refresh())
  find.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') (e.shiftKey ? findPrevious : findNext)(view)
    else if (e.key === 'Escape') {
      closeSearchPanel(view)
      view.focus()
    } else return
    e.preventDefault()
  })
  replace.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') replaceNext(view)
    else if (e.key === 'Escape') {
      closeSearchPanel(view)
      view.focus()
    } else return
    e.preventDefault()
  })

  const sync = (state = view.state) => {
    const q = getSearchQuery(state)
    if (find.value !== q.search) find.value = q.search
    if (replace.value !== q.replace) replace.value = q.replace
    nc.box.checked = q.caseSensitive
    re.box.checked = q.regexp
  }
  sync()
  refresh()
  return {
    dom,
    top: true,
    mount() {
      find.focus()
      find.select()
    },
    update(u: ViewUpdate) {
      if (u.transactions.some((tr) => tr.effects.some((e) => e.is(setSearchQuery)))) sync(u.state)
      if (u.docChanged || u.selectionSet || u.transactions.some((tr) => tr.effects.some((e) => e.is(setSearchQuery)))) refresh(u.state)
    },
  }
}

const theme = EditorView.theme({
  '&': { height: '100%', fontSize: 'calc(14px * var(--doc-scale))', backgroundColor: 'var(--bg)', color: 'var(--fg)' },
  '.cm-scroller': { fontFamily: "ui-monospace, 'SF Mono', Menlo, Consolas, 'DejaVu Sans Mono', monospace", lineHeight: '1.55' },
  '.cm-content': { caretColor: 'var(--fg)', padding: '12px 0' },
  '.cm-gutters': { backgroundColor: 'var(--panel)', color: 'var(--muted)', border: 'none' },
  '.cm-activeLine': { backgroundColor: 'color-mix(in srgb, var(--accent) 6%, transparent)' },
  '&.cm-focused .cm-selectionBackground, .cm-selectionBackground': { backgroundColor: 'var(--term-selection) !important' },
  '.cm-cursor': { borderLeftColor: 'var(--fg)' },
  '.cm-panels': { backgroundColor: 'var(--panel)', color: 'var(--fg)' },
  '.cm-searchMatch': { backgroundColor: 'color-mix(in srgb, var(--flash) 70%, transparent)' },
  // The find panel's fields: CodeMirror's own are light-only.
  '.cm-textfield, .cm-button': {
    backgroundColor: 'var(--bg)',
    backgroundImage: 'none',
    color: 'var(--fg)',
    border: '1px solid var(--line)',
    borderRadius: '4px',
  },
  '.cm-button:hover': { color: 'var(--accent)' },
  '.cm-panel.cm-search label': { color: 'var(--muted)' },
  '.cm-find': { display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '4px', padding: '4px 6px', fontSize: '12px' },
  '.cm-find .cm-textfield': { padding: '2px 6px', width: '14em' },
  '.cm-find .cm-jump': { width: 'auto', minWidth: '2.2em', padding: '2px 3px', textAlign: 'center' },
  '.cm-find .cm-count': { display: 'inline-flex', alignItems: 'center', gap: '4px', color: 'var(--muted)' },
  '.cm-find .cm-button': { padding: '1px 8px', cursor: 'pointer' },
  '.cm-find .cm-toggle': { display: 'inline-flex', alignItems: 'center', gap: '2px', color: 'var(--muted)', cursor: 'pointer' },
  '.cm-find.cm-none .cm-count': { color: '#e5484d' },
})

export type Kind = 'markdown' | 'mermaid' | 'latex' | 'text'

export function kindOf(path: string): Kind {
  const ext = path.split('.').pop()?.toLowerCase() ?? ''
  if (ext === 'md' || ext === 'markdown') return 'markdown'
  if (ext === 'mmd' || ext === 'mermaid') return 'mermaid'
  if (ext === 'tex' || ext === 'latex') return 'latex'
  return 'text'
}

export interface Hooks {
  onchange: (text: string) => void
  onsave: () => void
}

/** A fresh editor state for `text`; kept per tab so undo survives switching tabs. */
export function createState(text: string, kind: Kind, hooks: Hooks): EditorState {
  const extensions: Extension[] = [
    lineNumbers(),
    highlightActiveLine(),
    drawSelection(),
    history(),
    search({ top: true, createPanel: findPanel }),
    highlightSelectionMatches(),
    EditorView.lineWrapping,
    syntaxHighlighting(style),
    theme,
    keymap.of([
      { key: 'Mod-s', preventDefault: true, run: () => (hooks.onsave(), true) },
      ...searchKeymap,
      ...historyKeymap,
      indentWithTab,
      ...defaultKeymap,
    ]),
    EditorView.updateListener.of((u) => {
      if (u.docChanged) hooks.onchange(u.state.doc.toString())
    }),
  ]
  if (kind === 'markdown') extensions.push(markdown())
  if (kind === 'mermaid') extensions.push(mermaid)
  if (kind === 'latex') extensions.push(latex)
  return EditorState.create({ doc: text, extensions })
}

export { EditorView, openSearchPanel, redo, undo }
