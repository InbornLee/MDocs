import { defineStore } from 'pinia'
import { ref } from 'vue'

const KEY = 'mdocs.theme'

/** Stored preference wins; otherwise follow the OS setting. */
function prefersDark(): boolean {
  const stored = localStorage.getItem(KEY)
  if (stored) return stored === 'dark'
  return window.matchMedia?.('(prefers-color-scheme: dark)').matches ?? false
}

/**
 * Light / dark mode. The tokens live in styles/global.css (`:root` and
 * `html.dark`), so switching is just a class on <html>; Naive UI follows via
 * `darkTheme` in App.vue. index.html sets the class before first paint.
 */
export const useThemeStore = defineStore('theme', () => {
  const dark = ref(prefersDark())

  function apply() {
    document.documentElement.classList.toggle('dark', dark.value)
  }

  function setDark(value: boolean) {
    dark.value = value
    localStorage.setItem(KEY, value ? 'dark' : 'light')
    apply()
    // mermaid bakes the palette into the drawn SVG, so redraw what is on screen
    // (loaded lazily: only documents with a diagram need it)
    if (document.querySelector('.mermaid[data-mermaid-source]')) {
      void import('@/utils/markdown').then((m) => m.refreshMermaidTheme())
    }
  }

  function toggle() {
    setDark(!dark.value)
  }

  apply()

  return { dark, setDark, toggle }
})
