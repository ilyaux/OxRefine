import { useEffect, useState } from "react"
import { FilterIcon, FlipHorizontal2Icon, PencilIcon, SearchIcon, XIcon } from "lucide-react"
import type { Facet, FacetData, ListData, ListFacet, RangeData, RangeFacet, Summary, TextFacet } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Field, FieldLabel } from "@/components/ui/field"
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Separator } from "@/components/ui/separator"
import { Slider } from "@/components/ui/slider"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

const SHOWN = 200
const fmt = (n: number) => (Math.abs(n) >= 1e6 || (n !== 0 && Math.abs(n) < 1e-3) ? n.toExponential(2) : Number(n.toFixed(6)).toLocaleString())

export function Facets({
  facets,
  data,
  onChange,
  onEditChoice,
}: {
  facets: Facet[]
  data: FacetData[]
  onChange: (f: Facet[]) => void
  onEditChoice: (facet: ListFacet, choice: string) => void
}) {
  if (!facets.length)
    return (
      <Empty>
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <FilterIcon />
          </EmptyMedia>
          <EmptyTitle>No facets yet</EmptyTitle>
          <EmptyDescription>Open a column menu and pick a facet or a text filter to slice the rows.</EmptyDescription>
        </EmptyHeader>
      </Empty>
    )
  const update = (i: number, f: Facet) => onChange(facets.map((x, j) => (j === i ? f : x)))
  return (
    <div className="flex flex-col gap-3">
      <Button variant="outline" size="sm" onClick={() => onChange([])}>
        Remove all facets
      </Button>
      {facets.map((f, i) => (
        <div key={`${f.type}:${f.columnName}:${i}`} className="flex flex-col gap-2 rounded-lg border p-2">
          <div className="flex items-center gap-1">
            <div className="flex min-w-0 flex-1 flex-col">
              <span className="truncate text-sm font-medium">{f.columnName}</span>
              {f.type !== "text" && f.expression !== "value" && f.expression !== "grel:value" && (
                <span className="truncate font-mono text-xs text-muted-foreground" title={f.expression}>
                  {f.expression.replace(/^grel:/, "")}
                </span>
              )}
            </div>
            {f.type !== "range" && f.invert && <Badge variant="outline">inverted</Badge>}
            {f.type !== "range" && (
              <Button variant="ghost" size="icon-xs" aria-label="Invert facet" onClick={() => update(i, { ...f, invert: !f.invert })}>
                <FlipHorizontal2Icon />
              </Button>
            )}
            <Button variant="ghost" size="icon-xs" aria-label="Remove facet" onClick={() => onChange(facets.filter((_, j) => j !== i))}>
              <XIcon />
            </Button>
          </div>
          {f.type === "list" && <ListChoices facet={f} data={data[i] as ListData | null} onChange={(nf) => update(i, nf)} onEdit={(c) => onEditChoice(f, c)} />}
          {f.type === "text" && <TextQuery facet={f} onChange={(nf) => update(i, nf)} />}
          {f.type === "range" && <RangePanel facet={f} data={data[i] as RangeData | null} onChange={(nf) => update(i, nf)} />}
        </div>
      ))}
    </div>
  )
}

