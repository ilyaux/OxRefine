import { useEffect, useId, useState } from "react"
import { ChevronDownIcon, FlipHorizontal2Icon, PencilIcon, RotateCcwIcon, XIcon } from "lucide-react"
import { rangeFacet, timeFacet, type Facet, type FacetData, type ListData, type ListFacet, type RangeData, type RangeFacet, type Summary, type TextFacet, type TimeData, type TimeFacet } from "@/api"
import { Button } from "@/components/ui/button"
import { DropdownMenu, DropdownMenuContent, DropdownMenuGroup, DropdownMenuItem, DropdownMenuTrigger } from "@/components/ui/dropdown-menu"
import { Input } from "@/components/ui/input"
import { cn, maxOf } from "@/lib/utils"

const SHOWN = 200
const fmt = (n: number) => (Math.abs(n) >= 1e6 || (n !== 0 && Math.abs(n) < 1e-3) ? n.toExponential(2) : Number(n.toFixed(6)).toLocaleString())

/** A square checkbox mark, filled in the data colour when on (the row itself is the control). */
const Square = ({ on }: { on: boolean }) => <span aria-hidden="true" className={cn("size-2.5 shrink-0 border", on ? "border-data bg-data" : "border-faint")} />

type FacetKeys = { facets: Facet[]; entries: { facet: Facet; identity: string; key: number }[]; next: number }
function trackFacets(facets: Facet[], previous?: FacetKeys): FacetKeys {
  const used = new Set<FacetKeys["entries"][number]>()
  // Reserve unchanged duplicates before matching facets replaced by an immutable update.
  const exact = facets.map((facet) => {
    const entry = previous?.entries.find((entry) => entry.facet === facet && !used.has(entry))
    if (entry) used.add(entry)
    return entry
  })
  let next = previous?.next ?? 0
  const entries = facets.map((facet, i) => {
    const identity = facetKey(facets, i)
    const entry = exact[i] ?? previous?.entries.find((entry) => entry.identity === identity && !used.has(entry))
    if (entry) used.add(entry)
    return { facet, identity, key: entry?.key ?? next++ }
  })
  return { facets, entries, next }
}

/** The facet rail: one ruled section per facet, flat. */
export function Facets({
  facets,
  data,
  columns,
  onChange,
  onEditChoice,
  onLimit,
  onAdd,
}: {
  facets: Facet[]
  data: (FacetData | null)[]
  columns: string[]
  onChange: (f: Facet[]) => void
  onEditChoice: (facet: ListFacet, choice: string) => void
  /** raise the facet choice limit (a preference) */
  onLimit: (n: number) => void
  /** add a text facet on a column */
  onAdd: (column: string) => void
}) {
  const [tracked, setTracked] = useState(() => trackFacets(facets))
  let keyed = tracked
  if (tracked.facets !== facets) {
    keyed = trackFacets(facets, tracked)
    setTracked(keyed)
  }
  const update = (i: number, f: Facet) => onChange(facets.map((x, j) => (j === i ? f : x)))
  return (
    <div className="flex flex-col">
      {keyed.entries.map(({ facet, key }, i) => (
        <FacetSection key={key} facet={facet} data={data[i]} onChange={(f) => update(i, f)} onRemove={() => onChange(facets.filter((_, j) => j !== i))} onEditChoice={onEditChoice} onLimit={onLimit} />
      ))}
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <button
              type="button"
              className="border-b px-3.5 py-3 text-left text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
            />
          }
        >
          + Facet on a column…
        </DropdownMenuTrigger>
        <DropdownMenuContent align="start" className="max-h-80 w-56">
          <DropdownMenuGroup>
            {columns.map((c) => (
              <DropdownMenuItem key={c} onClick={() => onAdd(c)}>
                {c}
              </DropdownMenuItem>
            ))}
          </DropdownMenuGroup>
        </DropdownMenuContent>
      </DropdownMenu>
      {!facets.length && (
        <p className="px-3.5 py-3 text-xs text-muted-foreground">
          Facets slice the rows by a column's values. Pick a column above, use a column's menu, or press <kbd>F</kbd> on the column you last used.
        </p>
      )}
    </div>
  )
}

