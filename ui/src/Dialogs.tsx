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
  "facet-list": (c: string) => `Custom text facet on “${c}”`,
  "facet-range": (c: string) => `Custom numeric facet on “${c}”`,
}
const HINTS = {
  transform: "Runs on the rows matching the current facets: once per distinct value, or once per row when it reads other cells.",
  add: "Runs on the rows matching the current facets; other rows get an empty cell.",
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
  const [preview, setPreview] = useState<[string | null, Result][]>([])
  const [error, setError] = useState("")
  const debounced = useDebounced(expr)

  useEffect(() => {
    call<[string | null, Result][]>("preview", { engine, column, expression: debounced, limit: 20 })
      .then(({ data }) => (setPreview(data), setError("")))
      .catch((e) => setError(e.message))
  }, [debounced, column, engine])

  const nameTaken = mode === "add" && columns.includes(name.trim())
  const blocked = !!error || (mode === "add" && (!name.trim() || nameTaken))
  const apply = () => {
    if (blocked) return
    rememberExpression(expr.trim())
    if (mode === "facet-list" || mode === "facet-range") {
      onFacet?.(grel(expr))
      return onClose()
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
          {mode === "add" && (
            <Field data-invalid={nameTaken}>
              <FieldLabel htmlFor="new-column">New column name</FieldLabel>
              <Input id="new-column" value={name} aria-invalid={nameTaken} onChange={(e) => setName(e.target.value)} />
              {nameTaken && <FieldError>A column with this name already exists.</FieldError>}
            </Field>
          )}
          <Field data-invalid={!!error}>
            <div className="flex items-end justify-between gap-2">
              <FieldLabel htmlFor="expression">GREL expression</FieldLabel>
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
