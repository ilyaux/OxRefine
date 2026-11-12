import { useEffect, useMemo, useState } from "react"
import { call, type Engine, type Op } from "@/api"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Textarea } from "@/components/ui/textarea"
import { ExpressionEditor, recentExpressions, rememberExpression } from "@/ExpressionEditor"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { CellButton, FullView, Metric, RailLabel, Spacer, StripAction, SummaryStrip, Tag } from "@/components/instrument"
import docs from "@/grel-docs.json"
import { cn, maxOf, minOf } from "@/lib/utils"

/** Leading/trailing spaces are the usual culprit in near-duplicates, so make them visible. */
const vis = (s: string | null) => (s === null ? "(null)" : s === "" ? "(blank)" : s.replace(/^ +| +$/g, (m) => "␣".repeat(m.length)))
/** A preview result: text, null, or an error the transform would hit (onError decides what the cell gets). */
type Result = string | null | { error: string }
const show = (r: Result) => (r !== null && typeof r === "object" ? `Error: ${r.error}` : vis(r))
const grel = (e: string) => (e.trim().startsWith("grel:") ? e.trim() : `grel:${e.trim()}`)

export function useDebounced<T>(value: T, ms = 200) {
  const [v, setV] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return v
}

const TITLES = {
  transform: "Transform",
  add: "Add column",
  fetch: "Fetch URLs",
  "facet-list": "Custom text facet",
  "facet-range": "Custom numeric facet",
}

type Doc = { params?: string; returns?: string; description?: string }
const FUNCTIONS = Object.entries(docs.functions as Record<string, Doc>).sort(([a], [b]) => a.localeCompare(b, "en", { sensitivity: "base" }))

/** The GREL function reference: search, the picked function's description, and the list. */
function FunctionReference() {
  const [q, setQ] = useState("")
  const [picked, setPicked] = useState("toTitlecase")
  const shown = FUNCTIONS.filter(([n, d]) => !q || n.toLowerCase().includes(q.toLowerCase()) || d.description?.toLowerCase().includes(q.toLowerCase()))
  const doc = (docs.functions as Record<string, Doc>)[picked]
  return (
    <aside aria-label="GREL functions" className="flex w-75 shrink-0 flex-col border-l">
      <label className="flex h-9.5 shrink-0 items-center border-b bg-muted">
        <span className="sr-only">Search functions</span>
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search functions…" className="h-full w-full bg-transparent px-4 outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset" />
      </label>
      {doc && (
        <div className="border-b px-4 py-4">
          <div className="font-mono text-[12.5px] font-semibold">
            {picked}({doc.params ?? ""})
          </div>
          <p className="mt-1.5 leading-relaxed text-ink-2">{doc.description}</p>
        </div>
      )}
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {shown.map(([n, d]) => (
          <li key={n}>
            <button
              type="button"
              aria-pressed={n === picked}
              onClick={() => setPicked(n)}
              className={cn("flex w-full items-baseline gap-2 border-b border-row-rule px-4 py-2 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset", n === picked && "bg-secondary")}
            >
              <span className="min-w-0 flex-1 truncate font-mono text-[12.5px]">
                {n}({d.params ?? ""})
              </span>
              <span className="shrink-0 text-[11px] text-muted-foreground">{d.returns}</span>
            </button>
          </li>
        ))}
        {!shown.length && <li className="px-4 py-3 text-muted-foreground">No function matches.</li>}
      </ul>
    </aside>
  )
}
const HINTS = {
  transform: "Runs on the rows matching the current facets: once per distinct value, or once per row when it reads other cells.",
  add: "Runs on the rows matching the current facets; other rows get an empty cell.",
  fetch: "The expression gives each matching row's URL; the new column gets what each address returns. Requests go one at a time, with the delay between them, and can be cancelled.",
  "facet-list": "Each distinct result becomes a choice; an array gives one choice per item.",
  "facet-range": "Numbers go into the histogram; text, blanks and errors are counted separately.",
}

