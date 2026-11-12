import { StrictMode } from "react"
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

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <ThemeProvider>
      <App />
    </ThemeProvider>
  </StrictMode>
)
