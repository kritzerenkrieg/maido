// The hidden Mermaid helper (H3): its own window and web process, never shown.
// It asks Rust for diagram sources and hands back SVG strings; Rust closes it
// 3 s after the last one, and Mermaid's memory goes with its process.
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

interface Job {
  key: string
  source: string
  theme: string
}

let running = false
let again = false

async function work(): Promise<void> {
  if (running) {
    again = true
    return
  }
  running = true
  const { default: mermaid } = await import('mermaid')
  let theme = ''
  do {
    again = false
    const jobs = await invoke<Job[]>('mermaid_jobs')
    for (const job of jobs) {
      if (job.theme !== theme) {
        mermaid.initialize({ startOnLoad: false, securityLevel: 'strict', theme: job.theme as 'default' | 'dark' })
        theme = job.theme
      }
      const id = `m${job.key}`
      try {
        const { svg } = await mermaid.render(id, job.source)
        await invoke('mermaid_done', { key: job.key, svg })
      } catch (e) {
        // A failed render leaves its scratch element behind.
        document.getElementById(`d${id}`)?.remove()
        await invoke('mermaid_done', { key: job.key, error: e instanceof Error ? e.message : String(e) })
      }
    }
    if (jobs.length) again = true
  } while (again)
  running = false
}

await getCurrentWebviewWindow().listen('mermaid-jobs', () => void work())
void work()
