import { Component, StrictMode, type ReactNode } from "react"
import { createRoot } from "react-dom/client"
import { openUrl } from "@tauri-apps/plugin-opener"

import "./index.css"
import App from "./App.tsx"
import { ThemeProvider } from "@/components/theme-provider.tsx"
import { isTauri } from "@/api"

// The desktop window can't open new tabs, so web links (Docs, entity pages) go to the system browser.
if (isTauri)
  document.addEventListener("click", (e) => {
    const a = (e.target as Element | null)?.closest?.<HTMLAnchorElement>("a[href^='http']")
    if (!a || e.defaultPrevented) return
    e.preventDefault()
    openUrl(a.href).catch(console.error)
  })

/** A screen error shows what broke and a way back, never a blank window. */
class Crash extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null }
  static getDerivedStateFromError(error: Error) {
    return { error }
  }
  render() {
    if (!this.state.error) return this.props.children
    return (
      <main className="flex h-svh flex-col items-start justify-center gap-3 p-10">
        <h1 className="text-base font-semibold">Something on this screen broke</h1>
        <p className="max-w-2xl font-mono text-[12px] break-all text-muted-foreground">{String(this.state.error.message || this.state.error)}</p>
        <button type="button" onClick={() => location.reload()} className="border border-foreground px-4 py-1.5 hover:bg-accent">
          Reload OxRefine
        </button>
      </main>
    )
  }
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <Crash>
        <App />
      </Crash>
    </ThemeProvider>
  </StrictMode>
)
