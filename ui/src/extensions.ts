import { useEffect, useState } from "react"
import { call } from "@/api"

/** What an installed extension registered (crates/ox-core/src/extensions.rs). */
export type ExtParam = { name: string; label?: string; default?: string | number | boolean }
export type ExtItem = {
  kind: "function" | "importer" | "exporter" | "operation"
  id: string
  owner: string
  label?: string
  description?: string
  params?: string | ExtParam[]
  extension?: string
  extensions?: string[]
  mime?: string
}
export type Extension = { name: string; version: string; description: string; path: string; items: ExtItem[]; error: string | null }
export type Extensions = { folders: string[]; extensions: Extension[] }

let latest: Extensions | null = null
let pending: Promise<Extensions> | null = null
const listeners = new Set<(e: Extensions) => void>()

/** Installed extensions, fetched once per page; `reload` asks the engine to load the folders again.
 * A failed fetch is forgotten, so the next caller tries again. */
export function loadExtensions(reload = false): Promise<Extensions> {
  if (latest && !reload) return Promise.resolve(latest)
  if (!pending || reload) {
    pending = call<Extensions>(reload ? "reload_extensions" : "extensions", {}, { quiet: true }).then(({ data }) => {
      latest = data
      listeners.forEach((f) => f(data))
      return data
    })
    pending.catch(() => (pending = null))
  }
  return pending
}

export function useExtensions(): Extensions | null {
  const [state, setState] = useState<Extensions | null>(latest)
  useEffect(() => {
    listeners.add(setState)
    loadExtensions().then(setState, () => {})
    return () => void listeners.delete(setState)
  }, [])
  return state
}

/** Calls `f` whenever the installed extensions are fetched or reloaded (without fetching itself). */
export function listenExtensions(f: (e: Extensions) => void) {
  listeners.add(f)
  if (latest) f(latest)
}

/** Every item of one kind across the extensions that loaded. */
export function items(e: Extensions | null, kind: ExtItem["kind"]): ExtItem[] {
  return (e?.extensions ?? []).filter((x) => !x.error).flatMap((x) => x.items.filter((i) => i.kind === kind))
}
