import { useCallback, useEffect, useMemo, useState } from "react"
import { DownloadIcon, FileUpIcon, FolderOpenIcon, HouseIcon, Redo2Icon, TableIcon, Undo2Icon } from "lucide-react"
import { call, isProjectArchive, listFacet, pickFile, type ImportReport, rangeFacet, type Engine, type Facet, type FacetData, type ListFacet, type Op, type Summary } from "@/api"
import { AskDialog, ClusterDialog, ExtractDialog, SortDialog, TransformDialog } from "@/Dialogs"
import { Grid, type ColumnAction } from "@/Grid"
import { Home } from "@/Home"
import { EXTENSION, ExportDialog, type ExportFormat } from "@/ExportDialog"
import { ImportDialog } from "@/ImportDialog"
import { Jobs } from "@/Jobs"
import { Facets, History } from "@/Sidebar"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Separator } from "@/components/ui/separator"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Toaster, toast } from "@/components/ui/toast"

type Modal =
  | { kind: "transform" | "add" | "cluster" | "sort" | "facet-list" | "facet-range"; column: string }
  | { kind: "export"; format: ExportFormat }
  | { kind: "extract" }
  | { kind: "ask"; title: string; label: string; initial?: string; multiline?: boolean; fileAccept?: string; onSubmit: (v: string) => Promise<void> }
  | null

