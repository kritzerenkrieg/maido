<script lang="ts">
  // Full-text search (E1): runs in Rust over the workspace (`.gitignore`
  // respected) and streams results file by file; a new search stops the last.
  import { onDestroy } from 'svelte'
  import { api, type LineMatch } from './lib/api'

  interface Props {
    onopen: (path: string, line: number) => void
  }
  let { onopen }: Props = $props()

  let input = $state<HTMLInputElement>()
  let query = $state('')
  let results = $state<{ path: string; matches: LineMatch[] }[]>([])
  let closed = $state<Record<string, boolean>>({})
  let status = $state('')
  let error = $state('')
  let timer: ReturnType<typeof setTimeout> | undefined
  let run = 0

  export function focus() {
    input?.focus()
    input?.select()
  }

  function start(now = false) {
    clearTimeout(timer)
    const q = query
    // Short queries wait for Enter: one letter matches nearly every line.
    if (!now && q.trim().length < 2) {
      if (!q) void stop()
      return
    }
    timer = setTimeout(() => void begin(q), now ? 0 : 250)
  }

  async function begin(q: string) {
    const mine = ++run
    results = []
    closed = {}
    error = ''
    status = 'Searching…'
    let found = 0
    await api
      .search(q, (e) => {
        if (mine !== run) return
        if (e.kind === 'file') {
          results.push({ path: e.path, matches: e.matches })
          found += e.matches.length
          status = `Searching… ${found} so far`
        } else if (e.kind === 'done') {
          status = e.matches
            ? `${e.matches} ${e.matches === 1 ? 'match' : 'matches'} in ${e.files} ${e.files === 1 ? 'file' : 'files'}${e.truncated ? ' (stopped early: narrow the search)' : ''}`
            : 'No matches.'
        } else {
          error = e.message
          status = ''
        }
      })
      .catch((e) => (error = String(e)))
  }

  async function stop() {
    run++
    results = []
    status = ''
    error = ''
    await api.search('', false, () => {}).catch(() => {})
  }

  /** `text` cut into runs, the matched ranges marked. */
  function marks(m: LineMatch): { text: string; hit: boolean }[] {
    const out: { text: string; hit: boolean }[] = []
    let at = 0
    for (const [s, e] of m.ranges) {
      if (s < at) continue
      if (s > at) out.push({ text: m.text.slice(at, s), hit: false })
      out.push({ text: m.text.slice(s, e), hit: true })
      at = e
    }
    if (at < m.text.length) out.push({ text: m.text.slice(at), hit: false })
    return out
  }

  onDestroy(() => {
    clearTimeout(timer)
    if (status.startsWith('Searching')) void api.search('', false, () => {}).catch(() => {})
  })
</script>

<div class="search">
  <div class="search-box">
    <input
      bind:this={input}
      bind:value={query}
      oninput={() => start()}
      onkeydown={(e) => e.key === 'Enter' && start(true)}
      placeholder="Search the workspace"
      spellcheck="false"
    />
  </div>
  {#if error}<div class="search-error">{error}</div>{/if}
  {#if status}<div class="search-status">{status}</div>{/if}
  <ul class="search-results">
    {#each results as r (r.path)}
      <li>
        <button class="search-file" onclick={() => (closed[r.path] = !closed[r.path])} title={r.path}>
          <span class="caret">{closed[r.path] ? '▸' : '▾'}</span>
          <span class="search-path">{r.path}</span>
          <span class="search-count">{r.matches.length}</span>
        </button>
        {#if !closed[r.path]}
          <ul>
            {#each r.matches as m, i (i)}
              <li>
                <button class="search-line" onclick={() => onopen(r.path, m.line)} title="Line {m.line}">
                  {#each marks(m) as part, k (k)}{#if part.hit}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </li>
    {/each}
  </ul>
</div>
