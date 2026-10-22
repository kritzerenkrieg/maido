<script lang="ts" module>
  /** A side as the diff tab holds it: a workspace file or text. */
  export interface DiffSpec {
    mode: 'file' | 'text'
    path: string
    text: string
    /** Shown instead of the picker, for sides maido filled in (your unsaved edits). */
    label?: string
  }
</script>

<script lang="ts">
  // The diff window (E5): two sides, each a file or pasted text; side by side
  // or inline; changed words marked inside lines; Markdown also compared as
  // rendered, block by block. Read-only: nothing here writes a file.
  import { onMount, untrack } from 'svelte'
  import { api, type BlockRow, type DiffRow, type DiffSide } from './lib/api'
  import { fragment } from './lib/html'
  import { renderMath } from './lib/math'

  interface Props {
    left: DiffSpec
    right: DiffSpec
    /** Paths offered in the file pickers (the open tabs). */
    files: string[]
  }
  let props: Props = $props()
  // The sides are this tab's own from here on.
  let left = $state(untrack(() => ({ ...props.left })))
  let right = $state(untrack(() => ({ ...props.right })))

  const CONTEXT = 3
  let layout = $state<'split' | 'inline'>('split')
  let rendered = $state(false)
  let rows = $state<DiffRow[] | null>(null)
  let blocks = $state<BlockRow[] | null>(null)
  let error = $state('')
  let open = $state<Record<number, boolean>>({})

  function side(s: DiffSpec): DiffSide {
    return s.mode === 'file' ? { path: s.path.trim() } : { text: s.text }
  }
  const ready = (s: DiffSpec) => (s.mode === 'file' ? s.path.trim() !== '' : true)

  async function compare() {
    error = ''
    open = {}
    try {
      if (rendered) {
        blocks = await api.diffRendered(side(left), side(right))
        rows = null
      } else {
        rows = await api.diffLines(side(left), side(right))
        blocks = null
      }
    } catch (e) {
      error = String(e)
    }
  }

  // Compare straight away when both sides arrive filled in (E3's "show diff").
  onMount(() => {
    if (ready(left) && ready(right) && (left.label || right.label || (left.path && right.path))) void compare()
  })

  const added = $derived(rows?.filter((r) => r.right && r.kind !== 'equal').length ?? 0)
  const removed = $derived(rows?.filter((r) => r.left && r.kind !== 'equal').length ?? 0)

  /** Rows to draw: long unchanged runs fold to their first and last lines. */
  type Item = { row: DiffRow } | { fold: number; count: number }
  const items = $derived.by((): Item[] => {
    if (!rows) return []
    const out: Item[] = []
    let i = 0
    while (i < rows.length) {
      if (rows[i].kind !== 'equal') {
        out.push({ row: rows[i++] })
        continue
      }
      let j = i
      while (j < rows.length && rows[j].kind === 'equal') j++
      const run = j - i
      const head = i === 0 ? 0 : CONTEXT
      const tail = j === rows.length ? 0 : CONTEXT
      if (run > head + tail + 1 && !open[i]) {
        for (let k = i; k < i + head; k++) out.push({ row: rows[k] })
        out.push({ fold: i, count: run - head - tail })
        for (let k = j - tail; k < j; k++) out.push({ row: rows[k] })
      } else for (let k = i; k < j; k++) out.push({ row: rows[k] })
      i = j
    }
    return out
  })

  function html(node: HTMLElement, content: string | null) {
    if (!content) return
    node.append(fragment(content, false))
    void renderMath(node)
  }
</script>

