import { invoke } from "@tauri-apps/api/core"
import { open, save } from "@tauri-apps/plugin-dialog"

export type Summary = {
  id: string | null
  name: string
  columns: string[]
  rows: number
  history: string[]
  /** per step: whether it goes into an extracted recipe (single-cell edits do not, as in OpenRefine) */
  recipe: boolean[]
  cursor: number
}

export type ListFacet = {
  type: "list"
  name: string
  columnName: string
  expression: string
  selection: { v: { v: unknown; l: string } }[]
  selectBlank: boolean
  selectError: boolean
  invert: boolean
}
export type RangeFacet = {
  type: "range"
  name: string
  columnName: string
  expression: string
  from: number
  to: number
  selectNumeric: boolean
  selectNonNumeric: boolean
  selectBlank: boolean
  selectError: boolean
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
export type Facet = ListFacet | TextFacet | RangeFacet
export type Engine = { facets: Facet[]; mode: "row-based" | "record-based" }
/** Choices as [text, count, typed value], plus blank and error counts. */
export type ListData = { distinct: number; choices: [string, number, unknown][]; blank: number; error: number }
export type RangeData = {
  min?: number
  max?: number
  step?: number
  bins?: number[]
  baseBins?: number[]
  numericCount: number
  nonNumericCount: number
  blankCount: number
  errorCount: number
  error?: string
}
export type FacetData = ListData | RangeData | null

export const listFacet = (column: string, expression = "value"): ListFacet => ({
  type: "list", name: column, columnName: column, expression, selection: [], selectBlank: false, selectError: false, invert: false,
})
/** A numeric facet that starts by letting every number through; the panel narrows it. */
export const rangeFacet = (column: string, expression = "value"): RangeFacet => ({
  type: "range", name: column, columnName: column, expression, from: -Number.MAX_VALUE, to: Number.MAX_VALUE,
  selectNumeric: true, selectNonNumeric: true, selectBlank: true, selectError: true,
})
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
/** `"open"` picks a data file to import; anything else is the extension of a file to save. */
export async function pickFile(kind: string): Promise<string | null | undefined> {
  if (!isTauri) return undefined
  if (kind === "open") {
    const p = await open({
      filters: [
        { name: "Data files", extensions: ["csv", "tsv", "tab", "txt", "xlsx", "xlsm", "xls", "ods", "json", "jsonl", "ndjson"] },
        { name: "All files", extensions: ["*"] },
      ],
    })
    return typeof p === "string" ? p : null
  }
  return save({ filters: [{ name: kind.toUpperCase(), extensions: [kind] }] })
}
