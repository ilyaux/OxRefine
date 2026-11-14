import { useEffect, useLayoutEffect, useRef, useState } from "react"
import { ChevronDownIcon, CircleAlertIcon, CircleCheckIcon, CirclePlusIcon, FlagIcon, PencilIcon, StarIcon } from "lucide-react"
import { call, type Engine, type Op } from "@/api"
import { Button } from "@/components/ui/button"
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuGroup,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu"
import { Field, FieldLabel } from "@/components/ui/field"
import { Popover, PopoverContent } from "@/components/ui/popover"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { cn } from "cn"
import { ReconPanel, type Judging } from "@/ReconPanel"

/** Row, cells, stars/flags, types, record start, reconciliation, and optional fingerprint-variant columns. */
type Row = [number, (string | null)[], boolean, boolean, string, (number | null)?, Record<string, Recon>?, number[]?]
type ClusterHints = { rows: number; columns: { column: string; values: number; groups: number; cells: number }[] }

/** A reconciled cell, as the grid gets it: judgment, match, best candidates (with how many there are), error. */
type Candidate = { id: string; name: string; score: number }
type Recon = { j: "none" | "matched" | "new" | "error"; m: Candidate | null; c: Candidate[]; n: number; e: string | null }

const score = (c: Candidate) => (Math.round(c.score * 10) / 10).toString()

/** What a reconciled cell adds after its text: the match, how many candidates, or the error. */
function ReconMark({ r, text }: { r: Recon; text: string | null }) {
  if (r.j === "matched" && r.m)
    return (
      <span className="ml-1.5 inline-flex items-center gap-1 text-primary" title={`Matched: ${r.m.name} (${r.m.id})`}>
        <CircleCheckIcon aria-label="matched" className="size-3.5" />
        {r.m.name !== text && <span className="truncate">{r.m.name}</span>}
      </span>
    )
  if (r.j === "new")
    return (
      <span className="ml-1.5 inline-flex items-center gap-1 text-muted-foreground" title="Will be a new item">
        <CirclePlusIcon aria-label="new item" className="size-3.5" />
        new
      </span>
    )
  if (r.j === "error")
    return (
      <span className="ml-1.5 inline-flex items-center text-destructive" title={r.e ?? "Reconciliation failed"}>
        <CircleAlertIcon aria-label="reconciliation error" className="size-3.5" />
      </span>
    )
  return (
    <span className="ml-1.5 text-xs text-muted-foreground" title={r.c.map((c) => `${c.name} (${c.id}) ${score(c)}`).join("\n") || "No candidates"}>
      {r.n === 0 ? "no candidates" : r.n === 1 ? `1 candidate: ${r.c[0].name}` : `${r.n} candidates`}
    </span>
  )
}
const BLOCK = 200
const ROW_H = 30
// Browsers cap an element's height (~16-33M px). Past this the scrollbar maps onto the rows
// proportionally and the wheel is slowed down to match, so every row stays reachable.
const MAX_H = 8_000_000
const DEFAULT_W = 190
// numbers right-aligned, numbers, dates and booleans in mono; errors in the error colour
const TYPE_CLASS: Record<string, string> = {
  n: "text-right font-mono text-[12.5px]",
  d: "font-mono text-[12.5px]",
  b: "font-mono text-[12.5px]",
  e: "font-mono text-[12.5px] text-destructive",
}
const EDIT_TYPE: Record<string, string> = { n: "number", b: "boolean", d: "date" }
type Editing = { row: number; column: string; value: string; type: string; anchor: HTMLElement }

export const COMMON: [string, string][] = [
  ["Trim leading and trailing whitespace", "value.trim()"],
  ["Collapse consecutive whitespace", "value.replace(/\\s+/, ' ')"],
  ["To titlecase", "value.toTitlecase()"],
  ["To uppercase", "value.toUppercase()"],
  ["To lowercase", "value.toLowercase()"],
  ["To number", "value.toNumber()"],
  ["To date", "value.toDate()"],
  ["To text", "value.toString()"],
  ["To blank", "null"],
]

