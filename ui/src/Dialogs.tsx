import { useEffect, useState } from "react"
import { call, type Engine, type Op } from "@/api"
import { Badge } from "@/components/ui/badge"
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
import { ScrollArea } from "@/components/ui/scroll-area"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Textarea } from "@/components/ui/textarea"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { ExpressionEditor, recentExpressions, rememberExpression } from "@/ExpressionEditor"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

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
  transform: (c: string) => `Transform “${c}”`,
  add: (c: string) => `Add column based on “${c}”`,
  fetch: (c: string) => `Add column by fetching URLs based on “${c}”`,
  "facet-list": (c: string) => `Custom text facet on “${c}”`,
  "facet-range": (c: string) => `Custom numeric facet on “${c}”`,
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
  onApply,
  onFacet,
  onClose,
}: {
  column: string
  mode: keyof typeof TITLES
  columns: string[]
  engine: Engine
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
  const [error, setError] = useState("")
  const debounced = useDebounced(expr)

  useEffect(() => {
    call<[string | null, Result][]>("preview", { engine, column, expression: debounced, limit: 20 })
      .then(({ data }) => (setPreview(data), setError("")))
      .catch((e) => setError(e.message))
  }, [debounced, column, engine])

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
            onError: "keep-original",
            repeat: false,
            repeatCount: 10,
          }
    ).then(onClose)
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{TITLES[mode](column)}</DialogTitle>
          <DialogDescription>{HINTS[mode]}</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          {named && (
            <Field data-invalid={nameTaken}>
              <FieldLabel htmlFor="new-column">New column name</FieldLabel>
              <Input id="new-column" value={name} aria-invalid={nameTaken} onChange={(e) => setName(e.target.value)} />
              {nameTaken && <FieldError>A column with this name already exists.</FieldError>}
            </Field>
          )}
          {mode === "fetch" && (
            <div className="grid gap-4 sm:grid-cols-2">
              <Field>
                <FieldLabel htmlFor="fetch-delay">Delay between requests (ms)</FieldLabel>
                <Input id="fetch-delay" type="number" min={0} value={delay} onChange={(e) => setDelay(Math.max(0, Number(e.target.value) || 0))} />
              </Field>
              <Field>
                <FieldLabel>When a request fails</FieldLabel>
                <ToggleGroup variant="outline" value={[fetchError]} onValueChange={(v) => v[0] && setFetchError(v[0])}>
                  <ToggleGroupItem value="set-to-blank">Leave blank</ToggleGroupItem>
                  <ToggleGroupItem value="store-error">Store the error</ToggleGroupItem>
                </ToggleGroup>
              </Field>
              <Field>
                <FieldLabel htmlFor="fetch-headers">HTTP headers</FieldLabel>
                <Textarea id="fetch-headers" className="font-mono" rows={2} placeholder="Authorization: Bearer …" value={headers} onChange={(e) => setHeaders(e.target.value)} />
                <FieldDescription>One per line, as Name: value.</FieldDescription>
              </Field>
              <Field orientation="horizontal">
                <Checkbox id="fetch-cache" checked={cache} onCheckedChange={(c) => setCache(!!c)} />
                <FieldLabel htmlFor="fetch-cache">Fetch each distinct URL once</FieldLabel>
              </Field>
            </div>
          )}
          <Field data-invalid={!!error}>
            <div className="flex items-end justify-between gap-2">
              <FieldLabel htmlFor="expression">{mode === "fetch" ? "URL expression (GREL)" : "GREL expression"}</FieldLabel>
              {recent.length > 0 && (
                <Select value="" onValueChange={(v) => v && setExpr(String(v))}>
                  <SelectTrigger size="sm" className="max-w-60" aria-label="Recent expressions">
                    <SelectValue placeholder="Recent expressions" />
                  </SelectTrigger>
                  <SelectContent align="end">
                    <SelectGroup>
                      {recent.map((r) => (
                        <SelectItem key={r} value={r}>
                          <span className="truncate font-mono">{r}</span>
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              )}
            </div>
            <ExpressionEditor id="expression" value={expr} onChange={setExpr} onSubmit={apply} columns={columns} invalid={!!error} autoFocus />
            {error ? (
              <FieldError>{error}</FieldError>
            ) : (
              <FieldDescription>
                Ctrl+Space suggests functions; Ctrl+Enter applies. e.g. value.trim().toTitlecase() · value.replace(/\s+/, ' ')
              </FieldDescription>
            )}
          </Field>
        </FieldGroup>
        <ScrollArea className="h-64 rounded-lg border">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>{column}</TableHead>
                <TableHead>Result</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {preview.map(([a, b], i) => (
                <TableRow key={i}>
                  <TableCell className="max-w-xs truncate font-mono">{vis(a)}</TableCell>
                  <TableCell className="max-w-xs truncate font-mono">{show(b)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </ScrollArea>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button disabled={blocked} onClick={apply}>
            {mode.startsWith("facet") ? "Add facet" : "Apply"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

type Cluster = [string, number][]
const SHOWN = 300

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
  const method = kind === "binning" ? keyer : distance

  useEffect(() => {
    let live = true
    setClusters(null)
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

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Cluster and edit “{column}”</DialogTitle>
          <DialogDescription>
            {clusters
              ? `${clusters.length.toLocaleString()} clusters found in ${ms.toFixed(1)} ms. Check the ones to merge and adjust the new value.`
              : "Finding clusters…"}
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-wrap items-end gap-3">
          <Field className="w-auto">
            <FieldLabel>Method</FieldLabel>
            <ToggleGroup variant="outline" value={[kind]} onValueChange={(v) => v[0] && setKind(v[0] as "binning" | "knn")}>
              <ToggleGroupItem value="binning">Key collision</ToggleGroupItem>
              <ToggleGroupItem value="knn">Nearest neighbours</ToggleGroupItem>
            </ToggleGroup>
          </Field>
          {kind === "binning" ? (
            <>
              <Field className="w-56">
                <FieldLabel>Keying function</FieldLabel>
                <Select value={keyer} onValueChange={(v) => v && setKeyer(String(v))}>
                  <SelectTrigger aria-label="Keying function">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      {KEYERS.map(([v, label]) => (
                        <SelectItem key={v} value={v}>
                          {label}
                        </SelectItem>
                      ))}
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              {keyer === "ngram-fingerprint" && (
                <Field className="w-28">
                  <FieldLabel htmlFor="cl-ngram">N-gram size</FieldLabel>
                  <Input id="cl-ngram" type="number" min={1} value={ngram} onChange={(e) => setNgram(Math.max(1, Number(e.target.value) || 1))} />
                </Field>
              )}
            </>
          ) : (
            <>
              <Field className="w-44">
                <FieldLabel>Distance</FieldLabel>
                <Select value={distance} onValueChange={(v) => v && setDistance(String(v))}>
                  <SelectTrigger aria-label="Distance">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectGroup>
                      <SelectItem value="levenshtein">Levenshtein</SelectItem>
                      <SelectItem value="ppm" disabled>
                        PPM (not available yet)
                      </SelectItem>
                    </SelectGroup>
                  </SelectContent>
                </Select>
              </Field>
              <Field className="w-24">
                <FieldLabel htmlFor="cl-radius">Radius</FieldLabel>
                <Input id="cl-radius" type="number" min={0} step={0.5} value={radius} onChange={(e) => setRadius(Number(e.target.value) || 0)} />
              </Field>
              <Field className="w-32">
                <FieldLabel htmlFor="cl-block">Block chars</FieldLabel>
                <Input id="cl-block" type="number" min={1} value={block} onChange={(e) => setBlock(Math.max(1, Number(e.target.value) || 1))} />
              </Field>
            </>
          )}
        </div>
        <ScrollArea className="h-96 rounded-lg border">
          {error ? (
            <div className="flex h-96 items-center justify-center p-4">
              <FieldError>{error}</FieldError>
            </div>
          ) : !clusters ? (
            <div className="flex h-96 items-center justify-center">
              <Spinner />
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead className="w-10">
                    <Checkbox
                      aria-label="Select all clusters"
                      checked={picks.length > 0 && picks.every((p) => p.merge)}
                      onCheckedChange={(c) => setPicks((all) => all.map((p) => ({ ...p, merge: !!c })))}
                    />
                  </TableHead>
                  <TableHead>Values in cluster</TableHead>
                  <TableHead className="text-right">Rows</TableHead>
                  <TableHead>New cell value</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {clusters.slice(0, SHOWN).map((c, i) => (
                  <TableRow key={c.map(([v]) => v).join("\u0000")}>
                    <TableCell>
                      <Checkbox
                        aria-label={`Merge cluster ${i + 1}`}
                        checked={picks[i]?.merge ?? false}
                        onCheckedChange={(m) => set(i, { merge: !!m })}
                      />
                    </TableCell>
                    <TableCell>
                      <div className="flex flex-col gap-1">
                        {c.map(([v, n]) => (
                          <div key={v} className="flex items-center gap-2">
                            <Button variant="link" size="xs" onClick={() => set(i, { to: v })}>
                              <span className="font-mono">{vis(v)}</span>
                            </Button>
                            <Badge variant="secondary">{n.toLocaleString()}</Badge>
                          </div>
                        ))}
                      </div>
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {c.reduce((s, [, n]) => s + n, 0).toLocaleString()}
                    </TableCell>
                    <TableCell>
                      <Input
                        aria-label={`New value for cluster ${i + 1}`}
                        value={picks[i]?.to ?? ""}
                        onChange={(e) => set(i, { to: e.target.value, merge: true })}
                      />
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </ScrollArea>
        {clusters && clusters.length > SHOWN && (
          <p className="text-sm text-muted-foreground">
            Showing the {SHOWN} largest clusters; merge some or add a facet to see the rest.
          </p>
        )}
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Close</DialogClose>
          <Button variant="secondary" disabled={!chosen.length} onClick={() => merge(false)}>
            Merge {chosen.length || ""} and re-cluster
          </Button>
          <Button disabled={!chosen.length} onClick={() => merge(true)}>
            Merge and close
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
  onSubmit,
  onClose,
}: {
  title: string
  label: string
  initial?: string
  multiline?: boolean
  /** offer "Load from file…" for these file types; the file's text fills the box */
  fileAccept?: string
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

/** OpenRefine's Undo/Redo → Extract: pick steps, see their JSON, copy or save it. */
export function ExtractDialog({
  summary,
  onCopy,
  onSave,
  onClose,
}: {
  summary: { history: string[]; recipe: boolean[]; cursor: number }
  onCopy: (steps: number[]) => Promise<void>
  onSave: (steps: number[]) => void
  onClose: () => void
}) {
  const usable = summary.history.map((_, i) => i).filter((i) => i < summary.cursor && summary.recipe[i])
  const [steps, setSteps] = useState(usable)
  const [json, setJson] = useState("")
  useEffect(() => {
    if (!steps.length) return setJson("[]")
    call("export_recipe", { steps }).then(({ data }) => setJson(JSON.stringify(data, null, 2)), () => {})
  }, [steps])
  const toggle = (i: number, on: boolean) => setSteps((s) => (on ? [...s, i].sort((a, b) => a - b) : s.filter((x) => x !== i)))
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>Extract operation history</DialogTitle>
          <DialogDescription>
            Pick the steps to keep. The JSON applies in OpenRefine (Undo/Redo → Apply) or with ox apply. As in OpenRefine, single-cell edits and
            single-row stars or flags can't be extracted, and undone steps are not listed.
          </DialogDescription>
        </DialogHeader>
        <div className="grid min-h-0 gap-4 md:grid-cols-2">
          <ScrollArea className="h-96 rounded-md border p-2">
            <div className="flex flex-col gap-1">
              <Field orientation="horizontal">
                <Checkbox
                  id="ex-all"
                  checked={steps.length === usable.length && usable.length > 0}
                  indeterminate={steps.length > 0 && steps.length < usable.length}
                  onCheckedChange={(c) => setSteps(c ? usable : [])}
                />
                <FieldLabel htmlFor="ex-all">All steps</FieldLabel>
              </Field>
              {summary.history.slice(0, summary.cursor).map((d, i) => (
                <Field key={i} orientation="horizontal" data-disabled={!summary.recipe[i] || undefined}>
                  <Checkbox id={`ex-${i}`} disabled={!summary.recipe[i]} checked={steps.includes(i)} onCheckedChange={(c) => toggle(i, !!c)} />
                  <FieldLabel htmlFor={`ex-${i}`} className="font-normal">
                    <span className="w-6 shrink-0 tabular-nums text-muted-foreground">{i + 1}.</span>
                    {d}
                  </FieldLabel>
                </Field>
              ))}
            </div>
          </ScrollArea>
          <ScrollArea className="h-96 rounded-md border bg-muted">
            <pre className="p-2 font-mono text-xs whitespace-pre">{json}</pre>
          </ScrollArea>
        </div>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Close</DialogClose>
          <Button variant="outline" disabled={!steps.length} onClick={() => onCopy(steps)}>
            Copy JSON
          </Button>
          <Button disabled={!steps.length} onClick={() => onSave(steps)}>
            Save…
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
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
            Transpose
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
