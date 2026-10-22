<script lang="ts" module>
  export interface Command {
    title: string
    /** The shortcut, as shown. */
    key?: string
    run: () => void
  }
</script>

<script lang="ts">
  // Quick open (Ctrl+O) and the command palette (Ctrl+Shift+P): one box. Text
  // starting with ">" lists commands; anything else fuzzy-matches the
  // workspace's file paths in Rust (E1).
  import { onMount, tick } from 'svelte'
  import { api, type FileMatch } from './lib/api'

  interface Props {
    /** What the box starts with: "" for files, ">" for commands. */
    start: string
    commands: Command[]
    onopen: (path: string) => void
    onclose: () => void
  }
  let { start, commands, onopen, onclose }: Props = $props()

  let input = $state<HTMLInputElement>()
  let list = $state<HTMLElement>()
  let text = $state('')
  let files = $state<FileMatch[]>([])
  let selected = $state(0)
  let timer: ReturnType<typeof setTimeout> | undefined
  let asked = 0

  const commandMode = $derived(text.startsWith('>'))

  /** Commands whose title has the query's letters in order; earlier and tighter matches first. */
  const shown = $derived.by(() => {
    if (!commandMode) return []
    const q = text.slice(1).trim().toLowerCase()
    const scored: [number, Command][] = []
    for (const c of commands) {
      const t = c.title.toLowerCase()
      let at = -1
      let first = -1
      let gaps = 0
      let ok = true
      for (const ch of q) {
        if (ch === ' ') continue
        const next = t.indexOf(ch, at + 1)
        if (next < 0) {
          ok = false
          break
        }
        if (first < 0) first = next
        else gaps += next - at - 1
        at = next
      }
      if (ok) scored.push([t.includes(q) ? -1 : gaps + first, c])
    }
    return scored.sort((a, b) => a[0] - b[0]).map(([, c]) => c)
  })

  const count = $derived(commandMode ? shown.length : files.length)

  $effect(() => {
    const q = text
    selected = 0
    if (q.startsWith('>')) return
    clearTimeout(timer)
    timer = setTimeout(async () => {
      const mine = ++asked
      const got = await api.quickOpen(q).catch(() => [])
      if (mine === asked) files = got
    }, 25)
  })

  function choose(i: number) {
    if (commandMode) {
      const c = shown[i]
      if (!c) return
      onclose()
      c.run()
    } else {
      const f = files[i]
      if (!f) return
      onclose()
      onopen(f.path)
    }
  }

  async function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Escape') onclose()
    else if (e.key === 'Enter') choose(selected)
    else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      if (!count) return
      selected = (selected + (e.key === 'ArrowDown' ? 1 : count - 1)) % count
      await tick()
      list?.querySelector('.on')?.scrollIntoView({ block: 'nearest' })
    } else return
    e.preventDefault()
    e.stopPropagation()
  }

  /** `path` cut into runs of matched and unmatched characters. */
  function marks(f: FileMatch): { text: string; hit: boolean }[] {
    const hits = new Set(f.indices)
    const out: { text: string; hit: boolean }[] = []
    Array.from(f.path).forEach((ch, i) => {
      const hit = hits.has(i)
      const last = out[out.length - 1]
      if (last && last.hit === hit) last.text += ch
      else out.push({ text: ch, hit })
    })
    return out
  }

  onMount(() => {
    text = start
    input?.focus()
  })
</script>

<div class="palette-back" role="presentation" onclick={onclose}></div>
<div class="palette" role="dialog" aria-label={commandMode ? 'Commands' : 'Open file'}>
  <input bind:this={input} bind:value={text} {onkeydown} placeholder="File name, or > for commands" spellcheck="false" />
  <ul bind:this={list}>
    {#if commandMode}
      {#each shown as c, i (c.title)}
        <li>
          <button class:on={i === selected} onclick={() => choose(i)} onpointermove={() => (selected = i)}>
            <span>{c.title}</span>{#if c.key}<kbd>{c.key}</kbd>{/if}
          </button>
        </li>
      {:else}
        <li class="palette-none">No matching command.</li>
      {/each}
    {:else}
      {#each files as f, i (f.path)}
        <li>
          <button class:on={i === selected} onclick={() => choose(i)} onpointermove={() => (selected = i)}>
            <span>{#each marks(f) as m, k (k)}{#if m.hit}<b>{m.text}</b>{:else}{m.text}{/if}{/each}</span>
          </button>
        </li>
      {:else}
        <li class="palette-none">{text.trim() ? 'No matching file.' : 'No documents.'}</li>
      {/each}
    {/if}
  </ul>
</div>
