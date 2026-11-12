import { useCallback, useEffect, useMemo, useState } from "react"
import { Redo2Icon, SettingsIcon, Undo2Icon } from "lucide-react"
import { call, isProjectArchive, listFacet, pickFile, saveToBrowser, upload, type ImportReport, rangeFacet, timeFacet, type Engine, type Facet, type FacetData, type ListFacet, type Op, type Summary } from "@/api"
import { AskDialog, ClusterDialog, HistoryView, SortDialog, TransposeDialog, TransformDialog } from "@/Dialogs"
import { Grid, RECON_FACETS, type ColumnAction, type SelectedCell } from "@/Grid"
import { ExtendDialog, loadServices, ReconcileDialog } from "@/ReconcileDialog"
import { Home } from "@/Home"
import { Preferences } from "@/Preferences"
import { ExportDialog, type ExportKind } from "@/ExportDialog"
import { ImportDialog } from "@/ImportDialog"
import { Jobs } from "@/Jobs"
import { Facets, History } from "@/Sidebar"
import { Bar, BarCell, BrandCell, CellButton, CellLink, Crumbs, ProjectName, Spacer, StateDot, StatusBar, StatusCell } from "@/components/instrument"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Toaster, toast } from "@/components/ui/toast"
import { checkForUpdates } from "@/updates"
import { cn } from "@/lib/utils"
import pkg from "../package.json"
import { Palette } from "@/Palette"

type Modal =
  | { kind: "transform" | "add" | "fetch" | "cluster" | "sort" | "facet-list" | "facet-range"; column: string }
  | { kind: "transpose"; column: string; mode: "columns" | "rows" | "columnize" }
  | { kind: "reconcile"; column: string }
  | { kind: "extend"; column: string }
  | { kind: "export"; format: ExportKind }
  | { kind: "extract" }
  | { kind: "ask"; title: string; label: string; initial?: string; multiline?: boolean; fileAccept?: string; onUpload?: (f: File) => Promise<void>; onSubmit: (v: string) => Promise<void> }
  | null

type Staged = { path: string; name?: string; origin?: string }

const VERSION = pkg.version

const num = (n: number) => (Math.abs(n) >= 1e15 ? "∞" : Number(n.toFixed(4)).toLocaleString())
const day = (ms: number) => (Math.abs(ms) >= 8.64e15 ? "…" : new Date(ms).toISOString().slice(0, 10))

/** What a facet filters, as a chip label (null when it lets everything through). */
function chip(f: Facet): string | null {
  if (f.type === "list") {
    const picked = [...f.selection.map((s) => s.v.l || String(s.v.v)), ...(f.selectBlank ? ["(blank)"] : []), ...(f.selectError ? ["(error)"] : [])]
    if (!picked.length) return null
    const shown = picked.length > 3 ? `${picked.slice(0, 3).join(", ")} +${picked.length - 3}` : picked.join(", ")
    return `${f.columnName} ${picked.length > 1 ? (f.invert ? "∉" : "∈") : f.invert ? "≠" : "="} ${shown}`
  }
  if (f.type === "text") return f.query ? `${f.columnName} ${f.invert ? "!~" : "~"} "${f.query}"` : null
  const all = f.from <= -Number.MAX_VALUE && f.to >= Number.MAX_VALUE
  const kinds = f.type === "range" ? f.selectNumeric && f.selectNonNumeric && f.selectBlank && f.selectError : f.selectTime && f.selectNonTime && f.selectBlank && f.selectError
  if (all && kinds) return null
  const range = all ? "" : f.type === "range" ? ` ∈ [${num(f.from)}, ${num(f.to)}]` : ` ∈ [${day(f.from)}, ${day(f.to)}]`
  return `${f.columnName}${range}${kinds ? "" : " (some kinds)"}`
}

/** The facet with its filter cleared: no selection, no query, the full range. */
function cleared(f: Facet): Facet {
  if (f.type === "list") return { ...f, selection: [], selectBlank: false, selectError: false }
  if (f.type === "text") return { ...f, query: "" }
  if (f.type === "range") return { ...rangeFacet(f.columnName, f.expression), name: f.name }
  return { ...timeFacet(f.columnName, f.expression), name: f.name }
}

const fail = (e: unknown) => void toast.add({ title: "Something went wrong", description: String((e as Error).message ?? e), type: "error" })