/** OpenRefine's "Customized facets", as [menu label, facet type, expression]. */
export const PRESETS: [string, "list" | "range", string][] = [
  ["Word facet", "list", "value.split(' ')"],
  ["Duplicates facet", "list", "facetCount(value, 'value', $COL) > 1"],
  ["Numeric log facet", "range", "value.log()"],
  ["1-bounded numeric log facet", "range", "log(max(1, value))"],
  ["Text length facet", "range", "value.length()"],
  ["Log of text length facet", "range", "value.length().log()"],
  ["Unicode char-code facet", "range", "value.unicode()"],
  ["Facet by null", "list", "isNull(value)"],
  ["Facet by empty string", "list", "isEmptyString(value)"],
  ["Facet by blank", "list", "isBlank(value)"],
  ["Facet by error", "list", "isError(value)"],
  ["Facet by star", "list", "row.starred"],
  ["Facet by flag", "list", "row.flagged"],
]

/** OpenRefine's Reconcile → Facets (the first two also open by themselves after reconciling). */
export const RECON_FACETS: [string, "list" | "range", string][] = [
  ["By judgment", "list", 'forNonBlank(cell.recon.judgment, v, v, if(isNonBlank(value), "(unreconciled)", "(blank)"))'],
  ["Best candidate's score", "range", "cell.recon.best.score"],
  ["Best candidate's type match", "list", 'forNonBlank(cell.recon.features.typeMatch, v, v, if(isNonBlank(value), if(cell.recon != null, "(no type)", "(unreconciled)"), "(blank)"))'],
  ["Best candidate's name match", "list", 'forNonBlank(cell.recon.features.nameMatch, v, v, if(isNonBlank(value), "(unreconciled)", "(blank)"))'],
  ["Best candidate's name edit distance", "range", "cell.recon.features.nameLevenshtein"],
  ["Best candidate's name word similarity", "range", "cell.recon.features.nameWordDistance"],
  ["Best candidate's types", "list", 'forNonBlank(cell.recon.best.type, v, v, if(isNonBlank(value), "(unreconciled)", "(blank)"))'],
  ["Judgment action", "list", "cell.recon.judgmentAction"],
  ["Judgment history entry", "list", "cell.recon.judgmentHistoryEntry"],
]

/** What a column holds (the engine's `profile`): counts, the main type, and a small distribution. */
type Profile = {
  type: "text" | "number" | "date" | "boolean" | "error" | "empty"
  distinct: number
  empty: number
  top?: [string, number][]
  others?: number
  unique?: boolean
  bins?: number[]
  min?: number | string | null
  max?: number | string | null
  mean?: number
}

const short = (n: number) => (Math.abs(n) >= 1e6 || (n !== 0 && Math.abs(n) < 1e-3) ? n.toExponential(2) : Number(n.toFixed(2)).toLocaleString())
const month = (d: unknown) => (typeof d === "string" ? d.slice(0, 7) : "")

/** The profile line under a column's name: counts or the range, in mono. */
function profileLine(p: Profile) {
  if (p.type === "number" && typeof p.min === "number" && typeof p.max === "number")
    return `${short(p.min)} – ${short(p.max)}${p.mean !== undefined ? ` · μ ${short(p.mean)}` : ""}${p.empty ? ` · ${p.empty.toLocaleString()} empty` : ""}`
  if (p.type === "date") return `${month(p.min)} – ${month(p.max)}${p.empty ? ` · ${p.empty.toLocaleString()} empty` : ""}`
  return `${p.distinct.toLocaleString()} distinct · ${p.empty.toLocaleString()} empty`
}

/** A 14 px picture of the column: top values as a segmented bar (a value its facet selects in blue), a
 * histogram for numbers and dates, a hatched strip when nearly every value is different. */
