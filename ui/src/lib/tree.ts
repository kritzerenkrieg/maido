// Folder refreshes from the watcher reach the tree's folders as `refresh`
// events whose detail is the folder's workspace-relative path ('' is the root).
export const treeEvents = new EventTarget()

export function refreshFolder(path: string): void {
  treeEvents.dispatchEvent(new CustomEvent('refresh', { detail: path }))
}

/** Open the tree's folder `path` (and show what was just made in it). */
export function expandFolder(path: string): void {
  treeEvents.dispatchEvent(new CustomEvent('expand', { detail: path }))
}

export function parentOf(path: string): string {
  const i = path.lastIndexOf('/')
  return i < 0 ? '' : path.slice(0, i)
}