export function App() {
  const [summary, setSummary] = useState<Summary | null>(null)
  const [facets, setFacets] = useState<Facet[]>([])
  const [mode, setMode] = useState<Engine["mode"]>("row-based")
  const [facetData, setFacetData] = useState<FacetData[]>([])
  const [total, setTotal] = useState(0)
  const [lastMs, setLastMs] = useState<{ what: string; ms: number } | null>(null)
  const [modal, setModal] = useState<Modal>(null)
  // a file to import: picked from disk, or pasted / downloaded and staged by the engine
  const [staged, setStaged] = useState<Staged | null>(null)
  const [palette, setPalette] = useState(false)
  const [side, setSide] = useState<"facets" | "history">("facets")
  // the grid's selected cell, shown in the status bar
  const [selected, setSelected] = useState<SelectedCell | null>(null)
  // the column the command palette works on: the one last used in the grid
  const [focusCol, setFocusCol] = useState<string | null>(null)

  const engine: Engine = useMemo(() => ({ facets, mode }), [facets, mode])
  const version = `${summary?.name}:${summary?.cursor}:${summary?.history.length}:${JSON.stringify(engine)}`

  const run = useCallback(async (what: string, cmd: string, args: object = {}) => {
    const { data, ms } = await call<Summary>(cmd, args)
    setSummary(data)
    setLastMs({ what, ms })
    // facets on columns that no longer exist would make every request fail
    setFacets((fs) => fs.filter((f) => data.columns.includes(f.columnName)))
  }, [])

  useEffect(() => {
    call<Summary>("summary").then(({ data }) => setSummary(data), () => {})
    checkForUpdates()
  }, [])

  const [prefsChanged, setPrefsChanged] = useState(0)
  const [prefsOpen, setPrefsOpen] = useState(false)
  useEffect(() => {
    if (summary) call<FacetData[]>("facets", { engine }).then(({ data }) => setFacetData(data), fail)
  }, [summary, engine, prefsChanged])
  const setFacetLimit = (n: number) =>
    call("set_preference", { name: "ui.browsing.listFacet.limit", value: n }).then(() => setPrefsChanged((k) => k + 1), fail)

  const apply = (op: Op) => run(String(op.op).replace("core/", ""), "apply", { op }).catch((e) => (fail(e), Promise.reject(e)))
  const goto = (index: number) => run("history", "goto", { index }).catch(fail)

  // Desktop: native dialogs. Browser (`ox serve`): open a path on the server or upload a file from this
  // computer; files to save are written on the server and downloaded.
  const withPath = async (kind: string, title: string, then: (p: string) => Promise<void>) => {
    const p = await pickFile(kind)
    if (p) return then(p).catch(fail)
    if (p !== undefined) return
    if (kind !== "open") {
      const ext = kind === "openrefine" ? "openrefine.tar.gz" : kind
      return saveToBrowser(`${(summary?.name ?? "export").replace(/\.[^.]+$/, "")}.${ext}`, then).catch(fail)
    }
    setModal({
      kind: "ask",
      title,
      label: "File or folder path on the server",
      onUpload: (f) => upload(f).then((u) => then(u.path)).catch(fail),
      onSubmit: (v) => then(v.trim()).catch(fail),
    })
  }
  const importProject = async (path: string) => {
    setFacets([])
    const { data, ms } = await call<Summary & { report: ImportReport }>("import_project", { path })
    setSummary(data)
    setLastMs({ what: "import", ms })
    const r = data.report
    const notes = [
      r.merged && `${r.merged > 1 ? `The first ${r.merged} steps were` : "The first step was"} merged into the starting data and can't be undone.`,
      r.dropped && `${r.dropped} undone step${r.dropped > 1 ? "s were" : " was"} left out.`,
      r.reason && `Reason: ${r.reason}.`,
    ].filter(Boolean)
    if (notes.length) toast.add({ title: "Opened with part of its history", description: notes.join(" "), type: "warning", timeout: 0 })
  }
  // an OpenRefine project opens with its history; anything else goes through the import wizard
  const openFile = () =>
    withPath("open", "Import a file or folder", async (path) => {
      const { data } = await call<{ format: string }>("inspect", { path })
      return data.format === "openrefine-project" ? importProject(path) : setStaged({ path })
    })
  const openOpenRefine = () => withPath("open", "Open an OpenRefine project (.openrefine.tar.gz)", importProject)
  const exportProject = () =>
    withPath("openrefine", "Export as an OpenRefine project", async (path) => {
      const file = isProjectArchive(path) ? path : path.replace(/\.gz$/i, "") + ".openrefine.tar.gz"
      const { ms } = await call("export_project", { path: file })
      setLastMs({ what: "export", ms })
      toast.add({ title: "Project exported", description: `${file} opens in OpenRefine with its history.`, type: "success" })
    })
  const pasteData = () =>
    setModal({
      kind: "ask",
      title: "Paste data",
      label: "Rows copied from a spreadsheet, CSV text, or plain lines",
      multiline: true,
      onSubmit: (text) => call<Staged>("paste", { text }).then(({ data }) => setStaged(data), fail),
    })
  const fromUrl = () =>
    setModal({
      kind: "ask",
      title: "Import from a web address",
      label: "URL of a CSV, TSV, Excel, OpenDocument or JSON file",
      onSubmit: (url) => call<Staged>("fetch_url", { url: url.trim() }).then(({ data }) => setStaged(data), fail),
    })
  const openProject = (id: string) => {
    setFacets([])
    run("open", "open_project", { id }).catch(fail)
  }
  const goHome = () => call("close").then(() => setSummary(null), fail)
  const renameProject = (p: { id: string; name: string }, done: () => void) =>
    setModal({
      kind: "ask",
      title: `Rename “${p.name}”`,
      label: "Project name",
      initial: p.name,
      onSubmit: async (v) => {
        await call("rename_project", { id: p.id, name: v.trim() }).catch(fail)
        done()
      },
    })
  const exportRows = (format: string, ext: string, params: object = {}) =>
    withPath(ext, `Export matching rows as .${ext}`, async (path) => {
      const { data, ms } = await call<number>("export", { format, params, engine, path })
      setLastMs({ what: "export", ms })
      toast.add({ title: `Exported ${data.toLocaleString()} rows`, description: path, type: "success" })
    })
  const saveRecipe = (steps: number[] = []) =>
    withPath("json", "Save recipe JSON", async (path) => {
      await call("export_recipe", { path, steps })
      toast.add({ title: "Recipe saved", description: path, type: "success" })
    })
  const copyRecipe = async (steps: number[] = []) => {
    const { data } = await call("export_recipe", { steps })
    await navigator.clipboard.writeText(JSON.stringify(data, null, 2))
    toast.add({ title: "Recipe copied", description: "Paste it into OpenRefine's Undo/Redo → Apply, or `ox apply`.", type: "success" })
  }
  const applyRecipe = () =>
    setModal({
      kind: "ask",
      title: "Apply operation history",
      label: "OpenRefine or OxRefine recipe JSON",
      multiline: true,
      fileAccept: ".json,application/json",
      onSubmit: async (v) => {
        let ops: unknown
        try {
          ops = JSON.parse(v)
        } catch {
          return fail(new Error("That is not valid JSON."))
        }
        await run("recipe", "apply_recipe", { ops }).catch(fail)
      },
    })

  // editing a choice renames that value everywhere it appears in the column (a mass edit)
  const editChoice = (f: ListFacet, choice: string) =>
    setModal({
      kind: "ask",
      title: `Edit “${choice}” in ${f.columnName}`,
      label: "New value",
      initial: choice,
      onSubmit: async (to) => {
        if (to === choice) return
        await apply({ op: "core/mass-edit", engineConfig: engine, columnName: f.columnName, expression: "value", edits: [{ from: [choice], fromBlank: false, fromError: false, to }] })
        setFacets((fs) => fs.map((x) => (x === f ? { ...f, selection: f.selection.map((s) => ((s.v.l || String(s.v.v)) === choice ? { v: { v: to, l: to } } : s)) } : x)))
      },
    })

  // Reconcile → Actions, with OpenRefine's operations and defaults
  const reconAction = (kind: string, c: string) => {
    const mass = (op: string, extra: object = {}) => apply({ op: `core/${op}`, engineConfig: engine, columnName: c, ...extra }).catch(() => {})
    if (kind === "recon-best") mass("recon-match-best-candidates")
    else if (kind === "recon-new-each" || kind === "recon-new-shared") mass("recon-mark-new-topics", { shareNewTopics: kind === "recon-new-shared" })
    else if (kind === "recon-discard") mass("recon-discard-judgments", { clearData: false })
    else if (kind === "recon-clear") mass("recon-discard-judgments", { clearData: true })
    else if (kind === "recon-match-all")
      setModal({
        kind: "ask",
        title: `Match the filtered cells of “${c}” to one item`,
        label: "The item's id, then its name (Q42 Douglas Adams)",
        onSubmit: async (v) => {
          const [id, ...name] = v.trim().split(/\s+/)
          if (id) await mass("recon-match-specific-topic-to-cells", { match: { id, name: name.join(" ") || id, types: [] } })
        },
      })
    else if (kind === "recon-copy")
      setModal({
        kind: "ask",
        title: `Copy reconciliation data from “${c}”`,
        label: "To columns (separated by commas)",
        initial: summary!.columns.filter((x) => x !== c).join(", "),
        onSubmit: (v) =>
          apply({
            op: "core/recon-copy-across-columns",
            engineConfig: engine,
            fromColumnName: c,
            toColumnNames: v.split(",").map((s) => s.trim()).filter(Boolean),
            judgments: ["matched", "new"],
            applyToJudgedCells: true,
          }).catch(() => {}),
      })
    else if (kind === "recon-use-ids")
      setModal({
        kind: "ask",
        title: `Use the values of “${c}” as identifiers`,
        label: "Reconciliation service",
        initial: loadServices()[0].url,
        onSubmit: async (url) => {
          const { data } = await call<{ identifierSpace: string; schemaSpace: string }>("recon_service", { url: url.trim() })
          await mass("recon-use-values-as-identifiers", { service: url.trim(), identifierSpace: data.identifierSpace, schemaSpace: data.schemaSpace })
        },
      })
    else if (kind === "recon-extend") setModal({ kind: "extend", column: c })
    else if (kind === "recon-add-ids")
      setModal({
        kind: "ask",
        title: `Add a column of the items matched in “${c}”`,
        label: "New column name",
        initial: `${c} id`,
        onSubmit: (name) =>
          apply({ op: "core/column-addition", engineConfig: engine, baseColumnName: c, newColumnName: name.trim(), columnInsertIndex: summary!.columns.indexOf(c) + 1, expression: "grel:cell.recon.match.id", onError: "set-to-blank" }).catch(() => {}),
      })
  }

  const onAction = (a: ColumnAction) => {
    const c = a.column
    setFocusCol(c)
    if (a.kind === "facet") setFacets((f) => [...f, listFacet(c)])
    else if (a.kind === "numeric-facet") setFacets((f) => [...f, rangeFacet(c)])
    else if (a.kind === "timeline-facet") setFacets((f) => [...f, timeFacet(c)])
    else if (a.kind === "preset-facet") setFacets((f) => [...f, a.facet === "list" ? listFacet(c, a.expression) : rangeFacet(c, a.expression)])
    else if (a.kind === "custom-list-facet") setModal({ kind: "facet-list", column: c })
    else if (a.kind === "custom-range-facet") setModal({ kind: "facet-range", column: c })
    else if (a.kind === "text-filter")
      setFacets((f) => [...f, { type: "text", name: c, columnName: c, query: "", mode: "text", caseSensitive: false, invert: false }])
    else if (a.kind === "common")
      apply({ op: "core/text-transform", engineConfig: engine, columnName: c, expression: `grel:${a.expression}`, onError: "keep-original", repeat: false, repeatCount: 10 }).catch(() => {})
    else if (a.kind === "transform" || a.kind === "cluster") setModal({ kind: a.kind, column: c })
    else if (a.kind === "reconcile") setModal({ kind: "reconcile", column: c })
    else if (a.kind.startsWith("recon-")) reconAction(a.kind, c)
    else if (a.kind === "add-column" || a.kind === "fetch") setModal({ kind: a.kind === "fetch" ? "fetch" : "add", column: c })
    else if (a.kind === "transpose-columns" || a.kind === "transpose-rows" || a.kind === "columnize")
      setModal({ kind: "transpose", column: c, mode: a.kind === "columnize" ? "columnize" : a.kind === "transpose-rows" ? "rows" : "columns" })
    else if (a.kind === "remove") apply({ op: "core/column-removal", columnName: c }).catch(() => {})
    else if (a.kind === "fill-down" || a.kind === "blank-down") apply({ op: `core/${a.kind}`, engineConfig: engine, columnName: c }).catch(() => {})
    else if (a.kind === "dedupe") apply({ op: "core/row-duplicate-removal", engineConfig: engine, criteria: [c] }).catch(() => {})
    else if (a.kind === "sort") setModal({ kind: "sort", column: c })
    else if (a.kind.startsWith("move-")) apply({ op: `core/column-${a.kind}`, columnName: c }).catch(() => {})
    else if (a.kind === "mv-split" || a.kind === "mv-join" || a.kind === "split") {
      const what = { "mv-split": "Split multi-valued cells", "mv-join": "Join multi-valued cells", split: "Split into several columns" }[a.kind]
      setModal({
        kind: "ask",
        title: `${what} in “${c}”`,
        label: a.kind === "mv-join" ? "Join with" : "Separator",
        initial: a.kind === "mv-join" ? ", " : ",",
        onSubmit: (sep) =>
          apply(
            a.kind === "split"
              ? { op: "core/column-split", engineConfig: engine, columnName: c, guessCellType: true, removeOriginalColumn: true, mode: "separator", separator: sep, regex: false, maxColumns: 0 }
              : a.kind === "mv-split"
                ? { op: "core/multivalued-cell-split", columnName: c, keyColumnName: summary!.columns[0], mode: "separator", separator: sep, regex: false }
                : { op: "core/multivalued-cell-join", columnName: c, keyColumnName: summary!.columns[0], separator: sep }
          ).catch(() => {}),
      })
    }
    else if (a.kind === "rename")
      setModal({
        kind: "ask",
        title: `Rename “${c}”`,
        label: "New name",
        initial: c,
        onSubmit: async (v) => {
          if (v.trim() === c) return
          await apply({ op: "core/column-rename", oldColumnName: c, newColumnName: v.trim() })
          setFacets((fs) => fs.map((f) => (f.columnName === c ? { ...f, name: v.trim(), columnName: v.trim() } : f)))
        },
      })
  }

  // the column the tool cells and keys work on: the one last used in the grid, else the first
  const toolColumn = summary ? (focusCol && summary.columns.includes(focusCol) ? focusCol : (summary.columns[0] ?? null)) : null
  const tool = (kind: "facet" | "cluster" | "transform" | "reconcile") => toolColumn && onAction({ kind, column: toolColumn })

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault()
        return setPalette((p) => !p)
      }
      const typing = (e.target as HTMLElement).closest("input,textarea,select,[contenteditable],[role=dialog],[role=menu]")
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "o" && !typing) {
        e.preventDefault()
        return openFile()
      }
      // one-key tools, only with nothing else open and the focus outside text fields
      if (!e.ctrlKey && !e.metaKey && !e.altKey && !typing && !modal && !staged && !palette && !prefsOpen) {
        const k = e.key.toLowerCase()
        const tools: Record<string, () => void> = summary
          ? { f: () => tool("facet"), c: () => tool("cluster"), t: () => tool("transform"), r: () => tool("reconcile"), e: () => setModal({ kind: "export", format: "csv" }), i: openFile }
          : { i: openFile }
        if (tools[k]) {
          e.preventDefault()
          return tools[k]()
        }
      }
      if (!summary || !(e.ctrlKey || e.metaKey) || (e.target as HTMLElement).closest("input,textarea,[contenteditable]")) return
      const k = e.key.toLowerCase()
      if (k === "z" && !e.shiftKey && summary.cursor > 0) goto(summary.cursor - 1)
      else if ((k === "y" || (k === "z" && e.shiftKey)) && summary.cursor < summary.history.length) goto(summary.cursor + 1)
      else return
      e.preventDefault()
    }
    window.addEventListener("keydown", onKey)
    return () => window.removeEventListener("keydown", onKey)
  })

  return (
    <ProjectName.Provider value={summary?.name ?? null}>
      <Toaster>
        <div className="flex h-svh flex-col bg-background text-foreground">
          {summary ? (
            <Bar>
              <BrandCell />
              <Crumbs
                parts={[
                  <button key="p" type="button" onClick={goHome} className="outline-none hover:text-foreground hover:underline focus-visible:ring-2 focus-visible:ring-ring">
                    Projects
                  </button>,
                  <>
                    <span className="truncate">{summary.name}</span>
                    <span className="font-mono text-xs font-normal text-muted-foreground">
                      {summary.rows.toLocaleString()} × {summary.columns.length}
                    </span>
                  </>,
                ]}
              />
              <CellButton keys="F" onClick={() => tool("facet")} title={toolColumn ? `Facet by ${toolColumn}` : undefined}>
                Facet
              </CellButton>
              <CellButton keys="C" onClick={() => tool("cluster")} title={toolColumn ? `Cluster ${toolColumn}` : undefined}>
                Cluster
              </CellButton>
              <CellButton keys="T" onClick={() => tool("transform")} title={toolColumn ? `Transform ${toolColumn}` : undefined}>
                Transform
              </CellButton>
              <CellButton keys="R" onClick={() => tool("reconcile")} title={toolColumn ? `Reconcile ${toolColumn}` : undefined}>
                Reconcile
              </CellButton>
              <CellButton onClick={() => setModal({ kind: "extract" })}>Recipe</CellButton>
              <DropdownMenu>
                <DropdownMenuTrigger render={<CellButton />}>Rows</DropdownMenuTrigger>
                <DropdownMenuContent align="start" className="w-60">
                  <DropdownMenuGroup>
                    <DropdownMenuItem onClick={() => apply({ op: "core/row-star", engineConfig: engine, starred: true }).catch(() => {})}>Star matching rows</DropdownMenuItem>
                    <DropdownMenuItem onClick={() => apply({ op: "core/row-star", engineConfig: engine, starred: false }).catch(() => {})}>Unstar matching rows</DropdownMenuItem>
                    <DropdownMenuItem onClick={() => apply({ op: "core/row-flag", engineConfig: engine, flagged: true }).catch(() => {})}>Flag matching rows</DropdownMenuItem>
                    <DropdownMenuItem onClick={() => apply({ op: "core/row-flag", engineConfig: engine, flagged: false }).catch(() => {})}>Unflag matching rows</DropdownMenuItem>
                  </DropdownMenuGroup>
                  <DropdownMenuSeparator />
                  <DropdownMenuGroup>
                    <DropdownMenuItem onClick={() => apply({ op: "core/row-keep-matched", engineConfig: engine }).catch(() => {})}>Keep only matching rows</DropdownMenuItem>
                    <DropdownMenuItem variant="destructive" onClick={() => apply({ op: "core/row-removal", engineConfig: engine }).catch(() => {})}>
                      Remove matching rows
                    </DropdownMenuItem>
                  </DropdownMenuGroup>
                </DropdownMenuContent>
              </DropdownMenu>
              <Spacer />
              <CellButton rule="left" aria-label="Undo" title="Undo (Ctrl Z)" disabled={!summary.cursor} onClick={() => goto(summary.cursor - 1)}>
                <Undo2Icon className="size-4" />
              </CellButton>
              <CellButton rule="left" aria-label="Redo" title="Redo (Ctrl Y)" disabled={summary.cursor >= summary.history.length} onClick={() => goto(summary.cursor + 1)}>
                <Redo2Icon className="size-4" />
              </CellButton>
              <CellButton rule="left" aria-label="Preferences" title="Preferences" onClick={() => setPrefsOpen(true)}>
                <SettingsIcon className="size-4" />
              </CellButton>
              <CellButton rule="left" keys="E" onClick={() => setModal({ kind: "export", format: "csv" })}>
                Export
              </CellButton>
              <DropdownMenu>
                <DropdownMenuTrigger render={<CellButton rule="left" ink keys="I" />}>Import</DropdownMenuTrigger>
                <DropdownMenuContent align="end" className="w-52">
                  <DropdownMenuGroup>
                    <DropdownMenuItem onClick={openFile}>File…</DropdownMenuItem>
                    <DropdownMenuItem onClick={pasteData}>Paste data…</DropdownMenuItem>
                    <DropdownMenuItem onClick={fromUrl}>Web address…</DropdownMenuItem>
                  </DropdownMenuGroup>
                  <DropdownMenuSeparator />
                  <DropdownMenuGroup>
                    <DropdownMenuItem onClick={openOpenRefine}>OpenRefine project…</DropdownMenuItem>
                  </DropdownMenuGroup>
                </DropdownMenuContent>
              </DropdownMenu>
            </Bar>
          ) : (
            <Bar>
              <BrandCell version={VERSION} />
              <BarCell className="font-medium">Projects</BarCell>
              <Spacer />
              <CellLink href="https://ilyaux.github.io/OxRefine/">Docs</CellLink>
              <CellLink href="https://ilyaux.github.io/OxRefine/moving-over.html">Coming from OpenRefine</CellLink>
              <CellButton rule="left" aria-label="Preferences" title="Preferences" onClick={() => setPrefsOpen(true)}>
                <SettingsIcon className="size-4" />
              </CellButton>
            </Bar>
          )}

          {summary ? (
            <>
              <div className="flex h-10 shrink-0 items-stretch border-b text-[12.5px]">
                <div role="tablist" aria-label="Side panel" className="flex w-65 shrink-0 items-stretch border-r">
                  {(["facets", "history"] as const).map((t) => (
                    <button
                      key={t}
                      type="button"
                      role="tab"
                      aria-selected={side === t}
                      onClick={() => setSide(t)}
                      className={cn(
                        "flex flex-1 items-center justify-center gap-1.5 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                        side === t ? "-mb-px border-b-2 border-foreground font-medium" : "text-muted-foreground hover:bg-accent hover:text-foreground",
                      )}
                    >
                      {t === "facets" ? "Facets" : "History"}
                      <span className="font-mono text-[11px] text-muted-foreground">{t === "facets" ? facets.length : summary.cursor}</span>
                    </button>
                  ))}
                </div>
                <div className="flex items-center border-r px-3.5 font-mono whitespace-nowrap">
                  {total.toLocaleString()}
                  <span className="text-muted-foreground">&nbsp;/ {summary.rows.toLocaleString()} {mode === "record-based" ? "records" : "rows"}</span>
                </div>
                <div className="flex min-w-0 flex-1 items-stretch overflow-x-auto">
                  {facets.map((f, i) => {
                    const label = chip(f)
                    return (
                      label && (
                        <div key={i} className="flex shrink-0 items-center gap-3 border-r pl-3.5">
                          <span className="font-mono">{label}</span>
                          <button
                            type="button"
                            aria-label={`Clear the filter ${label}`}
                            onClick={() => setFacets((fs) => fs.map((x, j) => (j === i ? cleared(x) : x)))}
                            className="flex h-full items-center px-3 text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
                          >
                            ×
                          </button>
                        </div>
                      )
                    )
                  })}
                  {facets.some(chip) && (
                    <CellButton className="text-muted-foreground" onClick={() => setFacets((fs) => fs.map(cleared))}>
                      Clear all
                    </CellButton>
                  )}
                </div>
                <div role="group" aria-label="Show rows or records" className="flex items-stretch">
                  <CellButton rule="left" active={mode === "row-based"} onClick={() => setMode("row-based")} className={mode === "row-based" ? "font-medium" : "text-muted-foreground"}>
                    Rows
                  </CellButton>
                  <CellButton rule="left" active={mode === "record-based"} onClick={() => setMode("record-based")} className={mode === "record-based" ? "font-medium" : "text-muted-foreground"}>
                    Records
                  </CellButton>
                </div>
              </div>
              <div className="flex min-h-0 flex-1">
                <aside aria-label={side === "facets" ? "Facets" : "History"} className="w-65 shrink-0 overflow-y-auto border-r">
                  {side === "facets" ? (
                    <Facets
                      facets={facets}
                      data={facetData}
                      columns={summary.columns}
                      onChange={setFacets}
                      onEditChoice={editChoice}
                      onLimit={setFacetLimit}
                      onAdd={(c) => onAction({ kind: "facet", column: c })}
                    />
                  ) : (
                    <History summary={summary} onGoto={goto} onExtract={() => setModal({ kind: "extract" })} />
                  )}
                </aside>
                <main className="min-w-0 flex-1">
                  <Grid columns={summary.columns} engine={engine} version={version} onTotal={setTotal} onAction={onAction} onApply={apply} onFocusColumn={setFocusCol} onSelect={setSelected} dataVersion={`${summary.name}:${summary.cursor}:${summary.history.length}`} />
                </main>
              </div>
            </>
          ) : (
            <Home onImport={openFile} onPaste={pasteData} onUrl={fromUrl} onOpenRefine={openOpenRefine} onOpen={openProject} onRename={renameProject} onError={fail} />
          )}

          <StatusBar>
            <StatusCell className="text-foreground">
              <StateDot />
              ready
            </StatusCell>
            {lastMs && (
              <StatusCell>
                {lastMs.what} {lastMs.ms < 10 ? lastMs.ms.toFixed(1) : Math.round(lastMs.ms).toLocaleString()} ms
              </StatusCell>
            )}
            {summary && (
              <StatusCell>
                {summary.cursor} step{summary.cursor === 1 ? "" : "s"}
                {summary.id && " · saved"}
              </StatusCell>
            )}
            <Spacer />
            {summary && selected && selected.column && summary.columns.includes(selected.column) && (
              <StatusCell rule="left" className="min-w-0 gap-2">
                <span>
                  row {selected.row + 1} · {selected.column}
                </span>
                <span className="max-w-80 truncate border border-input bg-background px-1.5 text-foreground">{selected.value === null ? "null" : `"${selected.value.replace(/ /g, "·")}"`}</span>
                {selected.value !== null && /^\s|\s$/.test(selected.value) && <span className="text-warn-text">{/^\s/.test(selected.value) ? "leading" : "trailing"} space</span>}
                <span>Enter edit</span>
              </StatusCell>
            )}
            {summary ? (
              <StatusCell rule="left">
                <kbd className="text-[11.5px]">Ctrl K</kbd> commands
              </StatusCell>
            ) : (
              <StatusCell rule="left">matches OpenRefine 3.10.1</StatusCell>
            )}
          </StatusBar>
        </div>

        <Jobs />
        <Palette
          open={palette}
          onOpenChange={setPalette}
          column={summary ? (focusCol && summary.columns.includes(focusCol) ? focusCol : (summary.columns[0] ?? null)) : null}
          columns={summary?.columns ?? []}
          onColumn={setFocusCol}
          onAction={onAction}
          commands={[
            ...(summary
              ? [
                  { label: "Undo", shortcut: "Ctrl+Z", run: () => summary.cursor > 0 && goto(summary.cursor - 1) },
                  { label: "Redo", shortcut: "Ctrl+Y", run: () => summary.cursor < summary.history.length && goto(summary.cursor + 1) },
                  { label: mode === "row-based" ? "Show records" : "Show rows", run: () => setMode(mode === "row-based" ? "record-based" : "row-based") },
                  { label: "Remove all facets", run: () => setFacets([]) },
                  { label: "Apply recipe…", run: applyRecipe },
                  { label: "Extract recipe…", run: () => setModal({ kind: "extract" }) },
                  { label: "Copy recipe JSON", run: () => void copyRecipe().catch(fail) },
                  { label: "Export matching rows as CSV…", run: () => exportRows("csv", "csv") },
                  { label: "Export matching rows as Excel…", run: () => exportRows("xlsx", "xlsx") },
                  { label: "Export matching rows as OpenDocument…", run: () => exportRows("ods", "ods") },
                  { label: "Export as an OpenRefine project…", run: exportProject },
                  { label: "Back to projects", run: goHome },
                ]
              : []),
            { label: "Preferences…", run: () => setPrefsOpen(true) },
            { label: "Import a file or folder…", run: openFile },
            { label: "Import pasted data…", run: pasteData },
            { label: "Import from a web address…", run: fromUrl },
            { label: "Open an OpenRefine project…", run: openOpenRefine },
          ]}
        />
        {modal?.kind === "ask" && <AskDialog {...modal} onClose={() => setModal(null)} />}
        {prefsOpen && <Preferences onClose={() => setPrefsOpen(false)} onError={fail} onChanged={() => setPrefsChanged((k) => k + 1)} />}
        {staged && (
          <ImportDialog
            {...staged}
            onClose={() => setStaged(null)}
            onCreate={async (args) => {
              await run("import", "open", { ...args, origin: staged.origin })
              setFacets([])
              setStaged(null)
            }}
          />
        )}
        {modal?.kind === "reconcile" && summary && (
          <ReconcileDialog
            column={modal.column}
            columns={summary.columns}
            engine={engine}
            onApply={async (op) => {
              await apply(op)
              // as OpenRefine does when reconciling finishes: facets by judgment and by best score
              const c = modal.column
              setFacets((f) => [...f, { ...listFacet(c, RECON_FACETS[0][2]), name: `${c}: judgment` }, { ...rangeFacet(c, RECON_FACETS[1][2]), name: `${c}: best candidate's score` }])
            }}
            onClose={() => setModal(null)}
          />
        )}
        {modal?.kind === "extend" && summary && (
          <ExtendDialog column={modal.column} columns={summary.columns} engine={engine} onApply={apply} onClose={() => setModal(null)} />
        )}
        {modal?.kind === "transpose" && summary && (
          <TransposeDialog column={modal.column} columns={summary.columns} mode={modal.mode} onApply={apply} onClose={() => setModal(null)} />
        )}
        {(modal?.kind === "transform" || modal?.kind === "add" || modal?.kind === "fetch" || modal?.kind === "facet-list" || modal?.kind === "facet-range") && summary && (
          <TransformDialog
            column={modal.column}
            mode={modal.kind}
            columns={summary.columns}
            engine={engine}
            rows={total}
            onApply={apply}
            onFacet={(expression) => setFacets((f) => [...f, modal.kind === "facet-range" ? rangeFacet(modal.column, expression) : listFacet(modal.column, expression)])}
            onClose={() => setModal(null)}
          />
        )}
        {modal?.kind === "export" && summary && (
          <ExportDialog
            format={modal.format}
            name={summary.name}
            columns={summary.columns}
            engine={engine}
            rows={total}
            allRows={summary.rows}
            onExport={(format, ext, params) => {
              setModal(null)
              if (format === "project") exportProject()
              else if (format === "recipe") saveRecipe()
              else exportRows(format, ext, params)
            }}
            onClose={() => setModal(null)}
          />
        )}
        {modal?.kind === "extract" && summary && (
          <HistoryView
            summary={summary}
            onGoto={goto}
            onCopy={(steps) => copyRecipe(steps).catch(fail)}
            onSave={(steps) => saveRecipe(steps)}
            onApplyRecipe={applyRecipe}
            onClose={() => setModal(null)}
          />
        )}
        {modal?.kind === "sort" && <SortDialog column={modal.column} onApply={apply} onClose={() => setModal(null)} />}
        {modal?.kind === "cluster" && (
          <ClusterDialog column={modal.column} engine={engine} onApply={apply} onClose={() => setModal(null)} />
        )}
      </Toaster>
    </ProjectName.Provider>
  )
}

export default App