function ListChoices({ facet, data, onChange, onEdit }: { facet: ListFacet; data: ListData | null; onChange: (f: ListFacet) => void; onEdit: (choice: string) => void }) {
  const [order, setOrder] = useState("count")
  if (!data || !("choices" in data)) return null
  const selected = (label: string) => facet.selection.some((s) => (s.v.l || String(s.v.v)) === label)
  const toggle = (label: string, v: unknown) =>
    onChange({
      ...facet,
      selection: selected(label) ? facet.selection.filter((s) => (s.v.l || String(s.v.v)) !== label) : [...facet.selection, { v: { v, l: label } }],
    })
  const choices = order === "name" ? [...data.choices].sort((a, b) => a[0].localeCompare(b[0])) : data.choices
  const editable = facet.expression === "value" || facet.expression === "grel:value"
  const special: [string, number, boolean, () => void][] = [
    ["(blank)", data.blank, facet.selectBlank, () => onChange({ ...facet, selectBlank: !facet.selectBlank })],
    ["(error)", data.error, facet.selectError, () => onChange({ ...facet, selectError: !facet.selectError })],
  ]
  return (
    <>
      <div className="flex items-center justify-between gap-2">
        <span className="text-xs text-muted-foreground">
          {data.distinct.toLocaleString()} choices
          {data.distinct > SHOWN && ` · top ${SHOWN} shown`}
        </span>
        <ToggleGroup size="sm" variant="outline" value={[order]} onValueChange={(v) => v[0] && setOrder(v[0])} aria-label="Sort choices">
          <ToggleGroupItem value="count">Count</ToggleGroupItem>
          <ToggleGroupItem value="name">Name</ToggleGroupItem>
        </ToggleGroup>
      </div>
      <ScrollArea className="max-h-64">
        <div className="flex flex-col">
          {choices.slice(0, SHOWN).map(([label, n, v]) => (
            <div key={label} className="group/choice flex items-center gap-1">
              <Button variant={selected(label) ? "secondary" : "ghost"} size="sm" className="min-w-0 flex-1 justify-between" onClick={() => toggle(label, v)}>
                <span className="truncate">{label}</span>
                <span className="tabular-nums text-muted-foreground">{n.toLocaleString()}</span>
              </Button>
              {editable && (
                <Button variant="ghost" size="icon-xs" aria-label={`Edit ${label}`} className="opacity-0 group-hover/choice:opacity-100 focus-visible:opacity-100" onClick={() => onEdit(label)}>
                  <PencilIcon />
                </Button>
              )}
            </div>
          ))}
          {special
            .filter(([, n, on]) => n > 0 || on)
            .map(([label, n, on, flip]) => (
              <Button key={label} variant={on ? "secondary" : "ghost"} size="sm" className="justify-between" onClick={flip}>
                <span className="text-muted-foreground italic">{label}</span>
                <span className="tabular-nums text-muted-foreground">{n.toLocaleString()}</span>
              </Button>
            ))}
        </div>
      </ScrollArea>
    </>
  )
}

function RangePanel({ facet, data, onChange }: { facet: RangeFacet; data: RangeData | null; onChange: (f: RangeFacet) => void }) {
  const [range, setRange] = useState<[number, number] | null>(null)
  useEffect(() => setRange(null), [facet.from, facet.to])
  if (!data) return null
  const kinds: [keyof RangeFacet, string, number][] = [
    ["selectNumeric", "Numeric", data.numericCount],
    ["selectNonNumeric", "Non-numeric", data.nonNumericCount],
    ["selectBlank", "Blank", data.blankCount],
    ["selectError", "Error", data.errorCount],
  ]
  const boxes = (
    <div className="flex flex-col gap-1">
      {kinds.map(([k, label, n]) => (
        <Field key={k} orientation="horizontal">
          <Checkbox id={`rf-${facet.columnName}-${k}`} checked={Boolean(facet[k])} onCheckedChange={(c) => onChange({ ...facet, [k]: !!c })} />
          <FieldLabel htmlFor={`rf-${facet.columnName}-${k}`} className="font-normal">
            {label} <span className="tabular-nums text-muted-foreground">{n.toLocaleString()}</span>
          </FieldLabel>
        </Field>
      ))}
    </div>
  )
  if (data.error || data.min === undefined || data.max === undefined || !data.bins || !data.baseBins) {
    return (
      <>
        <p className="text-xs text-muted-foreground">
          {data.error ?? "No numbers to chart."} If the column holds numbers as text, convert it with Edit cells → Common transforms → To number,
          or add a custom numeric facet on value.toNumber().
        </p>
        {boxes}
      </>
    )
  }
  const { min, max, step = 1, bins, baseBins } = data
  const [lo, hi] = range ?? [Math.max(facet.from, min), Math.min(facet.to, max)]
  const top = Math.max(1, ...baseBins)
  const w = 100 / baseBins.length
  return (
    <>
      <svg viewBox="0 0 100 40" preserveAspectRatio="none" className="h-12 w-full" role="img" aria-label={`Histogram of ${facet.columnName}`}>
        {baseBins.map((c, i) => {
          const x0 = min + i * step
          const inside = x0 + step > lo && x0 < hi
          return (
            <g key={i}>
              <rect x={i * w} y={40 - (c / top) * 40} width={w} height={(c / top) * 40} className="fill-muted" />
              <rect x={i * w} y={40 - (bins[i] / top) * 40} width={w} height={(bins[i] / top) * 40} className={inside ? "fill-primary" : "fill-muted-foreground"} />
            </g>
          )
        })}
      </svg>
      <Slider
        aria-label={`Range of ${facet.columnName}`}
        min={min}
        max={max}
        step={step}
        value={[lo, hi]}
        onValueChange={(v) => Array.isArray(v) && setRange([v[0], v[1]])}
        onValueCommitted={(v) => Array.isArray(v) && onChange({ ...facet, from: v[0], to: v[1] })}
      />
      <span className="text-xs tabular-nums text-muted-foreground">
        {fmt(lo)} – {fmt(hi)}
      </span>
      {boxes}
    </>
  )
}