function FacetSection({ facet: f, data, onChange, onRemove, onEditChoice, onLimit }: {
  facet: Facet
  data: FacetData | undefined
  onChange: (f: Facet) => void
  onRemove: () => void
  onEditChoice: (facet: ListFacet, choice: string) => void
  onLimit: (n: number) => void
}) {
  const [collapsed, setCollapsed] = useState(false)
  const [resetVersion, setResetVersion] = useState(0)
  const bodyId = useId()
  const reset = () => {
    if (f.type !== "range" && f.type !== "timerange") return
    onChange({ ...(f.type === "range" ? rangeFacet(f.columnName, f.expression) : timeFacet(f.columnName, f.expression)), name: f.name })
    setResetVersion((n) => n + 1)
  }
  return (
    <section aria-label={`Facet ${f.name}`} className={cn("flex flex-col border-b", !collapsed && "pb-3")}>
      <div className="flex items-center gap-1 py-2.5 pr-1.5 pl-3.5">
        <button type="button" aria-label={`${collapsed ? "Expand" : "Collapse"} the ${f.name} facet`} aria-expanded={!collapsed} aria-controls={bodyId} onClick={() => setCollapsed(!collapsed)} className="flex min-w-0 flex-1 items-center gap-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
          <ChevronDownIcon aria-hidden="true" className={cn("size-3 shrink-0", collapsed && "-rotate-90")} />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate font-semibold" title={f.name}>{f.name}</span>
            {f.type !== "text" && f.expression !== "value" && f.expression !== "grel:value" && (
              <span className="truncate font-mono text-[11px] text-muted-foreground" title={f.expression}>
                {f.expression.replace(/^grel:/, "")}
              </span>
            )}
          </span>
        </button>
        <FacetCount facet={f} data={data} />
        {f.type === "list" || f.type === "text" ? (
          <Button variant="ghost" size="icon-xs" aria-label={f.invert ? "Stop inverting the facet" : "Invert facet"} aria-pressed={f.invert} onClick={() => onChange({ ...f, invert: !f.invert })}>
            <FlipHorizontal2Icon className={f.invert ? "text-data" : undefined} />
          </Button>
        ) : (
          <Button variant="ghost" size="icon-xs" aria-label={`Reset the ${f.name} facet`} onClick={reset}><RotateCcwIcon /></Button>
        )}
        <Button variant="ghost" size="icon-xs" aria-label={`Remove the ${f.name} facet`} onClick={onRemove}><XIcon /></Button>
      </div>
      <div id={bodyId} hidden={collapsed} key={resetVersion}>
        {f.type === "list" && (data as ListData | null)?.tooMany ? (
          <TooMany data={data as ListData} onLimit={onLimit} />
        ) : (
          f.type === "list" && <ListChoices facet={f} data={data as ListData | null} onChange={onChange} onEdit={(c) => onEditChoice(f, c)} />
        )}
        {f.type === "text" && <TextQuery facet={f} onChange={onChange} />}
        {f.type === "range" && <RangePanel facet={f} data={data as RangeData | null} onChange={onChange} />}
        {f.type === "timerange" && <TimePanel facet={f} data={data as TimeData | null} onChange={onChange} />}
      </div>
    </section>
  )
}

/** A facet's identity: type, column and expression, plus how many identical facets come before it. */
const facetKey = (facets: Facet[], i: number) => {
  const id = (f: Facet) => JSON.stringify([f.type, f.columnName, "expression" in f ? f.expression : ""])
  return `${id(facets[i])}:${facets.slice(0, i).filter((f) => id(f) === id(facets[i])).length}`
}

/** The count next to a facet's name: how many choices, or the range it covers. */
function FacetCount({ facet, data }: { facet: Facet; data: FacetData | undefined }) {
  if (!data) return null
  let text: string | null = null
  if (facet.type === "list" && "distinct" in data) text = `${data.distinct.toLocaleString()} values`
  if (facet.type === "range" && "min" in data && data.min !== undefined && data.max !== undefined) {
    const lo = Math.max(facet.from, data.min)
    const hi = Math.min(facet.to, data.max)
    text = `${fmt(lo)} – ${fmt(hi)}`
  }
  return text ? <span className="shrink-0 font-mono text-[11.5px] text-muted-foreground">{text}</span> : null
}

/** OpenRefine's "Too many choices": past the facet choice limit nothing is listed. */
function TooMany({ data, onLimit }: { data: ListData; onLimit: (n: number) => void }) {
  return (
    <div className="flex flex-col gap-2 px-3.5">
      <p className="text-xs text-muted-foreground">
        <span className="font-mono text-foreground">{data.distinct.toLocaleString()}</span> choices, more than the limit of{" "}
        <span className="font-mono">{data.limit?.toLocaleString()}</span>, so they aren't listed. Showing many choices can be slow.
      </p>
      <div>
        <Button variant="outline" size="sm" onClick={() => onLimit(data.distinct)}>
          Show all
        </Button>
      </div>
    </div>
  )
}

