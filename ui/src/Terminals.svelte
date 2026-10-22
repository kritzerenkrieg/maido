<script lang="ts">
  // The terminal panel (D1): sessions as tabs, each a real shell in the
  // workspace folder. Hidden sessions keep running; closing the window ends
  // them. Loaded with the first terminal.
  import { onDestroy } from 'svelte'
  import { Session, type LinkTarget } from './lib/terminal'
  import { onThemeChange } from './lib/theme'

  interface Props {
    scrollback: number
    gpu: boolean
    position: 'right' | 'bottom'
    onpath: (target: LinkTarget) => void
    onurl: (url: string) => void
    onposition: () => void
    onhide: () => void
    onempty: () => void
  }
  let { scrollback, gpu, position, onpath, onurl, onposition, onhide, onempty }: Props = $props()

  interface Tab {
    n: number
    title: string
    session: Session
    exited: boolean
  }
  let tabs = $state<Tab[]>([])
  let active = $state(0)
  let counter = 0
  let body = $state<HTMLElement>()

  export async function create(): Promise<void> {
    const n = ++counter
    const session = new Session({
      scrollback,
      gpu,
      onpath,
      onurl,
      ontitle: (title) => {
        const t = tabs.find((t) => t.n === n)
        if (t) t.title = title
      },
      onexit: () => {
        const t = tabs.find((t) => t.n === n)
        if (t) t.exited = true
      },
    })
    tabs.push({ n, title: 'shell', session, exited: false })
    active = n
    // The tab's element exists after this tick.
    await Promise.resolve()
    await new Promise(requestAnimationFrame)
    const el = body?.querySelector<HTMLElement>(`[data-term="${n}"]`)
    if (el) {
      await session.start(el)
      session.focus()
    }
  }

  function close(n: number) {
    const i = tabs.findIndex((t) => t.n === n)
    if (i < 0) return
    tabs[i].session.dispose()
    tabs.splice(i, 1)
    if (active === n) active = tabs[Math.min(i, tabs.length - 1)]?.n ?? 0
    if (!tabs.length) onempty()
  }

  /** Paste into the active session without Enter (D3). */
  export function paste(text: string): 'ok' | 'none' | 'multiline' {
    const t = tabs.find((t) => t.n === active)
    if (!t || t.exited) return 'none'
    return t.session.paste(text) ? 'ok' : 'multiline'
  }

  export function count(): number {
    return tabs.length
  }

  export function focus(): void {
    tabs.find((t) => t.n === active)?.session.focus()
  }

  $effect(() => {
    const lines = scrollback
    for (const t of tabs) t.session.setScrollback(lines)
  })

  // Refit on any size change: the splitter, the window, switching tabs.
  $effect(() => {
    if (!body) return
    const ro = new ResizeObserver(() => tabs.find((t) => t.n === active)?.session.resize())
    ro.observe(body)
    return () => ro.disconnect()
  })
  $effect(() => {
    const t = tabs.find((t) => t.n === active)
    if (t) requestAnimationFrame(() => t.session.resize())
  })

  const unTheme = onThemeChange(() => tabs.forEach((t) => t.session.retheme()))
  onDestroy(() => {
    unTheme()
    tabs.forEach((t) => t.session.dispose())
  })
</script>

<section class="terms">
  <header class="terms-head">
    {#each tabs as t (t.n)}
      <div class="tab" class:on={t.n === active} class:gone={t.exited}>
        <button class="tab-name" onclick={() => ((active = t.n), requestAnimationFrame(focus))} title={t.title}>{t.n}: {t.title}</button>
        <button class="tab-close" onclick={() => close(t.n)} title="Close terminal">×</button>
      </div>
    {/each}
    <button class="terms-btn" onclick={() => void create()} title="New terminal (Ctrl+Shift+`)">+</button>
    <span class="terms-gap"></span>
    <button class="terms-btn" onclick={() => (onposition(), requestAnimationFrame(focus))} title="Move panel">{position === 'right' ? '⬓' : '◨'}</button>
    <button class="terms-btn" onclick={onhide} title="Hide (Ctrl+`)">–</button>
  </header>
  <div class="terms-body" bind:this={body}>
    {#each tabs as t (t.n)}
      <div class="term" data-term={t.n} class:hidden={t.n !== active}></div>
    {/each}
  </div>
</section>
