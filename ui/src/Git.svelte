<script lang="ts">
  // The Git panel (G1): what changed in the workspace folder, commit, and
  // sync (commit, pull, push) through the system's git. Status is read again
  // when the panel opens, when files change and when the window gets focus.
  import { onDestroy, onMount } from 'svelte'
  import { api, type GitStatus } from './lib/api'

  interface Props {
    onopen: (path: string) => void
    /** Compare a file with its last commit. */
    oncompare: (path: string, deleted: boolean) => void
    /** False when files have unsaved edits: a commit would leave them out. */
    cansave: () => boolean
  }
  let { onopen, oncompare, cansave }: Props = $props()

  let status = $state<GitStatus | null>(null)
  let message = $state('')
  let busy = $state('')
  let note = $state('')
  let error = $state('')
  let timer: ReturnType<typeof setTimeout> | undefined

  export async function refresh() {
    try {
      status = await api.gitStatus()
      if (!busy) error = ''
    } catch (e) {
      error = String(e)
    }
  }

  /** Files changed on disk: read the status again soon (a save comes in bursts). */
  export function changed() {
    clearTimeout(timer)
    timer = setTimeout(() => void refresh(), 400)
  }

  const onfocus = () => void refresh()
  onMount(() => {
    void refresh()
    window.addEventListener('focus', onfocus)
  })
  onDestroy(() => {
    clearTimeout(timer)
    window.removeEventListener('focus', onfocus)
  })

  async function run(label: string, job: () => Promise<string>) {
    if (busy || !cansave()) return
    busy = label
    note = ''
    error = ''
    try {
      note = await job()
      message = ''
    } catch (e) {
      error = String(e)
    } finally {
      busy = ''
      await refresh()
    }
  }

  const text = () => message.trim() || `Update ${status?.changes.length === 1 ? status.changes[0].path : `${status?.changes.length ?? 0} files`}`

  const commit = () => run('Committing…', async () => ((await api.gitCommit(text())) ? 'Committed.' : 'Nothing to commit.'))
  const sync = () => run('Syncing…', () => api.gitSync(text()))
  const init = () => run('Creating…', async () => (await api.gitInit(), 'Repository created.'))

  /** One letter for a change, and what it means. */
  function kind(code: string): [string, string] {
    if (code === '??' || code.includes('A')) return ['A', 'Added']
    if (code.includes('D')) return ['D', 'Deleted']
    if (code.includes('R')) return ['R', 'Renamed']
    if (code.includes('U')) return ['U', 'Conflict']
    return ['M', 'Modified']
  }
</script>

<div class="git">
  {#if !status}
    <p class="doc-note">{error || 'Reading…'}</p>
  {:else if !status.repo}
    <p class="doc-note">This folder is not a Git repository.</p>
    <div class="git-actions"><button onclick={init} disabled={!!busy}>Create repository</button></div>
    {#if error}<div class="search-error">{error}</div>{/if}
  {:else}
    <div class="git-branch" title={status.upstream ? `Tracking ${status.upstream}` : status.remote ? 'Not pushed yet' : 'No remote'}>
      <span class="git-name">{status.branch}</span>
      {#if status.upstream}
        <span class="git-count">{#if status.ahead}↑{status.ahead}{/if}{#if status.behind} ↓{status.behind}{/if}</span>
      {:else}
        <span class="git-count">{status.remote ? 'not pushed' : 'local only'}</span>
      {/if}
      <button class="git-refresh" onclick={() => void refresh()} title="Read the status again" aria-label="Refresh">↻</button>
    </div>
    <div class="search-box">
      <input bind:value={message} onkeydown={(e) => e.key === 'Enter' && (e.ctrlKey ? void commit() : void sync())} placeholder="Message (Enter syncs, Ctrl+Enter commits)" spellcheck="false" disabled={!!busy} />
    </div>
    <div class="git-actions">
      <button onclick={commit} disabled={!!busy || !status.changes.length} title="Commit every change below">Commit</button>
      <button class="primary" onclick={sync} disabled={!!busy} title={status.remote ? 'Commit, pull the remote’s changes, then push' : 'Commit (there is no remote to sync with)'}>Sync</button>
    </div>
    {#if busy}<div class="search-status">{busy}</div>{/if}
    {#if note}<div class="search-status">{note}</div>{/if}
    {#if error}<div class="search-error git-error">{error}</div>{/if}
    {#if status.changes.length}
      <ul class="search-results">
        {#each status.changes as c (c.path)}
          {@const [letter, what] = kind(c.code)}
          <li class="git-change">
            <button class="search-file" onclick={() => (letter === 'D' ? oncompare(c.path, true) : onopen(c.path))} title="{what}: {c.path}">
              <span class="search-path">{c.path}</span>
              <span class="git-kind {letter}">{letter}</span>
            </button>
            {#if letter !== 'A'}
              <button class="git-diff" onclick={() => oncompare(c.path, letter === 'D')} title="Compare with the last commit">Diff</button>
            {/if}
          </li>
        {/each}
      </ul>
    {:else}
      <p class="doc-note">No changes.</p>
    {/if}
  {/if}
</div>
