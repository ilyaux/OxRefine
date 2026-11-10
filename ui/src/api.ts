import { invoke } from "@tauri-apps/api/core"
import { open, save } from "@tauri-apps/plugin-dialog"

export type Summary = {
  name: string
  columns: string[]
  rows: number
  history: string[]
  cursor: number
}

export type ListFacet = {
  type: "list"
  name: string
  columnName: string
  expression: "value"
  selection: { v: { v: string; l: string } }[]
  selectBlank: boolean
  invert: boolean
}
export type TextFacet = {
  type: "text"
  name: string
  columnName: string
  query: string
  mode: "text" | "regex"
  caseSensitive: boolean
  invert: boolean
}
export type Facet = ListFacet | TextFacet
export type Engine = { facets: Facet[]; mode: "row-based" | "record-based" }
export type FacetData = { distinct: number; choices: [string, number][] } | null
export type Op = { op: string } & Record<string, unknown>

export const isTauri = "__TAURI_INTERNALS__" in window

/** One entry point for both the desktop app (Tauri IPC) and `ox serve` (HTTP). */
export async function call<T = unknown>(cmd: string, args: object = {}): Promise<{ data: T; ms: number }> {
  if (isTauri) {
    try {
      return await invoke("ox", { cmd, args })
    } catch (e) {
      throw new Error(String(e))
    }
  }
  const r = await fetch(`/api/${cmd}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(args),
  })
  const j = await r.json()
  if (!r.ok) throw new Error(String(j))
  return j
}

/** Native file dialogs in the desktop app; null in the browser (caller asks for a path). */
export async function pickFile(kind: "open-csv" | "save-csv" | "save-json"): Promise<string | null | undefined> {
  if (!isTauri) return undefined
  if (kind === "open-csv") {
    const p = await open({ filters: [{ name: "Delimited text", extensions: ["csv", "tsv", "txt"] }] })
    return typeof p === "string" ? p : null
  }
  const ext = kind === "save-csv" ? "csv" : "json"
  return save({ filters: [{ name: ext.toUpperCase(), extensions: [ext] }] })
}
