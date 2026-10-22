import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [svelte()],
  // Tauri prints Rust errors to the same terminal; keep them visible.
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    rollupOptions: {
      input: { main: 'index.html', 'mermaid-frame': 'mermaid-frame.html' },
    },
  },
})