<div class="diff">
  <div class="diff-sides">
    {#each [left, right] as s, i (i)}
      <div class="diff-side">
        <div class="diff-side-head">
          <strong>{i === 0 ? 'Left' : 'Right'}</strong>
          {#if s.label}
            <span class="diff-label">{s.label}</span>
          {:else}
            <select bind:value={s.mode}>
              <option value="file">File</option>
              <option value="text">Text</option>
            </select>
          {/if}
        </div>
        {#if !s.label}
          {#if s.mode === 'file'}
            <input class="diff-path" list="diff-files" placeholder="path/in/workspace.md" bind:value={s.path} onkeydown={(e) => e.key === 'Enter' && compare()} />
          {:else}
            <textarea class="diff-text" placeholder="Paste text here" bind:value={s.text}></textarea>
          {/if}
        {/if}
      </div>
    {/each}
    <datalist id="diff-files">
      {#each props.files as f (f)}<option value={f}></option>{/each}
    </datalist>
  </div>
  <div class="diff-bar">
    <button class="primary" onclick={compare} disabled={!ready(left) || !ready(right)}>Compare</button>
    <label><input type="radio" bind:group={layout} value="split" disabled={rendered} /> Side by side</label>
    <label><input type="radio" bind:group={layout} value="inline" disabled={rendered} /> Inline</label>
    <label title="Markdown compared as rendered, block by block"><input type="checkbox" bind:checked={rendered} onchange={compare} /> Rendered</label>
    {#if rows}<span class="diff-stat"><span class="add">+{added}</span> <span class="del">−{removed}</span></span>{/if}
  </div>
  {#if error}<div class="doc-error">{error}</div>{/if}

  <div class="diff-out">
    {#if rows && rows.every((r) => r.kind === 'equal')}
      <p class="doc-note">The two sides are identical.</p>
    {:else if rows && layout === 'split'}
      <table class="diff-table sbs">
        <colgroup><col class="ln-col" /><col /><col class="ln-col" /><col /></colgroup>
        <tbody>
          {#each items as it, n (n)}
            {#if 'fold' in it}
              <tr class="fold"><td colspan="4"><button onclick={() => (open[it.fold] = true)}>⋯ {it.count} unchanged lines</button></td></tr>
            {:else}
              {@const r = it.row}
              <tr class={r.kind}>
                <td class="ln">{r.left?.line ?? ''}</td>
                <td class="code" class:del={r.left && r.kind !== 'equal'}>{#if r.left}{#each r.left.segments as g, k (k)}<span class:w={g.changed}>{g.text}</span>{/each}{/if}</td>
                <td class="ln">{r.right?.line ?? ''}</td>
                <td class="code" class:add={r.right && r.kind !== 'equal'}>{#if r.right}{#each r.right.segments as g, k (k)}<span class:w={g.changed}>{g.text}</span>{/each}{/if}</td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    {:else if rows}
      <table class="diff-table stacked">
        <colgroup><col class="ln-col" /><col class="ln-col" /><col class="sign-col" /><col /></colgroup>
        <tbody>
          {#each items as it, n (n)}
            {#if 'fold' in it}
              <tr class="fold"><td colspan="4"><button onclick={() => (open[it.fold] = true)}>⋯ {it.count} unchanged lines</button></td></tr>
            {:else}
              {@const r = it.row}
              {#if r.kind === 'equal'}
                <tr><td class="ln">{r.left?.line}</td><td class="ln">{r.right?.line}</td><td class="sign"></td><td class="code">{r.left?.segments.map((g) => g.text).join('')}</td></tr>
              {:else}
                {#if r.left}
                  <tr><td class="ln">{r.left.line}</td><td class="ln"></td><td class="sign del">−</td><td class="code del">{#each r.left.segments as g, k (k)}<span class:w={g.changed}>{g.text}</span>{/each}</td></tr>
                {/if}
                {#if r.right}
                  <tr><td class="ln"></td><td class="ln">{r.right.line}</td><td class="sign add">+</td><td class="code add">{#each r.right.segments as g, k (k)}<span class:w={g.changed}>{g.text}</span>{/each}</td></tr>
                {/if}
              {/if}
            {/if}
          {/each}
        </tbody>
      </table>
    {:else if blocks}
      <div class="diff-blocks">
        {#each blocks as b, n (n)}
          <div class="diff-block {b.kind}">
            <div class="doc diff-cell" class:del={b.left && b.kind !== 'equal'} use:html={b.left}></div>
            <div class="doc diff-cell" class:add={b.right && b.kind !== 'equal'} use:html={b.right}></div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
