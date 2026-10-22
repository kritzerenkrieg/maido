// Files open in the editor. The buffer holds the editor's state while its tab
// is open; `base` is the hash of the file as last loaded or saved, which a
// save is checked against so a change on disk is never overwritten silently (E3).
import type { EditorState } from '@codemirror/state'

export interface Buffer {
  path: string
  state: EditorState
  base: string
  /** Source line at the top of the editor when it was last hidden. */
  scroll: number
}