function ListChoices({ facet, data, onChange, onEdit }: { facet: ListFacet; data: ListData | null; onChange: (f: ListFacet) => void; onEdit: (choice: string) => void }) {
  const [order, setOrder] = useState<"count" | "name">("count")
  if (!data || !("choices" in data)) return null
  const selected = (label: string) => facet.selection.some((s) => (s.v.l || String(s.v.v)) === label)
  const toggle = (label: string, v: unknown) =>
    onChange({
      ...facet,
      selection: selected(label) ? facet.selection.filter((s) => (s.v.l || String(s.v.v)) !== label) : [...facet.selection, { v: { v, l: label } }],
    })
  const choices = order === "name" ? [...data.choices].sort((a, b) => a[0].localeCompare(b[0])) : data.choices
  const top = maxOf([...data.choices.map((c) => c[1]), data.blank, data.error], 1)
  const editable = facet.expression === "value" || facet.expression === "grel:value"
  const special: [string, number, boolean, () => void][] = [
    ["(blank)", data.blank, facet.selectBlank, () => onChange({ ...facet, selectBlank: !facet.selectBlank })],
    ["(error)", data.error, facet.selectError, () => onChange({ ...facet, selectError: !facet.selectError })],
  ]
  const row = (key: string, label: string, n: number, on: boolean, flip: () => void, italic?: boolean, edit?: () => void) => (
    <li key={key} className="group/choice relative flex items-stretch">
      {/* the bar behind the label: this choice's share of the largest count */}
      <span aria-hidden="true" className={cn("absolute inset-y-0 left-0", on ? "bg-data/12" : "bg-row-rule")} style={{ width: `${(n / top) * 100}%` }} />
      <button
        type="button"
        role="checkbox"
        aria-checked={on}
        onClick={flip}
        className="relative flex min-w-0 flex-1 items-center gap-2.5 py-1 pr-3.5 pl-3.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        <Square on={on} />
        <span className={cn("min-w-0 flex-1 truncate", italic && "text-muted-foreground italic", on && "font-medium")}>{label}</span>
        <span className={cn("font-mono text-[12px]", on ? "font-semibold" : "text-muted-foreground")}>{n.toLocaleString()}</span>
      </button>
      {edit && (
        <button
          type="button"
          aria-label={`Edit ${label}`}
          onClick={edit}
          className="relative flex w-7 items-center justify-center text-muted-foreground opacity-0 outline-none group-hover/choice:opacity-100 hover:text-foreground focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-ring"
        >
          <PencilIcon className="size-3" />
        </button>
      )}
    </li>
  )
  return (
    <>
      <div className="flex items-center justify-between gap-2 px-3.5 pb-1.5 text-[11.5px] text-muted-foreground">
        <span>{data.distinct > SHOWN ? `top ${SHOWN} shown` : "sort by"}</span>
        <div role="group" aria-label="Sort choices" className="flex">
          {(["count", "name"] as const).map((o) => (
            <button
              key={o}
              type="button"
              aria-pressed={order === o}
              onClick={() => setOrder(o)}
              className={cn("border border-input px-2 leading-5 first:rounded-l-[3px] last:-ml-px last:rounded-r-[3px] outline-none focus-visible:ring-2 focus-visible:ring-ring", order === o ? "bg-secondary text-foreground" : "hover:bg-accent")}
            >
              {o === "count" ? "Count" : "Name"}
            </button>
          ))}
        </div>
      </div>
      <ul className="max-h-72 overflow-y-auto">
        {choices.slice(0, SHOWN).map(([label, n, v]) => row(label, label, n, selected(label), () => toggle(label, v), false, editable ? () => onEdit(label) : undefined))}
        {special.filter(([, n, on]) => n > 0 || on).map(([label, n, on, flip]) => row(label, label, n, on, flip, true))}
      </ul>
    </>
  )
}