export function TransformDialog({
  column,
  mode,
  columns,
  engine,
  rows,
  onApply,
  onFacet,
  onClose,
}: {
  column: string
  mode: keyof typeof TITLES
  columns: string[]
  engine: Engine
  /** how many rows match the facets: the ones the transform runs on */
  rows?: number
  onApply: (op: Op) => Promise<void>
  onFacet?: (expression: string) => void
  onClose: () => void
}) {
  const [expr, setExpr] = useState("value")
  const [recent] = useState(recentExpressions)
  const [name, setName] = useState("")
  // fetching URLs: OpenRefine's defaults
  const [delay, setDelay] = useState(5000)
  const [fetchError, setFetchError] = useState("set-to-blank")
  const [cache, setCache] = useState(true)
  const [headers, setHeaders] = useState("")
  const [preview, setPreview] = useState<[string | null, Result][]>([])
  const [ms, setMs] = useState<number | null>(null)
  const [error, setError] = useState("")
  // OpenRefine's text-transform options; their defaults are what this dialog always used
  const [onError, setOnError] = useState("keep-original")
  const [repeat, setRepeat] = useState(false)
  const [onlyChanged, setOnlyChanged] = useState(false)
  const debounced = useDebounced(expr)

  useEffect(() => {
    call<[string | null, Result][]>("preview", { engine, column, expression: debounced, limit: 20 })
      .then(({ data, ms }) => (setPreview(data), setMs(ms), setError("")))
      .catch((e) => setError(e.message))
  }, [debounced, column, engine])
  const changed = (a: string | null, b: Result) => !(typeof b === "string" || b === null) || b !== a
  const shownPreview = onlyChanged ? preview.filter(([a, b]) => changed(a, b)) : preview

  const named = mode === "add" || mode === "fetch"
  const nameTaken = named && columns.includes(name.trim())
  const blocked = !!error || (named && (!name.trim() || nameTaken))
  const apply = () => {
    if (blocked) return
    rememberExpression(expr.trim())
    if (mode === "facet-list" || mode === "facet-range") {
      onFacet?.(grel(expr))
      return onClose()
    }
    if (mode === "fetch") {
      const httpHeadersJson = headers
        .split("\n")
        .map((l) => l.split(/:(.*)/s))
        .filter(([n, v]) => n.trim() && v !== undefined)
        .map(([n, v]) => ({ name: n.trim(), value: v.trim() }))
      return onApply({
        op: "core/column-addition-by-fetching-urls",
        engineConfig: engine,
        baseColumnName: column,
        urlExpression: grel(expr),
        onError: fetchError,
        newColumnName: name.trim(),
        columnInsertIndex: columns.indexOf(column) + 1,
        delay,
        cacheResponses: cache,
        httpHeadersJson,
      }).then(onClose)
    }
    return onApply(
      mode === "add"
        ? {
            op: "core/column-addition",
            engineConfig: engine,
            baseColumnName: column,
            newColumnName: name.trim(),
            columnInsertIndex: columns.indexOf(column) + 1,
            expression: grel(expr),
            onError: "set-to-blank",
          }
        : {
            op: "core/text-transform",
            engineConfig: engine,
            columnName: column,
            expression: grel(expr),
            onError,
            repeat,
            repeatCount: 10,
          }
    ).then(onClose)
  }

  const field = "flex items-center gap-2.5 border-r px-4"
  return (
    <FullView
      crumbs={[
        TITLES[mode],
        <Tag key="c">{column}</Tag>,
        ...(rows !== undefined ? [<span key="r" className="font-mono text-xs font-normal text-muted-foreground">{rows.toLocaleString()} rows</span>] : []),
      ]}
      onClose={onClose}
    >
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-9.5 shrink-0 items-stretch border-b">
            <span className="-mb-px flex items-center border-r border-b-2 border-b-foreground px-4 font-medium">GREL</span>
            <Spacer />
            <span role="status" className={cn("flex items-center gap-1.5 border-l px-4 font-mono text-[11.5px]", error ? "text-destructive" : "text-ok-text")}>
              <span aria-hidden="true" className={cn("size-1.5", error ? "bg-destructive" : "bg-ok")} />
              {error ? "error" : `ok · ${preview.length} rows${ms !== null ? ` · ${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms` : ""}`}
            </span>
          </div>
          <div className="shrink-0 border-b px-4 py-3">
            <ExpressionEditor id="expression" value={expr} onChange={setExpr} onSubmit={apply} columns={columns} invalid={!!error} autoFocus />
            {error ? (
              <p role="alert" className="pt-2 text-xs text-destructive">
                {error}
              </p>
            ) : (
              <p className="pt-2 text-xs text-muted-foreground">
                {HINTS[mode]} <kbd>Ctrl Space</kbd> suggests functions, <kbd>Ctrl ↵</kbd> applies.
              </p>
            )}
            {recent.length > 0 && (
              <div className="flex flex-wrap items-center gap-1.5 pt-2 text-xs text-muted-foreground">
                Recent:
                {recent.slice(0, 5).map((r) => (
                  <button key={r} type="button" onClick={() => setExpr(r)} className="max-w-60 truncate rounded-[3px] border border-input px-1.5 font-mono text-[11.5px] text-ink-2 outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring">
                    {r}
                  </button>
                ))}
              </div>
            )}
          </div>
          {named && (
            <div className="flex shrink-0 flex-wrap items-start gap-x-6 gap-y-3 border-b border-b-foreground px-4 py-3">
              {named && (
                <label className="flex flex-col gap-1">
                  <span className="text-[11.5px] text-muted-foreground">New column name</span>
                  <Input value={name} aria-invalid={nameTaken} onChange={(e) => setName(e.target.value)} className="h-8 w-60" />
                  {nameTaken && <span className="text-xs text-destructive">A column with this name already exists.</span>}
                </label>
              )}
              {mode === "fetch" && (
                <>
                  <label className="flex flex-col gap-1">
                    <span className="text-[11.5px] text-muted-foreground">Delay between requests (ms)</span>
                    <Input type="number" min={0} value={delay} onChange={(e) => setDelay(Math.max(0, Number(e.target.value) || 0))} className="h-8 w-32 font-mono" />
                  </label>
                  <label className="flex flex-col gap-1">
                    <span className="text-[11.5px] text-muted-foreground">When a request fails</span>
                    <select value={fetchError} onChange={(e) => setFetchError(e.target.value)} className="h-8 rounded-[3px] border border-input bg-background px-2">
                      <option value="set-to-blank">leave blank</option>
                      <option value="store-error">store the error</option>
                    </select>
                  </label>
                  <label className="flex flex-col gap-1">
                    <span className="text-[11.5px] text-muted-foreground">HTTP headers, one per line as Name: value</span>
                    <Textarea className="min-h-8 w-72 font-mono" rows={1} placeholder="Authorization: Bearer …" value={headers} onChange={(e) => setHeaders(e.target.value)} />
                  </label>
                  <label className="flex items-center gap-2 self-center">
                    <input type="checkbox" checked={cache} onChange={(e) => setCache(e.target.checked)} className="accent-foreground" />
                    Fetch each distinct URL once
                  </label>
                </>
              )}
            </div>
          )}
          <div className={cn("flex h-10 shrink-0 items-center gap-3 border-b px-5", !named && "border-t border-t-foreground")}>
            <span className="font-semibold">Preview</span>
            <span className="font-mono text-xs text-muted-foreground">
              {preview.filter(([a, b]) => changed(a, b)).length} of {preview.length} rows change
            </span>
            <Spacer />
            <label className="flex items-center gap-2 text-ink-2">
              <input type="checkbox" checked={onlyChanged} onChange={(e) => setOnlyChanged(e.target.checked)} className="accent-foreground" />
              only changed
            </label>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">
            <table className="w-full table-fixed border-collapse">
              <thead>
                <tr className="text-left text-[11.5px] text-muted-foreground">
                  <th scope="col" className="border-b px-5 py-2 font-normal">
                    {column}
                  </th>
                  <th scope="col" className="w-10 border-b font-normal">
                    <span className="sr-only">becomes</span>
                  </th>
                  <th scope="col" className="border-b px-5 py-2 font-normal">
                    result
                  </th>
                </tr>
              </thead>
              <tbody>
                {shownPreview.map(([a, b], i) => {
                  const err = b !== null && typeof b === "object"
                  return (
                    <tr key={i} className="h-8.5">
                      <td className="truncate border-b px-5 font-mono text-[12.5px] text-ink-2">{vis(a)}</td>
                      <td aria-hidden="true" className="border-b text-center text-faint">
                        →
                      </td>
                      <td className="truncate border-b px-5 font-mono text-[12.5px]">
                        <span className={cn(err ? "text-destructive" : changed(a, b) && "bg-data/8")}>{show(b)}</span>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
            {onlyChanged && !shownPreview.length && <p className="px-5 py-4 text-muted-foreground">Nothing changes in these rows.</p>}
          </div>
          <div className="flex h-11 shrink-0 items-stretch border-t">
            {mode === "transform" && (
              <>
                <label className={field}>
                  <span className="text-muted-foreground">On error</span>
                  <select value={onError} onChange={(e) => setOnError(e.target.value)} className="h-7.5 rounded-[3px] border border-input bg-background px-2">
                    <option value="keep-original">keep original</option>
                    <option value="set-to-blank">set to blank</option>
                    <option value="store-error">store error</option>
                  </select>
                </label>
                <label className={field}>
                  <input type="checkbox" checked={repeat} onChange={(e) => setRepeat(e.target.checked)} className="accent-foreground" />
                  <span className="text-ink-2">repeat until no change</span>
                </label>
              </>
            )}
            <Spacer />
            <CellButton rule="left" onClick={onClose}>
              Cancel
            </CellButton>
            <CellButton rule="left" ink keys="Ctrl ↵" disabled={blocked} onClick={apply} className="px-6">
              {mode.startsWith("facet") ? "Add facet" : rows !== undefined ? `Apply to ${rows.toLocaleString()} rows` : "Apply"}
            </CellButton>
          </div>
        </div>
        <FunctionReference />
      </div>
    </FullView>
  )
}

type Cluster = [string, number][]
const SHOWN = 300
/** values listed per cluster; the rest are counted (a numeric column under a phonetic key can put every value in one) */
const LISTED = 50

/** OpenRefine's cluster facets: choices in the cluster, rows, average length of the choices and its variance. */
const STATS = [
  ["size", "Values"],
  ["rows", "Rows"],
  ["length", "Avg length"],
  ["spread", "Length variance"],
] as const
type Stat = (typeof STATS)[number][0]
const statsOf = (c: Cluster): Record<Stat, number> => {
  const lens = c.map(([v]) => [...v].length)
  const length = lens.reduce((a, b) => a + b, 0) / lens.length
  return {
    size: c.length,
    rows: c.reduce((s, [, n]) => s + n, 0),
    length,
    spread: lens.reduce((a, l) => a + (l - length) ** 2, 0) / lens.length,
  }
}
const num = (n: number) => (Number.isInteger(n) ? n.toLocaleString() : n.toFixed(1))

/** A cluster filter as two numeric inputs: values outside [from, to] hide the cluster. */
function StatFilter({ label, values, value, onChange }: { label: string; values: number[]; value: [number, number] | null; onChange: (v: [number, number] | null) => void }) {
  const min = minOf(values)
  const max = maxOf(values)
  const [lo, hi] = value ?? [min, max]
  if (!values.length || min === max) return null
  const commit = (which: 0 | 1, s: string) => {
    const n = Number(s)
    if (s.trim() === "" || !Number.isFinite(n)) return
    const next: [number, number] = which === 0 ? [n, hi] : [lo, n]
    onChange(next[0] <= min && next[1] >= max ? null : next)
  }
  const box = (which: 0 | 1, v: number) => (
    <input
      key={`${which}:${v}`}
      aria-label={`${label}: ${which ? "at most" : "at least"}`}
      defaultValue={num(v)}
      onBlur={(e) => commit(which, e.target.value.replace(/,/g, ""))}
      onKeyDown={(e) => e.key === "Enter" && commit(which, e.currentTarget.value.replace(/,/g, ""))}
      className="h-7 w-16 rounded-[3px] border border-input bg-background px-1.5 text-right font-mono text-[12px] outline-none focus-visible:ring-2 focus-visible:ring-ring"
    />
  )
  return (
    <div className="flex items-center gap-1.5 px-3.5 py-1">
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {box(0, lo)}
      {box(1, hi)}
    </div>
  )
}

/** OpenRefine's key-collision keying functions, in its menu order. */
const KEYERS: [string, string][] = [
  ["fingerprint", "Fingerprint"],
  ["ngram-fingerprint", "N-gram fingerprint"],
  ["metaphone3", "Metaphone 3"],
  ["cologne-phonetic", "Cologne phonetic"],
  ["daitch-mokotoff", "Daitch-Mokotoff"],
  ["beider-morse", "Beider-Morse"],
]

export function ClusterDialog({
  column,
  engine,
  onApply,
  onClose,
}: {
  column: string
  engine: Engine
  onApply: (op: Op) => Promise<void>
  onClose: () => void
}) {
  // OpenRefine's clustering dialog: key collision with a keying function, or nearest neighbours
  // with a distance, a radius and the size of the n-grams that decide which values get compared
  const [kind, setKind] = useState<"binning" | "knn">("binning")
  const [keyer, setKeyer] = useState("fingerprint")
  const [distance, setDistance] = useState("levenshtein")
  const [ngram, setNgram] = useState(2)
  const [radius, setRadius] = useState(1)
  const [block, setBlock] = useState(6)
  const [clusters, setClusters] = useState<Cluster[] | null>(null)
  const [error, setError] = useState("")
  const [picks, setPicks] = useState<{ merge: boolean; to: string }[]>([])
  const [ms, setMs] = useState(0)
  const [tick, setTick] = useState(0)
  const [filters, setFilters] = useState<Partial<Record<Stat, [number, number]>>>({})
  const method = kind === "binning" ? keyer : distance
  const stats = useMemo(() => (clusters ?? []).map(statsOf), [clusters])
  // the clusters the filters let through, by index into `clusters`
  const visible = useMemo(
    () => stats.flatMap((st, i) => (STATS.every(([k]) => !filters[k] || (st[k] >= filters[k]![0] - 1e-9 && st[k] <= filters[k]![1] + 1e-9)) ? [i] : [])),
    [stats, filters],
  )
  /** OpenRefine's "Export clusters" JSON. */
  const exported = () =>
    JSON.stringify(
      {
        columnName: column,
        timestamp: new Date().toISOString(),
        clusterMethod: kind,
        ...(kind === "binning" ? { keyingFunction: keyer, ...(keyer === "ngram-fingerprint" ? { ngramSize: ngram } : {}) } : { function: distance, radius, blockingNgramSize: block }),
        clusters: visible.map((i) => clusters![i].map(([v, c]) => ({ v, c }))),
      },
      null,
      2,
    )
  const download = () => {
    const a = document.createElement("a")
    a.href = URL.createObjectURL(new Blob([exported()], { type: "application/json" }))
    a.download = `clusters-${column}.json`
    a.click()
    URL.revokeObjectURL(a.href)
  }

  useEffect(() => {
    let live = true
    setClusters(null)
    setFilters({})
    setError("")
    call<Cluster[]>("cluster", { engine, column, method, n: ngram, radius, block }).then(
      ({ data, ms }) => {
        if (!live) return
        setClusters(data)
        setPicks(data.map((c) => ({ merge: false, to: c[0][0] })))
        setMs(ms)
      },
      (e) => live && setError(String((e as Error).message ?? e)),
    )
    return () => {
      live = false
    }
  }, [method, ngram, radius, block, column, engine, tick])

  const set = (i: number, p: Partial<{ merge: boolean; to: string }>) =>
    setPicks((all) => all.map((x, j) => (j === i ? { ...x, ...p } : x)))
  const chosen = picks.flatMap((p, i) => (p.merge && clusters ? [{ values: clusters[i], to: p.to }] : []))

  const merge = async (close: boolean) => {
    await onApply({
      op: "core/mass-edit",
      engineConfig: engine,
      columnName: column,
      expression: "value",
      edits: chosen.map((c) => ({ from: c.values.map(([v]) => v), fromBlank: false, fromError: false, to: c.to })),
    })
    if (close) onClose()
    else setTick((t) => t + 1)
  }

  // what checking the clusters would do: values merged into one each, and the cells that change
  const merged = chosen.reduce((n, c) => n + c.values.length, 0)
  const changing = chosen.reduce((n, c) => n + c.values.reduce((m, [v, k]) => m + (v === c.to ? 0 : k), 0), 0)
  const top = (c: Cluster) => maxOf(c.map(([, n]) => n), 1)
  const radio = (name: string, value: string, current: string, label: string, set: (v: string) => void, disabled?: boolean) => (
    <label
      key={value}
      className={cn("flex items-center gap-2.5 px-3.5 py-1.5", disabled ? "text-muted-foreground" : "cursor-pointer hover:bg-accent", current === value && "bg-secondary")}
    >
      <input type="radio" name={name} value={value} checked={current === value} disabled={disabled} onChange={() => set(value)} className="accent-foreground" />
      {label}
    </label>
  )

  return (
    <FullView
      crumbs={[
        "Cluster",
        <Tag key="c">{column}</Tag>,
      ]}
      onClose={onClose}
    >
      <div
        className="flex min-h-0 flex-1 flex-col"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && chosen.length) {
            e.preventDefault()
            merge(true)
          }
        }}
      >
        <SummaryStrip>
          <Metric label="values in checked clusters">
            {merged.toLocaleString()} <span className="text-faint">→</span> <span className="text-data">{chosen.length.toLocaleString()}</span>
          </Metric>
          <Metric label="cells to change">{changing.toLocaleString()}</Metric>
          <Metric label="clusters">
            {chosen.length.toLocaleString()} <span className="text-muted-foreground">of {visible.length.toLocaleString()}</span>
          </Metric>
          <Metric label="found in">
            {clusters ? (ms < 10 ? ms.toFixed(1) : Math.round(ms).toLocaleString()) : "…"} <span className="text-[13px] text-muted-foreground">ms</span>
          </Metric>
          <Spacer />
          <StripAction disabled={!chosen.length} onClick={() => merge(false)}>
            Merge and re-cluster
          </StripAction>
          <StripAction ink keys="Ctrl ↵" disabled={!chosen.length} onClick={() => merge(true)}>
            Merge {chosen.length || ""} cluster{chosen.length === 1 ? "" : "s"}
          </StripAction>
        </SummaryStrip>
        <div className="flex min-h-0 flex-1">
          <aside aria-label="Clustering method" className="w-65 shrink-0 overflow-y-auto border-r pb-4">
            <RailLabel>Method</RailLabel>
            <div role="radiogroup" aria-label="Method">
              {radio("cl-kind", "binning", kind, "Key collision", (v) => setKind(v as "binning" | "knn"))}
              {radio("cl-kind", "knn", kind, "Nearest neighbour", (v) => setKind(v as "binning" | "knn"))}
            </div>
            <div className="mt-3 border-t" />
            {kind === "binning" ? (
              <>
                <RailLabel>Keying function</RailLabel>
                <div role="radiogroup" aria-label="Keying function">
                  {KEYERS.map(([v, label]) => radio("cl-keyer", v, keyer, label, setKeyer))}
                </div>
                {keyer === "ngram-fingerprint" && (
                  <label className="flex items-center gap-2 px-3.5 pt-2">
                    <span className="flex-1">N-gram size</span>
                    <Input type="number" min={1} value={ngram} onChange={(e) => setNgram(Math.max(1, Number(e.target.value) || 1))} className="h-7 w-16 text-right font-mono" />
                  </label>
                )}
              </>
            ) : (
              <>
                <RailLabel>Distance</RailLabel>
                <div role="radiogroup" aria-label="Distance">
                  {radio("cl-distance", "levenshtein", distance, "Levenshtein", setDistance)}
                  {radio("cl-distance", "ppm", distance, "PPM", setDistance)}
                </div>
                <label className="flex items-center gap-2 px-3.5 pt-2">
                  <span className="flex-1">Radius</span>
                  <Input type="number" min={0} step={0.5} value={radius} onChange={(e) => setRadius(Number(e.target.value) || 0)} className="h-7 w-16 text-right font-mono" />
                </label>
                <label className="flex items-center gap-2 px-3.5 pt-2">
                  <span className="flex-1">Block chars</span>
                  <Input type="number" min={1} value={block} onChange={(e) => setBlock(Math.max(1, Number(e.target.value) || 1))} className="h-7 w-16 text-right font-mono" />
                </label>
              </>
            )}
            {clusters && clusters.length > 1 && (
              <>
                <div className="mt-3 border-t" />
                <RailLabel>Show clusters where</RailLabel>
                {STATS.map(([k, label]) => (
                  <StatFilter key={k} label={label} values={stats.map((s) => s[k])} value={filters[k] ?? null} onChange={(v) => setFilters((f) => ({ ...f, [k]: v ?? undefined }))} />
                ))}
              </>
            )}
            <div className="mt-3 border-t" />
            <div className="flex flex-col items-start gap-1 px-2 pt-2">
              <Button variant="ghost" size="sm" disabled={!visible.length} onClick={() => navigator.clipboard.writeText(exported())}>
                Copy clusters as JSON
              </Button>
              <Button variant="ghost" size="sm" disabled={!visible.length} onClick={download}>
                Export clusters…
              </Button>
            </div>
          </aside>
          <div className="min-w-0 flex-1 overflow-y-auto">
            {error ? (
              <p role="alert" className="p-5 text-destructive">
                {error}
              </p>
            ) : !clusters ? (
              <div className="flex justify-center p-10">
                <Spinner />
              </div>
            ) : !clusters.length ? (
              <p className="p-5 text-muted-foreground">No clusters with this method. Try another keying function, or nearest neighbours with a larger radius.</p>
            ) : (
              <table className="w-full border-collapse">
                <thead className="sticky top-0 z-10 bg-background">
                  <tr className="text-left text-[11.5px] text-muted-foreground">
                    <th scope="col" className="w-14 border-b border-b-foreground py-2.5 pl-4 font-normal">
                      <input
                        type="checkbox"
                        aria-label="Check all shown clusters"
                        checked={visible.length > 0 && visible.every((i) => picks[i]?.merge)}
                        onChange={(e) => {
                          const shown = new Set(visible)
                          setPicks((all) => all.map((p, i) => (shown.has(i) ? { ...p, merge: e.target.checked } : p)))
                        }}
                        className="accent-foreground"
                      />
                    </th>
                    <th scope="col" className="border-b border-b-foreground font-normal">
                      Values in cluster
                    </th>
                    <th scope="col" className="w-28 border-b border-b-foreground pr-6 text-right font-normal">
                      Rows
                    </th>
                    <th scope="col" className="w-80 border-b border-b-foreground pr-4 font-normal">
                      Merge into
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {visible.slice(0, SHOWN).map((i) => [clusters[i], i] as const).map(([c, i]) => {
                    const on = picks[i]?.merge ?? false
                    return (
                      <tr key={`${i}:${c[0][0]}`} className={cn("align-top", !on && "bg-muted")}>
                        <td className="border-b py-3 pl-4">
                          <input type="checkbox" aria-label={`Merge cluster ${i + 1}`} checked={on} onChange={(e) => set(i, { merge: e.target.checked })} className="accent-foreground" />
                        </td>
                        <td className="border-b py-2">
                          <ul className="flex flex-col">
                            {c.slice(0, LISTED).map(([v, n], k) => (
                              <li key={v} className="grid grid-cols-[minmax(0,1fr)_120px_64px] items-center gap-4 py-0.5">
                                <button
                                  type="button"
                                  title="Merge into this value"
                                  onClick={() => set(i, { to: v })}
                                  className={cn("truncate text-left font-mono text-[12.5px] outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring", picks[i]?.to === v && "font-semibold")}
                                >
                                  {vis(v)}
                                </button>
                                <span aria-hidden="true" className="h-1 bg-row-rule">
                                  <span className={cn("block h-1", k === 0 ? (on ? "bg-data" : "bg-ink-2") : "bg-faint")} style={{ width: `${(n / top(c)) * 100}%` }} />
                                </span>
                                <span className="text-right font-mono text-[12px] text-muted-foreground">{n.toLocaleString()}</span>
                              </li>
                            ))}
                            {c.length > LISTED && (
                              <li className="py-0.5 font-mono text-[12px] text-muted-foreground">+ {(c.length - LISTED).toLocaleString()} more values</li>
                            )}
                          </ul>
                        </td>
                        <td className="border-b py-2.5 pr-6 text-right font-mono text-[12.5px] font-semibold">{c.reduce((s, [, n]) => s + n, 0).toLocaleString()}</td>
                        <td className="border-b py-2 pr-4">
                          <Input
                            aria-label={`New value for cluster ${i + 1}`}
                            value={picks[i]?.to ?? ""}
                            onChange={(e) => set(i, { to: e.target.value, merge: true })}
                            className={cn("h-8", on && "border-foreground")}
                          />
                        </td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            )}
            {clusters && (visible.length < clusters.length || visible.length > SHOWN) && (
              <p className="px-4 py-3 text-xs text-muted-foreground">
                {visible.length < clusters.length && `${visible.length.toLocaleString()} of ${clusters.length.toLocaleString()} clusters pass the filters. `}
                {visible.length > SHOWN && `Showing the ${SHOWN} largest; merge some or narrow the filters to see the rest.`}
              </p>
            )}
          </div>
        </div>
      </div>
    </FullView>
  )
}

export function SortDialog({ column, onApply, onClose }: { column: string; onApply: (op: Op) => Promise<void>; onClose: () => void }) {
  const [type, setType] = useState("string")
  const [order, setOrder] = useState("asc")
  const apply = () =>
    onApply({
      op: "core/row-reorder",
      mode: "row-based",
      sorting: {
        criteria: [{ column, valueType: type, reverse: order === "desc", blankPosition: 2, errorPosition: 1, ...(type === "string" ? { caseSensitive: false } : {}) }],
      },
    }).then(onClose)
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Sort rows by “{column}”</DialogTitle>
          <DialogDescription>Reorders the rows for good and records the step in history. Blank and unparseable values go last.</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <FieldLabel>Sort values as</FieldLabel>
            <ToggleGroup variant="outline" value={[type]} onValueChange={(v) => v[0] && setType(v[0])}>
              <ToggleGroupItem value="string">Text</ToggleGroupItem>
              <ToggleGroupItem value="number">Numbers</ToggleGroupItem>
              <ToggleGroupItem value="date">Dates</ToggleGroupItem>
              <ToggleGroupItem value="boolean">Booleans</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          <Field>
            <FieldLabel>Order</FieldLabel>
            <ToggleGroup variant="outline" value={[order]} onValueChange={(v) => v[0] && setOrder(v[0])}>
              <ToggleGroupItem value="asc">Ascending</ToggleGroupItem>
              <ToggleGroupItem value="desc">Descending</ToggleGroupItem>
            </ToggleGroup>
          </Field>
        </FieldGroup>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button onClick={apply}>Sort</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

export function AskDialog({
  title,
  label,
  initial = "",
  multiline,
  fileAccept,
  onUpload,
  onSubmit,
  onClose,
}: {
  title: string
  label: string
  initial?: string
  multiline?: boolean
  /** offer "Load from file…" for these file types; the file's text fills the box */
  fileAccept?: string
  /** offer "Upload from this computer…" (the browser app talking to `ox serve`) */
  onUpload?: (f: File) => Promise<void>
  onSubmit: (v: string) => Promise<void> | void
  onClose: () => void
}) {
  const [v, setV] = useState(initial)
  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    await onSubmit(v)
    onClose()
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className={multiline ? "sm:max-w-2xl" : undefined}>
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>{title}</DialogTitle>
          </DialogHeader>
          <Field>
            <FieldLabel htmlFor="ask">{label}</FieldLabel>
            {multiline ? (
              <Textarea id="ask" className="h-64 font-mono" value={v} onChange={(e) => setV(e.target.value)} autoFocus />
            ) : (
              <Input id="ask" value={v} onChange={(e) => setV(e.target.value)} autoFocus />
            )}
          </Field>
          <DialogFooter>
            {onUpload && (
              <Button variant="outline" className="sm:mr-auto" render={<label />} nativeButton={false}>
                Upload from this computer…
                <input
                  type="file"
                  className="sr-only"
                  onChange={(e) => {
                    const f = e.target.files?.[0]
                    if (f) onUpload(f).then(onClose)
                  }}
                />
              </Button>
            )}
            {fileAccept && (
              <Button variant="outline" className="sm:mr-auto" render={<label />} nativeButton={false}>
                Load from file…
                <input type="file" accept={fileAccept} className="sr-only" onChange={(e) => e.target.files?.[0]?.text().then(setV)} />
              </Button>
            )}
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={!v.trim()}>
              OK
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

/** OpenRefine's Undo/Redo as a full view: every step (go to one to undo or redo to it), and the recipe of
 * the steps picked, to copy or save; single-cell edits and undone steps can't go into a recipe. */
export function HistoryView({
  summary,
  onGoto,
  onCopy,
  onSave,
  onApplyRecipe,
  onClose,
}: {
  summary: { name: string; rows: number; columns: string[]; history: string[]; recipe: boolean[]; cursor: number }
  onGoto: (index: number) => void
  onCopy: (steps: number[]) => Promise<void>
  onSave: (steps: number[]) => void
  onApplyRecipe: () => void
  onClose: () => void
}) {
  const usable = summary.history.map((_, i) => i).filter((i) => i < summary.cursor && summary.recipe[i])
  const [picked, setPicked] = useState<number[] | null>(null)
  // until the user picks, every step that can go into a recipe is in it
  const steps = (picked ?? usable).filter((i) => usable.includes(i))
  const [json, setJson] = useState<{ key: string; text: string } | null>(null)
  const key = steps.join(",")
  useEffect(() => {
    if (!steps.length) return
    let live = true
    call("export_recipe", { steps }).then(({ data }) => live && setJson({ key, text: JSON.stringify(data, null, 2) }), () => {})
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key])
  const toggle = (i: number) => setPicked(steps.includes(i) ? steps.filter((x) => x !== i) : [...steps, i].sort((a, b) => a - b))
  const undone = summary.history.length - summary.cursor
  const rows = ["Create project", ...summary.history]
  return (
    <FullView crumbs={["History"]} onClose={onClose}>
      <div className="flex min-h-0 flex-1">
        <section aria-label="Steps" className="flex min-w-0 flex-1 flex-col border-r">
          <div className="flex h-11 shrink-0 items-center gap-2 border-b border-b-foreground px-5">
            <span className="font-semibold">Steps</span>
            <span className="font-mono text-xs text-muted-foreground">
              {summary.cursor} applied{undone ? ` · ${undone} undone` : ""}
            </span>
            <Spacer />
            <kbd>Ctrl Z / Ctrl Y</kbd>
          </div>
          <ol className="min-h-0 flex-1 overflow-y-auto">
            {rows.map((d, i) => {
              const step = i - 1
              const current = i === summary.cursor
              const isUndone = i > summary.cursor
              const canPick = i > 0 && usable.includes(step)
              return (
                <li
                  key={i}
                  className={cn(
                    "flex items-start gap-3 border-b px-5 py-3",
                    current && "bg-secondary",
                    isUndone && "bg-[repeating-linear-gradient(135deg,transparent_0_6px,var(--row-rule)_6px_7px)] text-muted-foreground",
                  )}
                >
                  <span className="w-6 shrink-0 pt-px text-right font-mono text-[12px] text-muted-foreground">{i}</span>
                  {i === 0 ? (
                    <span aria-hidden="true" className="mt-1 size-2.5 shrink-0" />
                  ) : (
                    <input
                      type="checkbox"
                      aria-label={`Put step ${i} in the recipe`}
                      disabled={!canPick}
                      checked={steps.includes(step)}
                      onChange={() => toggle(step)}
                      title={canPick ? "In the recipe" : isUndone ? "Undone steps can't go into a recipe" : "Single-cell edits can't go into a recipe, as in OpenRefine"}
                      className="mt-0.5 accent-foreground"
                    />
                  )}
                  <button
                    type="button"
                    aria-current={current ? "step" : undefined}
                    onClick={() => onGoto(i)}
                    className="flex min-w-0 flex-1 flex-col items-start text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <span className={cn("font-medium", isUndone && "line-through")}>{d}</span>
                    {i === 0 && (
                      <span className="font-mono text-xs text-muted-foreground">
                        {summary.name} · {summary.rows.toLocaleString()} rows · {summary.columns.length} columns now
                      </span>
                    )}
                  </button>
                  <span className={cn("shrink-0 text-xs", current ? "font-medium text-foreground" : "text-muted-foreground")}>{current ? "current" : isUndone ? "redo" : ""}</span>
                </li>
              )
            })}
          </ol>
        </section>
        <section aria-label="Recipe" className="flex w-[50%] shrink-0 flex-col bg-muted">
          <div className="flex h-11 shrink-0 items-stretch border-b border-b-foreground">
            <div className="flex items-center gap-2 px-5">
              <span className="font-semibold">Recipe</span>
              <span className="font-mono text-xs text-muted-foreground">
                {steps.length} step{steps.length === 1 ? "" : "s"} · OpenRefine JSON
              </span>
            </div>
            <Spacer />
            <CellButton rule="left" onClick={onApplyRecipe}>
              Apply a recipe…
            </CellButton>
            <CellButton rule="left" disabled={!steps.length} onClick={() => onCopy(steps)}>
              Copy
            </CellButton>
            <CellButton rule="left" ink disabled={!steps.length} onClick={() => onSave(steps)}>
              Save .json
            </CellButton>
          </div>
          <pre className="min-h-0 flex-1 overflow-auto p-5 font-mono text-[12.5px] leading-relaxed whitespace-pre">
            {!steps.length ? "[]" : json?.key === key ? json.text : ""}
          </pre>
          <p className="shrink-0 border-t px-5 py-3 text-xs text-muted-foreground">
            Runs unchanged in OpenRefine 3.10.1: Undo/Redo → Apply, or with <code>ox apply</code>. Single-cell edits and single-row stars or flags can't be extracted, as in OpenRefine.
          </p>
        </section>
      </div>
    </FullView>
  )
}

/** OpenRefine's Transpose menu: columns into rows, rows into columns, key/value pairs into columns. */
export function TransposeDialog({
  column,
  columns,
  mode,
  onApply,
  onClose,
}: {
  column: string
  columns: string[]
  mode: "columns" | "rows" | "columnize"
  onApply: (op: Op) => Promise<void>
  onClose: () => void
}) {
  const after = columns.slice(columns.indexOf(column) + 1)
  const [count, setCount] = useState("2")
  const [into, setInto] = useState<"one" | "two">("two")
  const [combined, setCombined] = useState("")
  const [prepend, setPrepend] = useState(true)
  const [separator, setSeparator] = useState(":")
  const [keyName, setKeyName] = useState("key")
  const [valueName, setValueName] = useState("value")
  const [ignoreBlank, setIgnoreBlank] = useState(true)
  const [fillDown, setFillDown] = useState(false)
  const [rows, setRows] = useState(2)
  const [valueColumn, setValueColumn] = useState(after[0] ?? "")
  const [noteColumn, setNoteColumn] = useState("")
  const [error, setError] = useState("")

  const names = into === "one" ? [combined.trim()] : [keyName.trim(), valueName.trim()]
  const blocked = mode === "columns" ? names.some((n) => !n) : mode === "columnize" ? !valueColumn || valueColumn === column : rows < 1
  const op = (): Op =>
    mode === "columns"
      ? {
          op: "core/transpose-columns-into-rows",
          startColumnName: column,
          columnCount: count.trim() === "" ? -1 : Math.max(1, Number(count) || 1),
          ignoreBlankCells: ignoreBlank,
          fillDown,
          ...(into === "one"
            ? { combinedColumnName: combined.trim(), prependColumnName: prepend, separator }
            : { keyColumnName: keyName.trim(), valueColumnName: valueName.trim(), separator: null }),
        }
      : mode === "rows"
        ? { op: "core/transpose-rows-into-columns", columnName: column, rowCount: rows }
        : { op: "core/key-value-columnize", keyColumnName: column, valueColumnName: valueColumn, noteColumnName: noteColumn }
  const apply = () => !blocked && onApply(op()).then(onClose, (e) => setError(String((e as Error).message ?? e)))
  const pick = (value: string, set: (v: string) => void, label: string, none?: string) => (
    <Field>
      <FieldLabel>{label}</FieldLabel>
      <Select value={value || "\u0000"} onValueChange={(v) => set(v === "\u0000" ? "" : String(v))}>
        <SelectTrigger aria-label={label}>
          <SelectValue>{(v: string) => (v === "\u0000" ? none : v)}</SelectValue>
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            {none && <SelectItem value={"\u0000"}>{none}</SelectItem>}
            {columns.filter((c) => c !== column).map((c) => (
              <SelectItem key={c} value={c}>
                {c}
              </SelectItem>
            ))}
          </SelectGroup>
        </SelectContent>
      </Select>
    </Field>
  )

  const title = { columns: `Transpose cells across columns into rows, from “${column}”`, rows: `Transpose cells in “${column}” into columns`, columnize: `Columnize by key “${column}” and a value column` }[mode]
  const hint = {
    columns: "Each row becomes one row per column, holding that column's cell; the other columns stay as they are.",
    rows: "Every group of this many cells down the column goes side by side into new columns.",
    columnize: "Each distinct key becomes a column holding its values; rows that agree on every other column become one row.",
  }[mode]
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{hint}</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          {mode === "columns" && (
            <>
              <Field>
                <FieldLabel htmlFor="tr-count">How many columns, starting with this one</FieldLabel>
                <Input id="tr-count" type="number" min={1} placeholder={`All (${after.length + 1})`} value={count} onChange={(e) => setCount(e.target.value)} />
                <FieldDescription>Leave empty for this column and every one after it.</FieldDescription>
              </Field>
              <Field>
                <FieldLabel>Into</FieldLabel>
                <ToggleGroup variant="outline" value={[into]} onValueChange={(v) => v[0] && setInto(v[0] as "one" | "two")}>
                  <ToggleGroupItem value="two">A key and a value column</ToggleGroupItem>
                  <ToggleGroupItem value="one">One column</ToggleGroupItem>
                </ToggleGroup>
              </Field>
              {into === "two" ? (
                <div className="grid grid-cols-2 gap-4">
                  <Field>
                    <FieldLabel htmlFor="tr-key">Key column</FieldLabel>
                    <Input id="tr-key" value={keyName} onChange={(e) => setKeyName(e.target.value)} />
                  </Field>
                  <Field>
                    <FieldLabel htmlFor="tr-value">Value column</FieldLabel>
                    <Input id="tr-value" value={valueName} onChange={(e) => setValueName(e.target.value)} />
                  </Field>
                </div>
              ) : (
                <>
                  <Field>
                    <FieldLabel htmlFor="tr-combined">Column name</FieldLabel>
                    <Input id="tr-combined" value={combined} onChange={(e) => setCombined(e.target.value)} />
                  </Field>
                  <Field orientation="horizontal">
                    <Checkbox id="tr-prepend" checked={prepend} onCheckedChange={(c) => setPrepend(!!c)} />
                    <FieldLabel htmlFor="tr-prepend">Put the column name before each cell, followed by</FieldLabel>
                    <Input aria-label="Separator" className="w-16 font-mono" value={separator} disabled={!prepend} onChange={(e) => setSeparator(e.target.value)} />
                  </Field>
                </>
              )}
              <Field orientation="horizontal">
                <Checkbox id="tr-blank" checked={ignoreBlank} onCheckedChange={(c) => setIgnoreBlank(!!c)} />
                <FieldLabel htmlFor="tr-blank">Skip blank cells</FieldLabel>
              </Field>
              <Field orientation="horizontal">
                <Checkbox id="tr-fill" checked={fillDown} onCheckedChange={(c) => setFillDown(!!c)} />
                <FieldLabel htmlFor="tr-fill">Fill the other columns down into the new rows</FieldLabel>
              </Field>
            </>
          )}
          {mode === "rows" && (
            <Field>
              <FieldLabel htmlFor="tr-rows">Cells per row</FieldLabel>
              <Input id="tr-rows" type="number" min={1} value={rows} onChange={(e) => setRows(Math.max(1, Number(e.target.value) || 1))} />
            </Field>
          )}
          {mode === "columnize" && (
            <>
              {pick(valueColumn, setValueColumn, "Value column")}
              {pick(noteColumn, setNoteColumn, "Note column", "(none)")}
            </>
          )}
          {error && <FieldError>{error}</FieldError>}
        </FieldGroup>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button disabled={blocked} onClick={apply}>
            {mode === "columnize" ? "Columnize" : "Transpose"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
