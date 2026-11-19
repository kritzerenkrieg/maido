<script lang="ts">
  // A `.tex` file in PDF mode (C4): compiled by the system's TeX through Rust,
  // and compiled again on every change: the file saved (or a file it may
  // include changed), or, while editing, the unsaved text once typing pauses.
  // The last PDF that compiled stays on screen while the next one compiles,
  // and under TeX's errors when it fails.
  import { onDestroy, untrack } from 'svelte'
  import { api } from './lib/api'
  import PdfPage from './PdfPage.svelte'

  interface Props {
    path: string
    /** Bumped when the file, or something beside it that it may include, changes on disk. */
    version: number
    /** The editor's unsaved text, or null for the file as saved. */
    text: string | null
  }
  let { path, version, text }: Props = $props()

  /** Typing pauses this long before the unsaved text is compiled. */
  const TYPING_PAUSE = 700

  let bytes: ArrayBuffer | null = null
  /** Counts PDFs received: PdfPage loads each one. */
  let compiled = $state(0)
  let compiling = $state(false)
  let error = $state('')
  let showLog = $state(true)
  let pdfPage = $state<ReturnType<typeof PdfPage>>()

  let timer: ReturnType<typeof setTimeout> | undefined
  let running = false
  let again = false
  let gone = false
  let seen: { version: number; text: string | null } | null = null

  $effect(() => {
    const now = { version, text }
    untrack(() => {
      const before = seen
      seen = now
      // Saving clears the unsaved text: the file's change event compiles then.
      if (before && before.version === now.version && now.text === null) return
      const typing = !!before && before.version === now.version
      clearTimeout(timer)
      timer = setTimeout(compile, typing ? TYPING_PAUSE : 0)
    })
  })

  async function compile() {
    if (running) {
      again = true
      return
    }
    running = true
    compiling = true
    try {
      const pdf = await api.compileLatex(path, text)
      if (gone) return
      bytes = pdf
      error = ''
      compiled++
    } catch (e) {
      if (gone) return
      error = String(e)
      showLog = true
    } finally {
      running = false
      compiling = again
      if (again && !gone) {
        again = false
        void compile()
      }
    }
  }

  /** pdf.js takes the buffer it is given (it moves to its worker): it gets a copy. */
  const load = () => (bytes ? Promise.resolve(bytes.slice(0)) : Promise.reject(new Error('Nothing compiled yet')))

  /** Rejects until a PDF has compiled. */
  export function preparePrint(): Promise<void> {
    return pdfPage ? pdfPage.preparePrint() : Promise.reject(new Error(compiling ? 'it is still compiling' : 'nothing has compiled yet'))
  }

  onDestroy(() => {
    gone = true
    clearTimeout(timer)
  })
</script>

<div class="tex-pdf">
  {#if error}
    <div class="tex-error">
      <div class="tex-error-head">
        <span>{compiled ? 'TeX stopped: showing the last PDF that compiled.' : 'TeX stopped.'}</span>
        <button onclick={() => (showLog = !showLog)}>{showLog ? 'Hide' : 'Show'} errors</button>
        <button onclick={() => void compile()} disabled={compiling}>Compile again</button>
      </div>
      {#if showLog}<pre>{error}</pre>{/if}
    </div>
  {/if}
  <div class="tex-pdf-body">
    {#if compiled}
      <PdfPage bind:this={pdfPage} {path} version={compiled} {load} />
    {:else if compiling}
      <p class="doc-note">Compiling {path.slice(path.lastIndexOf('/') + 1)}…</p>
    {/if}
    {#if compiling && compiled}
      <span class="tex-busy">Compiling…</span>
    {/if}
  </div>
</div>