/** A histogram over an ink baseline: counts inside the facet's range in the data colour, the rest grey. */
function Histogram({ label, bins, baseBins, inside, ticks }: { label: string; bins: number[]; baseBins: number[]; inside: (i: number) => boolean; ticks: string[] }) {
  const top = maxOf(baseBins, 1)
  const w = 100 / baseBins.length
  const gap = Math.min(0.3, w / 4)
  return (
    <div className="px-3.5">
      <svg viewBox="0 0 100 40" preserveAspectRatio="none" className="block h-12 w-full" role="img" aria-label={label}>
        {baseBins.map((c, i) => (
          <g key={i}>
            <rect x={i * w + gap / 2} y={40 - (c / top) * 40} width={w - gap} height={(c / top) * 40} className="fill-row-rule" />
            <rect x={i * w + gap / 2} y={40 - (bins[i] / top) * 40} width={w - gap} height={(bins[i] / top) * 40} className={inside(i) ? "fill-data" : "fill-faint"} />
          </g>
        ))}
      </svg>
      <div className="h-px bg-foreground" />
      <div className="flex justify-between pt-1 font-mono text-[11px] text-muted-foreground">
        {ticks.map((t, i) => (
          <span key={i}>{t}</span>
        ))}
      </div>
    </div>
  )
}