function TextQuery({ facet, onChange }: { facet: TextFacet; onChange: (f: TextFacet) => void }) {
  const [q, setQ] = useState(facet.query)
  useEffect(() => {
    if (q === facet.query) return
    const t = setTimeout(() => onChange({ ...facet, query: q }), 250)
    return () => clearTimeout(t)
  }, [q, facet, onChange])
  return (
    <InputGroup>
      <InputGroupAddon>
        <SearchIcon />
      </InputGroupAddon>
      <InputGroupInput aria-label={`Filter ${facet.columnName}`} value={q} onChange={(e) => setQ(e.target.value)} autoFocus />
    </InputGroup>
  )
}

export function History({ summary, onGoto, onExtract }: { summary: Summary; onGoto: (i: number) => void; onExtract: () => void }) {
  const [q, setQ] = useState("")
  const steps = ["Create project", ...summary.history]
  const shown = steps.map((s, i) => [s, i] as const).filter(([s]) => !q || s.toLowerCase().includes(q.toLowerCase()))
  return (
    <div className="flex flex-col gap-1">
      <div className="mb-1 flex gap-1">
        <InputGroup>
          <InputGroupAddon>
            <SearchIcon />
          </InputGroupAddon>
          <InputGroupInput aria-label="Filter steps" placeholder="Filter steps" value={q} onChange={(e) => setQ(e.target.value)} />
        </InputGroup>
        <Button variant="outline" disabled={!summary.cursor} onClick={onExtract}>
          Extract…
        </Button>
      </div>
      {q && !shown.length && <p className="px-2 text-xs text-muted-foreground">No steps match.</p>}
      {shown.map(([s, i]) => (
        <Button
          key={i}
          variant={i === summary.cursor ? "secondary" : "ghost"}
          size="sm"
          className="h-auto justify-start py-1.5 text-left whitespace-normal"
          onClick={() => onGoto(i)}
        >
          <span className="w-6 shrink-0 tabular-nums text-muted-foreground">{i}.</span>
          <span className="min-w-0 flex-1">{s}</span>
          {i > summary.cursor && <Badge variant="outline">undone</Badge>}
        </Button>
      ))}
      <Separator className="my-2" />
      <p className="text-xs text-muted-foreground">
        Click a step to undo or redo to it. Extract gives the steps you pick as OpenRefine-compatible JSON.
      </p>
    </div>
  )
}
