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
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

/** Leading/trailing spaces are the usual culprit in near-duplicates, so make them visible. */
const vis = (s: string) => (s === "" ? "(blank)" : s.replace(/^ +| +$/g, (m) => "␣".repeat(m.length)))
const grel = (e: string) => (e.trim().startsWith("grel:") ? e.trim() : `grel:${e.trim()}`)

function useDebounced<T>(value: T, ms = 200) {
  const [v, setV] = useState(value)
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms)
    return () => clearTimeout(t)
  }, [value, ms])
  return v
}

export function TransformDialog({
  column,
  mode,
  columns,
  engine,
  onApply,
  onClose,
}: {
  column: string
  mode: "transform" | "add"
  columns: string[]
  engine: Engine
  onApply: (op: Op) => Promise<void>
  onClose: () => void
}) {
  const [expr, setExpr] = useState("value")
  const [name, setName] = useState("")
  const [preview, setPreview] = useState<[string, string][]>([])
  const [error, setError] = useState("")
  const debounced = useDebounced(expr)

  useEffect(() => {
    call<[string, string][]>("preview", { engine, column, expression: debounced, limit: 20 })
      .then(({ data }) => (setPreview(data), setError("")))
      .catch((e) => setError(e.message))
  }, [debounced, column, engine])

  const nameTaken = mode === "add" && columns.includes(name.trim())
  const apply = () =>
    onApply(
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

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>{mode === "add" ? `Add column based on “${column}”` : `Transform “${column}”`}</DialogTitle>
          <DialogDescription>
            Runs once per distinct value and applies only to rows matching the current facets.
          </DialogDescription>
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
            <FieldLabel htmlFor="expression">GREL expression</FieldLabel>
            <Textarea
              id="expression"
              className="font-mono"
              value={expr}
              aria-invalid={!!error}
              onChange={(e) => setExpr(e.target.value)}
              autoFocus
            />
            {error ? (
              <FieldError>{error}</FieldError>
            ) : (
              <FieldDescription>
                e.g. value.trim().toTitlecase() · value.replace(/\s+/, ' ') · value.fingerprint()
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
                  <TableCell className="max-w-xs truncate font-mono">{vis(b)}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        </ScrollArea>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button disabled={!!error || (mode === "add" && (!name.trim() || nameTaken))} onClick={apply}>
            Apply
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}

type Cluster = [string, number][]
const SHOWN = 300

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
  const [method, setMethod] = useState("fingerprint")
  const [clusters, setClusters] = useState<Cluster[] | null>(null)
  const [picks, setPicks] = useState<{ merge: boolean; to: string }[]>([])
  const [ms, setMs] = useState(0)
  const [tick, setTick] = useState(0)

  useEffect(() => {
    setClusters(null)
    call<Cluster[]>("cluster", { engine, column, method }).then(({ data, ms }) => {
      setClusters(data)
      setPicks(data.map((c) => ({ merge: false, to: c[0][0] })))
      setMs(ms)
    })
  }, [method, column, engine, tick])

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
        <ToggleGroup variant="outline" value={[method]} onValueChange={(v) => v[0] && setMethod(v[0])}>
          <ToggleGroupItem value="fingerprint">Fingerprint</ToggleGroupItem>
          <ToggleGroupItem value="ngram-fingerprint">N-gram fingerprint</ToggleGroupItem>
        </ToggleGroup>
        <ScrollArea className="h-96 rounded-lg border">
          {!clusters ? (
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

export function AskDialog({
  title,
  label,
  initial = "",
  multiline,
  onSubmit,
  onClose,
}: {
  title: string
  label: string
  initial?: string
  multiline?: boolean
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