/** A from/to pair of mono inputs that commit on Enter or when leaving the field. */
function Bounds({ label, lo, hi, show, parse, onCommit }: { label: string; lo: number; hi: number; show: (n: number) => string; parse: (s: string) => number; onCommit: (lo: number, hi: number) => void }) {
  const [a, setA] = useState(show(lo))
  const [b, setB] = useState(show(hi))
  const commit = () => {
    const x = parse(a)
    const y = parse(b)
    if (Number.isFinite(x) && Number.isFinite(y) && x <= y && (x !== lo || y !== hi)) onCommit(x, y)
  }
  const box = (which: "from" | "to", v: string, set: (s: string) => void) => (
    <label className="flex h-7.5 min-w-0 flex-1 items-center gap-2 rounded-[3px] border border-input px-2 focus-within:ring-2 focus-within:ring-ring">
      <span className="text-[11.5px] text-muted-foreground">{which}</span>
      <input
        aria-label={`${label}: ${which}`}
        value={v}
        onChange={(e) => set(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
        className="min-w-0 flex-1 bg-transparent text-right font-mono text-[12.5px] outline-none"
      />
    </label>
  )
  return (
    <div className="flex gap-2 px-3.5 pt-3">
      {box("from", a, setA)}
      {box("to", b, setB)}
    </div>
  )
}

/** The kinds of values a range or timeline facet lets through, as square checkboxes with counts. */
function Kinds<F extends RangeFacet | TimeFacet>({ facet, kinds, onChange }: { facet: F; kinds: [keyof F, string, number][]; onChange: (f: F) => void }) {
  return (
    <ul className="pt-2">
      {kinds.map(([k, label, n]) => {
        const on = Boolean(facet[k])
        return (
          <li key={String(k)}>
            <button
              type="button"
              role="checkbox"
              aria-checked={on}
              onClick={() => onChange({ ...facet, [k]: !on })}
              className="flex w-full items-center gap-2.5 px-3.5 py-0.5 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
            >
              <Square on={on} />
              <span className="flex-1">{label}</span>
              <span className="font-mono text-[12px] text-muted-foreground">{n.toLocaleString()}</span>
            </button>
          </li>
        )
      })}
    </ul>
  )
}

function RangePanel({ facet, data, onChange }: { facet: RangeFacet; data: RangeData | null; onChange: (f: RangeFacet) => void }) {
  if (!data) return null
  const kinds: [keyof RangeFacet, string, number][] = [
    ["selectNumeric", "Numeric", data.numericCount],
    ["selectNonNumeric", "Non-numeric", data.nonNumericCount],
    ["selectBlank", "Blank", data.blankCount],
    ["selectError", "Error", data.errorCount],
  ]
  if (data.error || data.min === undefined || data.max === undefined || !data.bins || !data.baseBins) {
    return (
      <>
        <p className="px-3.5 text-xs text-muted-foreground">
          {data.error ?? "No numbers to chart."} If the column holds numbers as text, convert it with Edit cells → Common transforms → To number, or add a custom numeric facet on value.toNumber().
        </p>
        <Kinds facet={facet} kinds={kinds} onChange={onChange} />
      </>
    )
  }
  const { min, max, step = 1, bins, baseBins } = data
  const lo = Math.max(facet.from, min)
  const hi = Math.min(facet.to, max)
  return (
    <>
      <Histogram
        label={`Histogram of ${facet.columnName}`}
        bins={bins}
        baseBins={baseBins}
        inside={(i) => min + i * step + step > lo && min + i * step < hi}
        ticks={[fmt(min), fmt((min + max) / 2), fmt(max)]}
      />
      <Bounds key={`${lo}:${hi}`} label={facet.columnName} lo={lo} hi={hi} show={(n) => String(Number(n.toFixed(6)))} parse={Number} onCommit={(from, to) => onChange({ ...facet, from, to })} />
      <Kinds facet={facet} kinds={kinds} onChange={onChange} />
    </>
  )
}

/** A date as the timeline shows it: the day, plus the time when bins are shorter than a day. */
const day = (ms: number, step: number) => {
  const iso = new Date(ms).toISOString()
  return step < 86_400_000 ? iso.slice(0, 16).replace("T", " ") : iso.slice(0, 10)
}

function TimePanel({ facet, data, onChange }: { facet: TimeFacet; data: TimeData | null; onChange: (f: TimeFacet) => void }) {
  if (!data) return null
  const kinds: [keyof TimeFacet, string, number][] = [
    ["selectTime", "Dates", data.timeCount],
    ["selectNonTime", "Not dates", data.nonTimeCount],
    ["selectBlank", "Blank", data.blankCount],
    ["selectError", "Error", data.errorCount],
  ]
  // OpenRefine sends max = 1 when there are fewer than two distinct dates
  if (data.baseTimeCount === 0 || data.max <= data.min) {
    return (
      <>
        <p className="px-3.5 text-xs text-muted-foreground">
          {data.baseTimeCount === 0
            ? "No dates to chart. If the column holds dates as text, convert it with Edit cells → Common transforms → To date, or facet on value.toDate()."
            : `Every date is ${day(data.min, 86_400_000)}.`}
        </p>
        <Kinds facet={facet} kinds={kinds} onChange={onChange} />
      </>
    )
  }
  const { min, max, step, bins, baseBins } = data
  const lo = Math.max(facet.from, min)
  const hi = Math.min(facet.to, max)
  return (
    <>
      <Histogram
        label={`Timeline of ${facet.columnName}`}
        bins={bins}
        baseBins={baseBins}
        inside={(i) => min + i * step + step > lo && min + i * step <= hi}
        ticks={[day(min, step), day(max, step)]}
      />
      <Bounds key={`${lo}:${hi}`} label={facet.columnName} lo={lo} hi={hi} show={(n) => day(n, step)} parse={(s) => Date.parse(s.length <= 10 ? `${s}T00:00:00Z` : `${s.replace(" ", "T")}Z`)} onCommit={(from, to) => onChange({ ...facet, from, to })} />
      <Kinds facet={facet} kinds={kinds} onChange={onChange} />
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
    <div className="px-3.5">
      <Input aria-label={`Filter ${facet.columnName}`} placeholder="Contains…" value={q} onChange={(e) => setQ(e.target.value)} className="h-7.5 font-mono" autoFocus />
    </div>
  )
}

/** The history rail: every step, the current one marked, undone ones muted; click to go there. */
export function History({ summary, onGoto, onExtract }: { summary: Summary; onGoto: (i: number) => void; onExtract: () => void }) {
  const [q, setQ] = useState("")
  const steps = ["Create project", ...summary.history]
  const shown = steps.map((s, i) => [s, i] as const).filter(([s]) => !q || s.toLowerCase().includes(q.toLowerCase()))
  return (
    <div className="flex flex-col">
      <div className="flex gap-2 border-b p-3">
        <Input aria-label="Filter steps" placeholder="Filter steps" value={q} onChange={(e) => setQ(e.target.value)} className="h-7.5" />
        <Button variant="outline" size="sm" disabled={!summary.cursor} onClick={onExtract}>
          Extract…
        </Button>
      </div>
      {q && !shown.length && <p className="px-3.5 py-3 text-xs text-muted-foreground">No steps match.</p>}
      <ol>
        {shown.map(([s, i]) => (
          <li key={i}>
            <button
              type="button"
              aria-current={i === summary.cursor ? "step" : undefined}
              onClick={() => onGoto(i)}
              className={cn(
                "flex w-full items-baseline gap-2.5 border-b border-row-rule px-3.5 py-2 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                i === summary.cursor && "bg-secondary font-medium",
                i > summary.cursor && "text-muted-foreground",
              )}
            >
              <span className="w-6 shrink-0 text-right font-mono text-[11.5px] text-muted-foreground">{i}</span>
              <span className="min-w-0 flex-1">{s}</span>
              {i > summary.cursor && <span className="font-mono text-[11px]">undone</span>}
              {i === summary.cursor && <span className="font-mono text-[11px] text-data">now</span>}
            </button>
          </li>
        ))}
      </ol>
      <p className="px-3.5 py-3 text-xs text-muted-foreground">Click a step to undo or redo to it. Extract gives the steps you pick as OpenRefine-compatible JSON.</p>
    </div>
  )
}
