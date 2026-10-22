// Themes (L4): System (follows the desktop), Light or Dark. The page's colours
// switch through `data-theme` on <html> (see app.css); diagrams, terminals and
// code colours ask `isDark()` and listen for changes here.
export type ThemeSetting = 'system' | 'light' | 'dark'

const media = matchMedia('(prefers-color-scheme: dark)')
const listeners = new Set<() => void>()
let setting: ThemeSetting = 'system'

export function isDark(): boolean {
  return setting === 'dark' || (setting === 'system' && media.matches)
}

/** Call `f` whenever the effective theme changes; returns the unsubscribe. */
export function onThemeChange(f: () => void): () => void {
  listeners.add(f)
  return () => listeners.delete(f)
}

export function applyTheme(next: ThemeSetting): void {
  const was = isDark()
  setting = next
  if (next === 'system') delete document.documentElement.dataset.theme
  else document.documentElement.dataset.theme = next
  if (isDark() !== was) listeners.forEach((f) => f())
}

media.addEventListener('change', () => {
  if (setting === 'system') listeners.forEach((f) => f())
})
