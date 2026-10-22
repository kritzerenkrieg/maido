// Block HTML from Rust (already sanitized) made ready for the page: images in
// the workspace go through the asset protocol, remote ones wait for consent
// (F4), blocked ones stay empty.
import { convertFileSrc } from '@tauri-apps/api/core'

export function fragment(html: string, remote: boolean): DocumentFragment {
  const t = document.createElement('template')
  t.innerHTML = html
  for (const img of t.content.querySelectorAll('img')) {
    const src = img.getAttribute('src') ?? ''
    if (src.startsWith('maido-local:')) {
      // The path stays on the element for HTML export, which embeds the file.
      img.dataset.local = decodeURIComponent(src.slice('maido-local:'.length))
      img.src = convertFileSrc(img.dataset.local)
    }
    else if (src.startsWith('maido-remote:')) {
      img.removeAttribute('src')
      img.dataset.remote = src.slice('maido-remote:'.length)
      if (remote) img.src = img.dataset.remote
    } else if (src.startsWith('maido-blocked:')) {
      img.removeAttribute('src')
      img.classList.add('blocked')
    }
  }
  return t.content
}
