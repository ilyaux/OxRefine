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
  archiveWarning?: string | null
}

/** What happened to an OpenRefine project's history on import. */
export type ImportReport = { merged: number; dropped: number; reason: string | null; warnings?: string[] }

/** OpenRefine project archives open with their history instead of going through the import wizard. */
export const isProjectArchive = (path: string) => /\.(tar\.gz|tgz)$/i.test(path)

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
/** OpenRefine's timeline facet: dates from `from` to `to`, in milliseconds since 1970. */
export type TimeFacet = {
  type: "timerange"
  name: string
  columnName: string
  expression: string
  from: number
  to: number
  selectTime: boolean
  selectNonTime: boolean
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
export type Facet = ListFacet | TextFacet | RangeFacet | TimeFacet
export type Engine = { facets: Facet[]; mode: "row-based" | "record-based" }
/** Choices as [text, count, typed value], plus blank and error counts. */
/** `tooMany`: more choices than the facet choice limit (`limit`), so none are listed, as in OpenRefine. */
export type ListData = { distinct: number; choices: [string, number, unknown][]; blank: number; error: number; tooMany?: boolean; limit?: number }
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
export type TimeData = {
  min: number
  max: number
  step: number
  bins: number[]
  baseBins: number[]
  timeCount: number
  nonTimeCount: number
  blankCount: number
  errorCount: number
  baseTimeCount: number
}
export type FacetData = ListData | RangeData | TimeData | null

export const listFacet = (column: string, expression = "value"): ListFacet => ({
  type: "list", name: column, columnName: column, expression, selection: [], selectBlank: false, selectError: false, invert: false,
})
/** A numeric facet that starts by letting every number through; the panel narrows it. */
export const rangeFacet = (column: string, expression = "value"): RangeFacet => ({
  type: "range", name: column, columnName: column, expression, from: -Number.MAX_VALUE, to: Number.MAX_VALUE,
  selectNumeric: true, selectNonNumeric: true, selectBlank: true, selectError: true,
})
/** A timeline facet that starts by letting every date through. */
export const timeFacet = (column: string, expression = "value"): TimeFacet => ({
  type: "timerange", name: column, columnName: column, expression, from: -Number.MAX_VALUE, to: Number.MAX_VALUE,
  selectTime: true, selectNonTime: true, selectBlank: true, selectError: true,
})
export type Op = { op: string } & Record<string, unknown>

export const isTauri = "__TAURI_INTERNALS__" in window

// `ox serve --host`: the link carries a token; this tab keeps it and sends it with every request
const token = (() => {
  const q = new URLSearchParams(location.search).get("token")
  try {
    if (q) sessionStorage.setItem("ox-token", q)
    return q ?? sessionStorage.getItem("ox-token")
  } catch {
    return q
  }
})()
const auth: Record<string, string> = token ? { Authorization: `Bearer ${token}` } : {}

/** Browser (`ox serve`): send a file from this computer to the server, staged for the import wizard. */
export async function upload(file: File): Promise<{ path: string; name: string }> {
  const r = await fetch("/upload", { method: "POST", headers: { ...auth, "X-File-Name": encodeURIComponent(file.name) }, body: file })
  const j = await r.json()
  if (!r.ok) throw new Error(String(j))
  return j.data
}

/** Browser: write a file with `write(path)` on the server, then download it to this computer. */
export async function saveToBrowser(name: string, write: (path: string) => Promise<void>) {
  const r = await fetch("/stage", { method: "POST", headers: { ...auth, "Content-Type": "application/json" }, body: JSON.stringify({ name }) })
  const j = await r.json()
  if (!r.ok) throw new Error(String(j))
  await write(j.data.path)
  const a = document.createElement("a")
  a.href = `/download?path=${encodeURIComponent(j.data.path)}${token ? `&token=${encodeURIComponent(token)}` : ""}`
  a.download = name
  a.click()
}

// commands in flight, so the progress panel knows when to look for running jobs
let pending = 0
const pendingListeners = new Set<(n: number) => void>()
/** Follow the number of commands in flight; returns an unsubscribe function. */
export function onPending(f: (n: number) => void) {
  pendingListeners.add(f)
  return () => void pendingListeners.delete(f)
}

/** One entry point for both the desktop app (Tauri IPC) and `ox serve` (HTTP). `quiet` calls (progress
 * polling) don't count as work in flight. */
export async function call<T = unknown>(cmd: string, args: object = {}, { quiet = false } = {}): Promise<{ data: T; ms: number }> {
  if (quiet) return send<T>(cmd, args)
  pendingListeners.forEach((f) => f(++pending))
  try {
    return await send<T>(cmd, args)
  } finally {
    pendingListeners.forEach((f) => f(--pending))
  }
}

async function send<T>(cmd: string, args: object): Promise<{ data: T; ms: number }> {
  if (isTauri) {
    try {
      return await invoke("ox", { cmd, args })
    } catch (e) {
      throw new Error(String(e))
    }
  }
  const r = await fetch(`/api/${cmd}`, {
    method: "POST",
    headers: { ...auth, "Content-Type": "application/json" },
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
        { name: "Data files", extensions: ["csv", "tsv", "tab", "txt", "xlsx", "xlsm", "xls", "ods", "json", "jsonl", "ndjson", "xml", "zip", "gz", "tgz", "tar"] },
        { name: "OpenRefine projects", extensions: ["gz", "tgz"] },
        { name: "All files", extensions: ["*"] },
      ],
    })
    return typeof p === "string" ? p : null
  }
  if (kind === "openrefine") return save({ filters: [{ name: "OpenRefine project", extensions: ["gz"] }], defaultPath: "project.openrefine.tar.gz" })
  return save({ filters: [{ name: kind.toUpperCase(), extensions: [kind] }] })
}