function ProfileChart({ p, selected }: { p: Profile; selected: Set<string> }) {
  if (p.unique)
    return <div aria-hidden="true" title="Nearly every value is different" className="h-3.5 bg-[repeating-linear-gradient(90deg,var(--muted-foreground)_0_1px,transparent_1px_4px)] opacity-60" />
  if (p.bins?.length) {
    const top = Math.max(1, ...p.bins)
    return (
      <div aria-hidden="true" className="flex h-3.5 items-end gap-px">
        {p.bins.map((b, i) => (
          <span key={i} className="flex-1 bg-chart-4" style={{ height: b ? `${Math.max(12, (b / top) * 100)}%` : 0 }} />
        ))}
      </div>
    )
  }
  if (!p.top?.length) return <div aria-hidden="true" className="h-3.5" />
  const total = p.top.reduce((s, [, n]) => s + n, 0) + (p.others ?? 0)
  const greys = ["bg-chart-5", "bg-chart-4", "bg-chart-3", "bg-chart-2", "bg-chart-1"]
  return (
    <div aria-hidden="true" className="flex h-3.5 gap-px" title={p.top.map(([v, n]) => `${v || "(blank)"}: ${n.toLocaleString()}`).join("\n")}>
      {p.top.map(([v, n], i) => (
        <span key={i} className={selected.has(v) ? "bg-data" : greys[i]} style={{ width: `${(n / total) * 100}%` }} />
      ))}
      {!!p.others && <span className="flex-1 bg-row-rule" />}
    </div>
  )
}

/** The grid's selected cell: its row, column and text (null for an empty cell). */
export type SelectedCell = { row: number; column: string; value: string | null }

export type ColumnAction =
  | {
      kind:
        | "facet" | "text-filter" | "transform" | "add-column" | "cluster" | "rename" | "remove"
        | "fill-down" | "blank-down" | "mv-split" | "mv-join" | "split" | "sort" | "dedupe"
        | "move-first" | "move-left" | "move-right" | "move-last"
        | "numeric-facet" | "timeline-facet" | "custom-list-facet" | "custom-range-facet"
        | "fetch" | "transpose-columns" | "transpose-rows" | "columnize" | "reconcile"
        | "recon-best" | "recon-new-each" | "recon-new-shared" | "recon-match-all" | "recon-discard" | "recon-clear"
        | "recon-copy" | "recon-use-ids" | "recon-add-ids" | "recon-extend"
      column: string
    }
  | { kind: "preset-facet"; column: string; facet: "list" | "range"; expression: string }
  | { kind: "common"; column: string; expression: string }

