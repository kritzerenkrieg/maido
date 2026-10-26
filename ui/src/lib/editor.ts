// CodeMirror 6 set up for maido (B5): Markdown and Mermaid colouring, search
// and replace, undo history kept per file while its tab is open. This module
// loads with the first edit.
import { defaultKeymap, history, historyKeymap, indentWithTab, redo, undo } from '@codemirror/commands'
import { markdown } from '@codemirror/lang-markdown'
import { HighlightStyle, StreamLanguage, syntaxHighlighting } from '@codemirror/language'
import { highlightSelectionMatches, openSearchPanel, search, searchKeymap } from '@codemirror/search'
import { EditorState, type Extension } from '@codemirror/state'
import { EditorView, drawSelection, highlightActiveLine, keymap, lineNumbers } from '@codemirror/view'
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
    search({ top: true }),
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