type Staged = { path: string; name?: string; origin?: string }

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
  }, [])

  useEffect(() => {
    if (summary) call<FacetData[]>("facets", { engine }).then(({ data }) => setFacetData(data), fail)
  }, [summary, engine])

  const apply = (op: Op) => run(String(op.op).replace("core/", ""), "apply", { op }).catch((e) => (fail(e), Promise.reject(e)))
  const goto = (index: number) => run("history", "goto", { index }).catch(fail)

  // Desktop: native dialogs. Browser (`ox serve`): the server reads/writes paths on its own machine.
  const withPath = async (kind: string, title: string, then: (p: string) => Promise<void>) => {
    const p = await pickFile(kind)
    if (p) return then(p).catch(fail)
    if (p === undefined) setModal({ kind: "ask", title, label: "File path on this machine", onSubmit: (v) => then(v.trim()).catch(fail) })
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
      r.recon && "Reconciliation data isn't kept yet; the cell values are.",
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

  const onAction = (a: ColumnAction) => {
    const c = a.column
    if (a.kind === "facet") setFacets((f) => [...f, listFacet(c)])
    else if (a.kind === "numeric-facet") setFacets((f) => [...f, rangeFacet(c)])
    else if (a.kind === "preset-facet") setFacets((f) => [...f, a.facet === "list" ? listFacet(c, a.expression) : rangeFacet(c, a.expression)])
    else if (a.kind === "custom-list-facet") setModal({ kind: "facet-list", column: c })
    else if (a.kind === "custom-range-facet") setModal({ kind: "facet-range", column: c })
    else if (a.kind === "text-filter")
      setFacets((f) => [...f, { type: "text", name: c, columnName: c, query: "", mode: "text", caseSensitive: false, invert: false }])
    else if (a.kind === "common")
      apply({ op: "core/text-transform", engineConfig: engine, columnName: c, expression: `grel:${a.expression}`, onError: "keep-original", repeat: false, repeatCount: 10 }).catch(() => {})
    else if (a.kind === "transform" || a.kind === "cluster") setModal({ kind: a.kind, column: c })
    else if (a.kind === "add-column") setModal({ kind: "add", column: c })
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

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!summary || !(e.ctrlKey || e.metaKey) || (e.target as HTMLElement).closest("input,textarea")) return
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
    <Toaster>
      <div className="flex h-svh flex-col bg-background text-foreground">
        <header className="flex items-center gap-2 border-b px-3 py-2">
          <TableIcon />
          <span className="font-semibold">OxRefine</span>
          {summary && (
            <Button variant="ghost" size="sm" onClick={goHome}>
              <HouseIcon data-icon="inline-start" />
              Projects
            </Button>
          )}
          {summary && (
            <>
              <Separator orientation="vertical" className="mx-1 h-5" />
              <span className="truncate text-sm">{summary.name}</span>
              <Badge variant="secondary">
                {summary.rows.toLocaleString()} rows × {summary.columns.length} columns
              </Badge>
            </>
          )}
          <div className="ml-auto flex items-center gap-1">
            <Button variant="ghost" size="icon" aria-label="Undo" disabled={!summary?.cursor} onClick={() => goto(summary!.cursor - 1)}>
              <Undo2Icon />
            </Button>
            <Button
              variant="ghost"
              size="icon"
              aria-label="Redo"
              disabled={!summary || summary.cursor >= summary.history.length}
              onClick={() => goto(summary!.cursor + 1)}
            >
              <Redo2Icon />
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button variant="outline" disabled={!summary} />}>Rows</DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-60">
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
            <Button variant="outline" disabled={!summary} onClick={applyRecipe}>
              <FileUpIcon data-icon="inline-start" />
              Apply recipe
            </Button>
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button variant="outline" disabled={!summary} />}>
                <DownloadIcon data-icon="inline-start" />
                Export
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Matching rows</DropdownMenuLabel>
                  <DropdownMenuItem onClick={() => exportRows("csv", "csv")}>CSV…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => exportRows("tsv", "tsv")}>TSV…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => exportRows("xlsx", "xlsx")}>Excel (.xlsx)…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => exportRows("html", "html")}>HTML table…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setModal({ kind: "export", format: "*sv" })}>Custom tabular…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setModal({ kind: "export", format: "template" })}>Templating…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setModal({ kind: "export", format: "sql" })}>SQL…</DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Whole project</DropdownMenuLabel>
                  <DropdownMenuItem onClick={exportProject}>OpenRefine project…</DropdownMenuItem>
                </DropdownMenuGroup>
                <DropdownMenuSeparator />
                <DropdownMenuGroup>
                  <DropdownMenuLabel>Operation history</DropdownMenuLabel>
                  <DropdownMenuItem onClick={() => saveRecipe()}>Recipe JSON…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => setModal({ kind: "extract" })}>Pick steps…</DropdownMenuItem>
                  <DropdownMenuItem onClick={() => copyRecipe().catch(fail)}>Copy recipe JSON</DropdownMenuItem>
                </DropdownMenuGroup>
              </DropdownMenuContent>
            </DropdownMenu>
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button />}>
                <FolderOpenIcon data-icon="inline-start" />
                Import
              </DropdownMenuTrigger>
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
          </div>
        </header>

        {summary ? (
          <div className="flex min-h-0 flex-1">
            <aside className="flex w-80 shrink-0 flex-col border-r">
              <Tabs defaultValue="facets" className="flex min-h-0 flex-1 flex-col p-2">
                <TabsList className="w-full">
                  <TabsTrigger value="facets">Facets / Filters</TabsTrigger>
                  <TabsTrigger value="history">History ({summary.cursor})</TabsTrigger>
                </TabsList>
                <ScrollArea className="min-h-0 flex-1">
                  <TabsContent value="facets">
                    <Facets facets={facets} data={facetData} onChange={setFacets} onEditChoice={editChoice} />
                  </TabsContent>
                  <TabsContent value="history">
                    <History summary={summary} onGoto={goto} onExtract={() => setModal({ kind: "extract" })} />
                  </TabsContent>
                </ScrollArea>
              </Tabs>
            </aside>
            <main className="min-w-0 flex-1">
              <Grid columns={summary.columns} engine={engine} version={version} onTotal={setTotal} onAction={onAction} onApply={apply} />
            </main>
          </div>
        ) : (
          <Home onImport={openFile} onPaste={pasteData} onUrl={fromUrl} onOpenRefine={openOpenRefine} onOpen={openProject} onRename={renameProject} onError={fail} />
        )}

        <footer className="flex items-center gap-3 border-t px-3 py-1 text-xs text-muted-foreground">
          {summary && (
            <>
              <ToggleGroup size="sm" variant="outline" value={[mode]} onValueChange={(v) => v[0] && setMode(v[0] as Engine["mode"])} aria-label="Show rows or records">
                <ToggleGroupItem value="row-based">Rows</ToggleGroupItem>
                <ToggleGroupItem value="record-based">Records</ToggleGroupItem>
              </ToggleGroup>
              <span className="tabular-nums">
                {total.toLocaleString()} of {summary.rows.toLocaleString()} rows match
              </span>
            </>
          )}
          {lastMs && (
            <span className="ml-auto tabular-nums">
              last {lastMs.what}: {lastMs.ms.toFixed(1)} ms
            </span>
          )}
        </footer>
      </div>

      <Jobs />
      {modal?.kind === "ask" && <AskDialog {...modal} onClose={() => setModal(null)} />}
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
      {(modal?.kind === "transform" || modal?.kind === "add" || modal?.kind === "facet-list" || modal?.kind === "facet-range") && summary && (
        <TransformDialog
          column={modal.column}
          mode={modal.kind}
          columns={summary.columns}
          engine={engine}
          onApply={apply}
          onFacet={(expression) => setFacets((f) => [...f, modal.kind === "facet-range" ? rangeFacet(modal.column, expression) : listFacet(modal.column, expression)])}
          onClose={() => setModal(null)}
        />
      )}
      {modal?.kind === "export" && summary && (
        <ExportDialog
          format={modal.format}
          columns={summary.columns}
          engine={engine}
          onExport={(params) => {
            setModal(null)
            exportRows(modal.format, EXTENSION[modal.format], params)
          }}
          onClose={() => setModal(null)}
        />
      )}
      {modal?.kind === "extract" && summary && (
        <ExtractDialog
          summary={summary}
          onCopy={(steps) => copyRecipe(steps).catch(fail)}
          onSave={(steps) => {
            setModal(null)
            saveRecipe(steps)
          }}
          onClose={() => setModal(null)}
        />
      )}
      {modal?.kind === "sort" && <SortDialog column={modal.column} onApply={apply} onClose={() => setModal(null)} />}
      {modal?.kind === "cluster" && (
        <ClusterDialog column={modal.column} engine={engine} onApply={apply} onClose={() => setModal(null)} />
      )}
    </Toaster>
  )
}

export default App