export function Grid({
  columns,
  engine,
  version,
  onTotal,
  onAction,
  onApply,
  onFocusColumn,
  onSelect,
  dataVersion,
}: {
  columns: string[]
  engine: Engine
  version: string
  /** how many rows match; in records mode also how many records match, of how many */
  onTotal: (rows: number, records?: [number, number]) => void
  onAction: (a: ColumnAction) => void
  onApply: (op: Op) => Promise<void>
  /** the column the user last worked on (its menu or a cell), for the command palette */
  onFocusColumn?: (column: string) => void
  /** the selected cell changed (null: none) */
  onSelect?: (cell: SelectedCell | null) => void
  /** changes when the data does (another step), not with the facets: the column profiles follow it */
  dataVersion?: string
}) {
  const scroller = useRef<HTMLDivElement>(null)
  const head = useRef<HTMLTableSectionElement>(null)
  const [view, setView] = useState({ top: 0, height: 600, head: 40 })
  const [total, setTotal] = useState(0)
  const [blocks, setBlocks] = useState(() => new Map<number, Row[]>())
  const [widths, setWidths] = useState<Record<string, number>>({})
  const [editing, setEditing] = useState<Editing | null>(null)
  const [judging, setJudging] = useState<Judging | null>(null)
  // the selected cell, by position in the view it was picked in: [row index, column index]; another view
  // (other filters, another step) has none
  const [picked, setPicked] = useState<{ version: string; at: [number, number] } | null>(null)
  const sel = picked?.version === version ? picked.at : null
  const setSel = (at: [number, number] | null) => setPicked(at && { version, at })
  const pending = useRef(new Set<number>())
  const current = useRef(version)
  const records = engine.mode === "record-based"
  // the column profiles of this version of the data, kept with the version they belong to
  const [profiles, setProfiles] = useState<{ key: string; data: Profile[] } | null>(null)
  useEffect(() => {
    if (dataVersion === undefined) return
    let live = true
    call<Profile[]>("profile", {}, { quiet: true }).then(({ data }) => live && setProfiles({ key: dataVersion, data }), () => {})
    return () => {
      live = false
    }
  }, [dataVersion])
  const profileOf = (j: number) => (profiles && profiles.key === dataVersion ? profiles.data[j] : undefined)
  const [clusterHints, setClusterHints] = useState<{ key: string; data: ClusterHints } | null>(null)
  useEffect(() => {
    let live = true
    call<ClusterHints>("cluster_hints", { engine }, { quiet: true }).then(({ data }) => live && setClusterHints({ key: version, data }), () => {})
    return () => { live = false }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version])
  const hints = clusterHints?.key === version ? clusterHints.data : undefined
  // values a list facet selects, per column, to show them in blue in the header's bar
  const facetPicks = (c: string) => new Set(engine.facets.flatMap((f) => (f.type === "list" && f.columnName === c && (f.expression === "value" || f.expression === "grel:value") ? f.selection.map((x) => x.v.l || String(x.v.v)) : [])))

  const load = (b: number) => {
    if (pending.current.has(b)) return
    pending.current.add(b)
    const v = version
    call<{ total: number; rows: Row[]; records?: number; recordTotal?: number }>("rows", { engine, start: b * BLOCK, limit: BLOCK, params: { clusterHints: true } })
      .then(({ data }) => {
        if (current.current !== v) return
        setTotal(data.total)
        onTotal(data.total, data.records === undefined ? undefined : [data.records, data.recordTotal ?? 0])
        setBlocks((m) => new Map(m).set(b, data.rows))
      })
      .catch(() => pending.current.delete(b))
  }

  useEffect(() => {
    current.current = version
    pending.current = new Set()
    setBlocks(new Map())
    load(0)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [version])

  // geometry: scrolled height is the real height, capped at MAX_H
  const real = total * ROW_H
  const height = Math.min(real, MAX_H)
  const viewH = Math.max(ROW_H, view.height - view.head)
  const ratio = real > height ? (real - viewH) / Math.max(1, height - viewH) : 1
  const virt = Math.min(view.top * ratio, Math.max(0, real - viewH))
  const first = Math.min(Math.floor(virt / ROW_H), Math.max(0, total - 1))
  const count = Math.min(total - first, Math.ceil(viewH / ROW_H) + 2)
  const topPad = Math.max(0, view.top - (virt - first * ROW_H))
  const bottomPad = Math.max(0, height - topPad - count * ROW_H)

  useLayoutEffect(() => {
    const el = scroller.current
    if (!el) return
    const measure = () => setView({ top: el.scrollTop, height: el.clientHeight, head: head.current?.offsetHeight ?? 40 })
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(el)
    el.addEventListener("scroll", measure, { passive: true })
    return () => {
      ro.disconnect()
      el.removeEventListener("scroll", measure)
    }
  }, [])

  // scaled scrolling: one wheel notch still moves by rows, not by `ratio` screens
  useEffect(() => {
    const el = scroller.current
    if (!el || ratio <= 1) return
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey || Math.abs(e.deltaY) < Math.abs(e.deltaX)) return
      e.preventDefault()
      el.scrollTop += (e.deltaMode === 1 ? e.deltaY * ROW_H : e.deltaMode === 2 ? e.deltaY * viewH : e.deltaY) / ratio
    }
    el.addEventListener("wheel", onWheel, { passive: false })
    return () => el.removeEventListener("wheel", onWheel)
  }, [ratio, viewH])

  useEffect(() => {
    for (let b = Math.floor(first / BLOCK); b <= Math.floor((first + count) / BLOCK); b++) if (!blocks.has(b)) load(b)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [first, count, blocks])

  const width = (c: string) => widths[c] ?? DEFAULT_W
  const resize = (c: string) => (e: React.PointerEvent) => {
    e.preventDefault()
    e.stopPropagation()
    const x0 = e.clientX
    const w0 = width(c)
    const move = (ev: PointerEvent) => setWidths((w) => ({ ...w, [c]: Math.max(48, w0 + ev.clientX - x0) }))
    const up = () => {
      window.removeEventListener("pointermove", move)
      window.removeEventListener("pointerup", up)
    }
    window.addEventListener("pointermove", move)
    window.addEventListener("pointerup", up)
  }
  const annotate = (row: number, what: { starred: boolean } | { flagged: boolean }) => onApply({ op: "ox/row-annotate", row, ...what }).catch(() => {})

  const rows = Array.from({ length: Math.max(0, count) }, (_, i) => first + i)
  const rowAt = (i: number) => blocks.get(Math.floor(i / BLOCK))?.[i % BLOCK]
  const selRow = sel ? rowAt(sel[0]) : undefined

  // the status bar follows the selection, and the data under it as pages load or change
  useEffect(() => {
    if (!sel || !selRow || !columns[sel[1]]) return onSelect?.(null)
    onSelect?.({ row: selRow[0], column: columns[sel[1]], value: selRow[1][sel[1]] ?? null })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sel, selRow, columns])

  const edit = (i: number, j: number) => {
    const row = rowAt(i)
    const cell = scroller.current?.querySelector<HTMLElement>(`[data-cell="${i}:${j}"]`)
    if (!row || !cell || !columns[j]) return
    onFocusColumn?.(columns[j])
    setEditing({ row: row[0], column: columns[j], value: row[1][j] ?? "", type: EDIT_TYPE[row[4]?.[j] ?? "s"] ?? "text", anchor: cell })
  }
  const onKeyDown = (e: React.KeyboardEvent) => {
    if (!sel || editing || judging || (e.target as HTMLElement).closest("input,textarea,button[aria-haspopup]")) return
    const [i, j] = sel
    const move = (di: number, dj: number) => {
      e.preventDefault()
      const ni = Math.max(0, Math.min(total - 1, i + di))
      const nj = Math.max(0, Math.min(columns.length - 1, j + dj))
      setSel([ni, nj])
      onFocusColumn?.(columns[nj])
      // keep the row in view: scroll by whole rows when it leaves the visible window
      const el = scroller.current
      if (el && (ni < first + 1 || ni > first + count - 3)) el.scrollTop += ((ni - i) * ROW_H) / ratio
    }
    if (e.key === "ArrowDown") move(1, 0)
    else if (e.key === "ArrowUp") move(-1, 0)
    else if (e.key === "ArrowRight" || (e.key === "Tab" && !e.shiftKey)) move(0, 1)
    else if (e.key === "ArrowLeft" || (e.key === "Tab" && e.shiftKey)) move(0, -1)
    else if (e.key === "Enter") {
      e.preventDefault()
      edit(i, j)
    } else if (e.key === "Escape") setSel(null)
  }

  return (
    <div ref={scroller} tabIndex={0} aria-label="Rows" onKeyDown={onKeyDown} className="h-full overflow-auto outline-none">
      <table className="table-fixed border-separate border-spacing-0" style={{ width: 56 + 52 + columns.reduce((s, c) => s + width(c), 0) }}>
        <colgroup>
          <col style={{ width: 56 }} />
          <col style={{ width: 52 }} />
          {columns.map((c) => (
            <col key={c} style={{ width: width(c) }} />
          ))}
        </colgroup>
        <thead ref={head} className="sticky top-0 z-10 bg-muted">
          <tr>
            <th scope="col" className="border-r border-b border-b-foreground px-2 py-2 text-right align-bottom font-mono text-[11.5px] font-normal text-muted-foreground">
              {records ? "rec" : "#"}
            </th>
            <th scope="col" className="border-r border-b border-b-foreground">
              <span className="sr-only">Star and flag</span>
            </th>
            {columns.map((c, j) => (
              <th key={c} scope="col" className="relative border-r border-b border-b-foreground p-0 text-left align-top font-normal" onPointerDownCapture={() => onFocusColumn?.(c)}>
                <ColumnMenu column={c} type={profileOf(j)?.type} onAction={onAction} />
                {profileOf(j) && (
                  <div className="flex flex-col gap-1.5 px-3 pb-2.5">
                    <span className="-mt-1.5 truncate font-mono text-[11px] text-muted-foreground" title={profileLine(profileOf(j)!)}>
                      {profileLine(profileOf(j)!)}
                    </span>
                    <ProfileChart p={profileOf(j)!} selected={facetPicks(c)} />
                  </div>
                )}
                {!!hints?.columns[j]?.values && (
                  <button
                    type="button"
                    className="mx-3 mb-2.5 block max-w-[calc(100%_-_1.5rem)] truncate text-left text-xs text-warn-text underline decoration-dotted underline-offset-4 hover:decoration-solid focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
                    aria-label={`Cluster ${hints.columns[j].values} fingerprint variants in ${hints.columns[j].groups} ${hints.columns[j].groups === 1 ? "group" : "groups"} in ${c}`}
                    title={`Review ${hints.columns[j].cells.toLocaleString()} text cells in ${hints.rows.toLocaleString()} matching rows with fingerprint clustering`}
                    onClick={() => onAction({ kind: "cluster", column: c })}
                  >
                    {hints.columns[j].values.toLocaleString()} variants · {hints.columns[j].groups.toLocaleString()} {hints.columns[j].groups === 1 ? "group" : "groups"}
                  </button>
                )}
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={`Resize ${c}`}
                  className="absolute inset-y-0 -right-px w-1.5 cursor-col-resize hover:bg-foreground/20"
                  onPointerDown={resize(c)}
                  onDoubleClick={() => setWidths(({ [c]: _, ...w }) => w)}
                />
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {topPad > 0 && <tr style={{ height: topPad }} />}
          {rows.map((i) => {
            const row = rowAt(i)
            const label = !row ? "" : records ? (row[5] == null ? "" : row[5] + 1) : row[0] + 1
            return (
              <tr key={i} style={{ height: ROW_H }} className="group/row">
                <td className="border-r border-b border-row-rule px-2 text-right font-mono text-[11.5px] text-muted-foreground">{label}</td>
                <td className="border-r border-b border-row-rule">
                  {row && (
                    <span className="flex justify-center">
                      <Button variant="ghost" size="icon-xs" aria-label={row[2] ? "Unstar row" : "Star row"} aria-pressed={row[2]} onClick={() => annotate(row[0], { starred: !row[2] })}>
                        <StarIcon className={row[2] ? "fill-current" : "opacity-25"} />
                      </Button>
                      <Button variant="ghost" size="icon-xs" aria-label={row[3] ? "Unflag row" : "Flag row"} aria-pressed={row[3]} onClick={() => annotate(row[0], { flagged: !row[3] })}>
                        <FlagIcon className={row[3] ? "fill-current" : "opacity-25"} />
                      </Button>
                    </span>
                  )}
                </td>
                {columns.map((c, j) => {
                  const text = row?.[1][j] ?? null
                  const type = row?.[4]?.[j] ?? "s"
                  const recon = row?.[6]?.[j]
                  const on = sel?.[0] === i && sel?.[1] === j
                  return (
                    <td
                      key={c}
                      data-cell={`${i}:${j}`}
                      aria-selected={on}
                      className={cn(
                        "group/cell relative truncate border-r border-b border-row-rule px-3",
                        TYPE_CLASS[type],
                        on && "outline-[1.5px] -outline-offset-1 outline-foreground outline-solid",
                      )}
                      title={recon ? undefined : (text ?? undefined)}
                      onClick={() => row && (setSel([i, j]), onFocusColumn?.(c))}
                      onDoubleClick={() => row && edit(i, j)}
                    >
                      {hints && type === "s" && row?.[7]?.includes(j) ? (
                        <span className="underline decoration-warn decoration-dotted underline-offset-4" title="Possible fingerprint variant. Review clustering from the column header.">
                          {text}
                        </span>
                      ) : text}
                      {recon && row && (
                        <Button
                          variant="ghost"
                          size="xs"
                          aria-label={`Judge ${c} in row ${row[0] + 1}`}
                          onClick={(e) => (onFocusColumn?.(c), setJudging({ row: row[0], column: c, value: text ?? "", anchor: e.currentTarget.parentElement! }))}
                        >
                          <ReconMark r={recon} text={text} />
                        </Button>
                      )}
                      {row && (
                        <Button
                          variant="outline"
                          size="icon-xs"
                          aria-label={`Edit ${c} in row ${row[0] + 1}`}
                          className="absolute top-1/2 right-1 -translate-y-1/2 bg-background opacity-0 group-hover/cell:opacity-100 focus-visible:opacity-100"
                          onClick={(e) => (e.stopPropagation(), setSel([i, j]), edit(i, j))}
                        >
                          <PencilIcon />
                        </Button>
                      )}
                    </td>
                  )
                })}
              </tr>
            )
          })}
          {bottomPad > 0 && <tr style={{ height: bottomPad }} />}
        </tbody>
      </table>
      {editing && <CellEditor editing={editing} engine={engine} onApply={onApply} onClose={() => setEditing(null)} />}
      {judging && <ReconPanel judging={judging} engine={engine} onApply={onApply} onClose={() => setJudging(null)} />}
    </div>
  )
}

/** OpenRefine's cell editor: a value, its type, and "apply to all identical cells" (a mass edit). */
function CellEditor({ editing, engine, onApply, onClose }: { editing: Editing; engine: Engine; onApply: (op: Op) => Promise<void>; onClose: () => void }) {
  const [value, setValue] = useState(editing.value)
  const [type, setType] = useState(editing.type)
  const one = () => onApply({ op: "ox/cell-edit", row: editing.row, columnName: editing.column, value, type }).then(onClose, () => {})
  const all = () =>
    onApply({
      op: "core/mass-edit",
      engineConfig: engine,
      columnName: editing.column,
      expression: "value",
      edits: [{ from: [editing.value], fromBlank: editing.value === "", fromError: false, to: value }],
    }).then(onClose, () => {})
  return (
    <Popover open onOpenChange={(o) => !o && onClose()}>
      <PopoverContent anchor={editing.anchor} align="start" className="w-80">
        <Field>
          <FieldLabel htmlFor="cell-edit">
            {editing.column}, row {editing.row + 1}
          </FieldLabel>
          <Textarea
            id="cell-edit"
            className="font-mono"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) one()
            }}
            autoFocus
          />
        </Field>
        <ToggleGroup size="sm" variant="outline" value={[type]} onValueChange={(v) => v[0] && setType(v[0])} aria-label="Cell type">
          <ToggleGroupItem value="text">Text</ToggleGroupItem>
          <ToggleGroupItem value="number">Number</ToggleGroupItem>
          <ToggleGroupItem value="boolean">Boolean</ToggleGroupItem>
          <ToggleGroupItem value="date">Date</ToggleGroupItem>
        </ToggleGroup>
        <div className="flex justify-end gap-2">
          {type === "text" && (
            <Button variant="outline" size="sm" onClick={all}>
              Apply to all identical cells
            </Button>
          )}
          <Button size="sm" onClick={one}>
            Apply
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}

function ColumnMenu({ column, type, onAction }: { column: string; type?: string; onAction: (a: ColumnAction) => void }) {
  const act = (kind: Exclude<ColumnAction["kind"], "common" | "preset-facet">) => () => onAction({ kind, column })
  return (
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <button
            type="button"
            aria-label={`${column}: column menu`}
            className="flex w-full items-center gap-1.5 px-3 py-2.5 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
          />
        }
      >
        <span className="min-w-0 truncate font-semibold">{column}</span>
        {type && type !== "empty" && <span className="shrink-0 font-mono text-[11px] text-muted-foreground">{type}</span>}
        <span className="flex-1" />
        <ChevronDownIcon className="size-3.5 shrink-0 text-muted-foreground" />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-64">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Filter</DropdownMenuLabel>
          <DropdownMenuItem onClick={act("facet")}>Text facet</DropdownMenuItem>
          <DropdownMenuItem onClick={act("numeric-facet")}>Numeric facet</DropdownMenuItem>
          <DropdownMenuItem onClick={act("timeline-facet")}>Timeline facet</DropdownMenuItem>
          <DropdownMenuItem onClick={act("text-filter")}>Text filter</DropdownMenuItem>
          <DropdownMenuItem onClick={act("custom-list-facet")}>Custom text facet…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("custom-range-facet")}>Custom numeric facet…</DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Customized facets</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-56">
              <DropdownMenuGroup>
                {PRESETS.map(([label, facet, expression]) => (
                  <DropdownMenuItem key={label} onClick={() => onAction({ kind: "preset-facet", column, facet, expression: expression.replace("$COL", JSON.stringify(column)) })}>
                    {label}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuLabel>Edit cells</DropdownMenuLabel>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Common transforms</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-64">
              <DropdownMenuGroup>
                {COMMON.map(([label, expression]) => (
                  <DropdownMenuItem key={label} onClick={() => onAction({ kind: "common", column, expression })}>
                    {label}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem onClick={act("transform")}>Transform…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("fill-down")}>Fill down</DropdownMenuItem>
          <DropdownMenuItem onClick={act("blank-down")}>Blank down</DropdownMenuItem>
          <DropdownMenuItem onClick={act("mv-split")}>Split multi-valued cells…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("mv-join")}>Join multi-valued cells…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("cluster")}>Cluster and edit…</DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuLabel>Edit column</DropdownMenuLabel>
          <DropdownMenuItem onClick={act("split")}>Split into several columns…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("add-column")}>Add column based on this…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("fetch")}>Add column by fetching URLs…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("recon-extend")}>Add columns from reconciled values…</DropdownMenuItem>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Reconcile</DropdownMenuSubTrigger>
            <DropdownMenuSubContent className="w-56">
              <DropdownMenuGroup>
                <DropdownMenuItem onClick={act("reconcile")}>Start reconciling…</DropdownMenuItem>
              </DropdownMenuGroup>
              <DropdownMenuSeparator />
              <DropdownMenuGroup>
                <DropdownMenuLabel>Actions</DropdownMenuLabel>
                <DropdownMenuItem onClick={act("recon-best")}>Match each cell to its best candidate</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-new-each")}>Create a new item for each cell</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-new-shared")}>Create one new item for similar cells</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-match-all")}>Match all filtered cells to…</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-discard")}>Discard reconciliation judgments</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-clear")}>Clear reconciliation data</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-copy")}>Copy reconciliation data to other columns…</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-use-ids")}>Use values as identifiers…</DropdownMenuItem>
                <DropdownMenuItem onClick={act("recon-add-ids")}>Add entity identifiers column…</DropdownMenuItem>
              </DropdownMenuGroup>
              <DropdownMenuSeparator />
              <DropdownMenuGroup>
                <DropdownMenuLabel>Facets</DropdownMenuLabel>
                {RECON_FACETS.map(([label, facet, expression]) => (
                  <DropdownMenuItem key={label} onClick={() => onAction({ kind: "preset-facet", column, facet, expression })}>
                    {label}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuSub>
            <DropdownMenuSubTrigger>Move column</DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              <DropdownMenuGroup>
                <DropdownMenuItem onClick={act("move-first")}>To the beginning</DropdownMenuItem>
                <DropdownMenuItem onClick={act("move-left")}>Left</DropdownMenuItem>
                <DropdownMenuItem onClick={act("move-right")}>Right</DropdownMenuItem>
                <DropdownMenuItem onClick={act("move-last")}>To the end</DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
          <DropdownMenuItem onClick={act("rename")}>Rename…</DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuLabel>Transpose</DropdownMenuLabel>
          <DropdownMenuItem onClick={act("transpose-columns")}>Cells across columns into rows…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("transpose-rows")}>Cells in rows into columns…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("columnize")}>Columnize by key/value columns…</DropdownMenuItem>
        </DropdownMenuGroup>
        <DropdownMenuSeparator />
        <DropdownMenuGroup>
          <DropdownMenuLabel>Rows</DropdownMenuLabel>
          <DropdownMenuItem onClick={act("sort")}>Sort rows by this column…</DropdownMenuItem>
          <DropdownMenuItem onClick={act("dedupe")}>Remove duplicate rows by this column</DropdownMenuItem>
          <DropdownMenuItem variant="destructive" onClick={act("remove")}>
            Remove column
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  )
}
