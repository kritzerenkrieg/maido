<script lang="ts">
  // One folder of the workspace tree, listed when first expanded and again
  // when the watcher reports a change inside it (E1, E2). Right-click opens
  // the Explorer's file menu (N4).
  import { onDestroy } from 'svelte'
  import { api, isCode, type Entry } from './lib/api'
  import { treeEvents } from './lib/tree'
  import Tree from './Tree.svelte'

  interface Props {
    path: string
    all: boolean
    active: string | null
    /** The entry whose menu is open. */
    marked: string | null
    onopen: (path: string) => void
    oncontext: (e: MouseEvent, entry: Entry) => void
    depth?: number
  }
  let { path, all, active, marked, onopen, oncontext, depth = 0 }: Props = $props()

  let entries = $state<Entry[]>([])
  let error = $state('')
  let expanded = $state<Record<string, boolean>>({})

  async function list() {
    try {
      entries = await api.listDir(path, all)
      error = ''
    } catch (e) {
      error = String(e)
    }
  }

  $effect(() => {
    void all
    void list()
  })

  const refresh = (e: Event) => {
    if ((e as CustomEvent<string>).detail === path) void list()
  }
  const expand = (e: Event) => {
    const dir = (e as CustomEvent<string>).detail
    if (entries.some((x) => x.dir && x.path === dir)) expanded[dir] = true
  }
  treeEvents.addEventListener('refresh', refresh)
  treeEvents.addEventListener('expand', expand)
  onDestroy(() => {
    treeEvents.removeEventListener('refresh', refresh)
    treeEvents.removeEventListener('expand', expand)
  })

  /** The icon of a file: documents, pictures, PDFs and source files have their own, everything else a plain file. */
  function icon(name: string): Kind {
    const ext = name.slice(name.lastIndexOf('.') + 1).toLowerCase()
    if (['png', 'jpg', 'jpeg', 'webp', 'gif', 'bmp', 'avif', 'ico', 'svg'].includes(ext)) return 'img'
    if (ext === 'pdf') return 'pdf'
    if (ext === 'md' || ext === 'markdown') return 'md'
    if (ext === 'mmd' || ext === 'mermaid') return 'mmd'
    if (ext === 'tex' || ext === 'latex') return 'tex'
    if (isCode(name)) return 'code'
    return 'file'
  }

  type Kind = 'md' | 'mmd' | 'tex' | 'img' | 'pdf' | 'code' | 'file'

  function context(ev: MouseEvent, e: Entry) {
    ev.preventDefault()
    ev.stopPropagation()
    oncontext(ev, e)
  }
</script>

{#if error}
  <div class="tree-error" style:padding-left="{depth + 1}em">{error}</div>
{/if}
<ul class="tree">
  {#each entries as e (e.path)}
    <li>
      {#if e.dir}
        <button class="tree-row dir" data-path={e.path} class:marked={marked === e.path} style:padding-left="{depth + 0.5}em" onclick={() => (expanded[e.path] = !expanded[e.path])} oncontextmenu={(ev) => context(ev, e)}>
          <span class="twisty">{expanded[e.path] ? '▾' : '▸'}</span>{e.name}
        </button>
        {#if expanded[e.path]}
          <Tree path={e.path} {all} {active} {marked} {onopen} {oncontext} depth={depth + 1} />
        {/if}
      {:else}
        <button class="tree-row file" data-path={e.path} class:active={active === e.path} class:marked={marked === e.path} style:padding-left="{depth + 0.5}em" onclick={() => onopen(e.path)} oncontextmenu={(ev) => context(ev, e)} title={e.path}>
          {@render fileIcon(icon(e.name))}{e.name}
        </button>
      {/if}
    </li>
  {/each}
</ul>

{#snippet fileIcon(kind: Kind)}
  <svg class="ficon {kind}" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if kind === 'md'}
      <rect x="1" y="3" width="14" height="10" rx="2" />
      <path d="M3.6 10.4V5.6l2 2.4 2-2.4v4.8M11.2 5.6v4.6M9.7 8.8l1.5 1.6 1.5-1.6" />
    {:else if kind === 'mmd'}
      <rect x="5.2" y="1.8" width="5.6" height="3.4" rx="1" />
      <rect x="1" y="10.8" width="5" height="3.4" rx="1" />
      <rect x="10" y="10.8" width="5" height="3.4" rx="1" />
      <path d="M8 5.2v2.6M3.5 10.8V7.8h9v3" />
    {:else if kind === 'tex'}
      <rect x="1" y="3" width="14" height="10" rx="2" />
      <path d="M4 6h4M6 6v5M10 6l3 5M13 6l-3 5" />
    {:else if kind === 'img'}
      <rect x="1.5" y="2.5" width="13" height="11" rx="2" />
      <circle cx="5.4" cy="6.2" r="1.2" />
      <path d="M2 12l3.8-3.6 2.7 2.6 2.2-2 3.8 3.4" />
    {:else if kind === 'pdf'}
      <path d="M3.5 1.8h5.6l3.4 3.4v9H3.5z" />
      <path d="M9 1.8v3.6h3.5" />
      <rect x="5.2" y="9" width="5.6" height="2.8" rx="0.6" fill="currentColor" stroke="none" />
    {:else if kind === 'code'}
      <path d="M5.5 4.5L2 8l3.5 3.5M10.5 4.5L14 8l-3.5 3.5M9 3.5l-2 9" />
    {:else}
      <path d="M3.5 1.8h5.6l3.4 3.4v9H3.5z" />
      <path d="M9 1.8v3.6h3.5M5.6 8.6h4.8M5.6 11h4.8" />
    {/if}
  </svg>
{/snippet}
