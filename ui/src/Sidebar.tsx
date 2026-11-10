import { useEffect, useState } from "react"
import { FilterIcon, FlipHorizontal2Icon, SearchIcon, XIcon } from "lucide-react"
import type { Facet, FacetData, ListFacet, Summary, TextFacet } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Separator } from "@/components/ui/separator"

const SHOWN = 200

export function Facets({
  facets,
  data,
  onChange,
}: {
  facets: Facet[]
  data: FacetData[]
  onChange: (f: Facet[]) => void
}) {
  if (!facets.length)
    return (
      <Empty>
        <EmptyHeader>
          <EmptyMedia variant="icon">
            <FilterIcon />
          </EmptyMedia>
          <EmptyTitle>No facets yet</EmptyTitle>
          <EmptyDescription>Open a column menu and pick Text facet or Text filter to slice the rows.</EmptyDescription>
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
            <span className="min-w-0 flex-1 truncate text-sm font-medium">{f.columnName}</span>
            {f.invert && <Badge variant="outline">inverted</Badge>}
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Invert facet"
              onClick={() => update(i, { ...f, invert: !f.invert })}
            >
              <FlipHorizontal2Icon />
            </Button>
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label="Remove facet"
              onClick={() => onChange(facets.filter((_, j) => j !== i))}
            >
              <XIcon />
            </Button>
          </div>
          {f.type === "list" ? (
            <ListChoices facet={f} data={data[i]} onChange={(nf) => update(i, nf)} />
          ) : (
            <TextQuery facet={f} onChange={(nf) => update(i, nf)} />
          )}
        </div>
      ))}
    </div>
  )
}

function ListChoices({ facet, data, onChange }: { facet: ListFacet; data: FacetData; onChange: (f: ListFacet) => void }) {
  if (!data) return null
  const selected = (v: string) => (v === "" ? facet.selectBlank : facet.selection.some((s) => s.v.v === v))
  const toggle = (v: string) =>
    onChange(
      v === ""
        ? { ...facet, selectBlank: !facet.selectBlank }
        : {
            ...facet,
            selection: selected(v)
              ? facet.selection.filter((s) => s.v.v !== v)
              : [...facet.selection, { v: { v, l: v } }],
          }
    )
  return (
    <>
      <span className="text-xs text-muted-foreground">
        {data.distinct.toLocaleString()} choices
        {data.distinct > SHOWN && ` · top ${SHOWN} shown`}
      </span>
      <ScrollArea className="max-h-64">
        <div className="flex flex-col">
          {data.choices.slice(0, SHOWN).map(([v, n]) => (
            <Button
              key={v}
              variant={selected(v) ? "secondary" : "ghost"}
              size="sm"
              className="justify-between"
              onClick={() => toggle(v)}
            >
              <span className="truncate">{v === "" ? "(blank)" : v}</span>
              <span className="tabular-nums text-muted-foreground">{n.toLocaleString()}</span>
            </Button>
          ))}
        </div>
      </ScrollArea>
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

export function History({ summary, onGoto }: { summary: Summary; onGoto: (i: number) => void }) {
  const steps = ["Create project", ...summary.history]
  return (
    <div className="flex flex-col gap-1">
      {steps.map((s, i) => (
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
        Click a step to undo or redo to it. Export → Recipe gives these steps as OpenRefine-compatible JSON.
      </p>
    </div>
  )
}
