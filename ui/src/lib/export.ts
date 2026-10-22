// Exports (L1): bytes go to Rust, which asks where to save them in the
// system's dialog and writes them only there.
import { invoke } from '@tauri-apps/api/core'

/** Save `data` as `name`, the dialog starting beside the workspace file `near`. Null if cancelled. */
export function saveExport(name: string, near: string | null, data: string | Uint8Array): Promise<string | null> {
  const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data
  const headers: Record<string, string> = { 'x-name': encodeURIComponent(name) }
  if (near) headers['x-near'] = encodeURIComponent(near)
  return invoke<string | null>('export_file', bytes, { headers })
}
