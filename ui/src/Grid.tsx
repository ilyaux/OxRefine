import { useEffect, useLayoutEffect, useRef, useState } from "react"
import { ChevronDownIcon, FlagIcon, PencilIcon, StarIcon } from "lucide-react"
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
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { cn } from "cn"

/** [row, cells, starred, flagged, cell types (one letter each), record number when the row starts a record] */
type Row = [number, (string | null)[], boolean, boolean, string, (number | null)?]
const BLOCK = 200
const ROW_H = 33
// Browsers cap an element's height (~16-33M px). Past this the scrollbar maps onto the rows
// proportionally and the wheel is slowed down to match, so every row stays reachable.
const MAX_H = 8_000_000
const DEFAULT_W = 160
const TYPE_CLASS: Record<string, string> = {
  n: "text-right tabular-nums text-(--syntax-number)",
  d: "tabular-nums text-(--syntax-variable)",
  b: "text-(--syntax-function)",
  e: "text-destructive",
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
const PRESETS: [string, "list" | "range", string][] = [
  ["Word facet", "list", "value.split(' ')"],
  ["Duplicates facet", "list", "facetCount(value, 'value', $COL) > 1"],
  ["Text length facet", "range", "value.length()"],
  ["Unicode char-code facet", "range", "value.unicode()"],
  ["Facet by blank", "list", "isBlank(value)"],
  ["Facet by error", "list", "isError(value)"],
  ["Facet by star", "list", "row.starred"],
  ["Facet by flag", "list", "row.flagged"],
]

export type ColumnAction =
  | {
      kind:
        | "facet" | "text-filter" | "transform" | "add-column" | "cluster" | "rename" | "remove"
        | "fill-down" | "blank-down" | "mv-split" | "mv-join" | "split" | "sort" | "dedupe"
        | "move-first" | "move-left" | "move-right" | "move-last"
        | "numeric-facet" | "custom-list-facet" | "custom-range-facet"
        | "fetch" | "transpose-columns" | "transpose-rows" | "columnize"
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
}: {
  columns: string[]
  engine: Engine
  version: string
  onTotal: (n: number) => void
  onAction: (a: ColumnAction) => void
  onApply: (op: Op) => Promise<void>
}) {
  const scroller = useRef<HTMLDivElement>(null)
  const head = useRef<HTMLTableSectionElement>(null)
  const [view, setView] = useState({ top: 0, height: 600, head: 40 })
  const [total, setTotal] = useState(0)
  const [blocks, setBlocks] = useState(() => new Map<number, Row[]>())
  const [widths, setWidths] = useState<Record<string, number>>({})
  const [editing, setEditing] = useState<Editing | null>(null)
  const pending = useRef(new Set<number>())
  const current = useRef(version)
  const records = engine.mode === "record-based"

  const load = (b: number) => {
    if (pending.current.has(b)) return
    pending.current.add(b)
    const v = version
    call<{ total: number; rows: Row[] }>("rows", { engine, start: b * BLOCK, limit: BLOCK })
      .then(({ data }) => {
        if (current.current !== v) return
        setTotal(data.total)
        onTotal(data.total)
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
  return (
    <div ref={scroller} className="h-full overflow-auto [&_[data-slot=table-container]]:overflow-visible">
      <Table className="table-fixed" style={{ width: 72 + 64 + columns.reduce((s, c) => s + width(c), 0) }}>
        <colgroup>
          <col style={{ width: 72 }} />
          <col style={{ width: 64 }} />
          {columns.map((c) => (
            <col key={c} style={{ width: width(c) }} />
          ))}
        </colgroup>
        <TableHeader ref={head} className="sticky top-0 z-10 bg-background">
          <TableRow>
            <TableHead className="text-right text-muted-foreground">{records ? "record" : "#"}</TableHead>
            <TableHead className="text-muted-foreground">
              <span className="sr-only">Star and flag</span>
            </TableHead>
            {columns.map((c) => (
              <TableHead key={c} className="relative">
                <ColumnMenu column={c} onAction={onAction} />
                <div
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={`Resize ${c}`}
                  className="absolute inset-y-0 right-0 w-1.5 cursor-col-resize hover:bg-border"
                  onPointerDown={resize(c)}
                  onDoubleClick={() => setWidths(({ [c]: _, ...w }) => w)}
                />
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {topPad > 0 && <tr style={{ height: topPad }} />}
          {rows.map((i) => {
            const row = blocks.get(Math.floor(i / BLOCK))?.[i % BLOCK]
            const label = !row ? "" : records ? (row[5] == null ? "" : row[5] + 1) : row[0] + 1
            return (
              <TableRow key={i} style={{ height: ROW_H }}>
                <TableCell className="text-right font-mono text-xs text-muted-foreground tabular-nums">{label}</TableCell>
                <TableCell className="py-0">
                  {row && (
                    <span className="flex">
                      <Button variant="ghost" size="icon-xs" aria-label={row[2] ? "Unstar row" : "Star row"} aria-pressed={row[2]} onClick={() => annotate(row[0], { starred: !row[2] })}>
                        <StarIcon className={row[2] ? "fill-current" : "opacity-30"} />
                      </Button>
                      <Button variant="ghost" size="icon-xs" aria-label={row[3] ? "Unflag row" : "Flag row"} aria-pressed={row[3]} onClick={() => annotate(row[0], { flagged: !row[3] })}>
                        <FlagIcon className={row[3] ? "fill-current" : "opacity-30"} />
                      </Button>
                    </span>
                  )}
                </TableCell>
                {columns.map((c, j) => {
                  const text = row?.[1][j] ?? null
                  const type = row?.[4]?.[j] ?? "s"
                  return (
                    <TableCell key={c} className={cn("group/cell relative truncate", TYPE_CLASS[type])} title={text ?? undefined}>
                      {text}
                      {row && (
                        <Button
                          variant="secondary"
                          size="icon-xs"
                          aria-label={`Edit ${c} in row ${row[0] + 1}`}
                          className="absolute top-1/2 right-1 -translate-y-1/2 opacity-0 group-hover/cell:opacity-100 focus-visible:opacity-100"
                          onClick={(e) => setEditing({ row: row[0], column: c, value: text ?? "", type: EDIT_TYPE[type] ?? "text", anchor: e.currentTarget.parentElement! })}
                        >
                          <PencilIcon />
                        </Button>
                      )}
                    </TableCell>
                  )
                })}
              </TableRow>
            )
          })}
          {bottomPad > 0 && <tr style={{ height: bottomPad }} />}
        </TableBody>
      </Table>
      {editing && <CellEditor editing={editing} engine={engine} onApply={onApply} onClose={() => setEditing(null)} />}
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

function ColumnMenu({ column, onAction }: { column: string; onAction: (a: ColumnAction) => void }) {
  const act = (kind: Exclude<ColumnAction["kind"], "common" | "preset-facet">) => () => onAction({ kind, column })
  return (
    <DropdownMenu>
      <DropdownMenuTrigger render={<Button variant="ghost" size="sm" />}>
        {column}
        <ChevronDownIcon data-icon="inline-end" />
      </DropdownMenuTrigger>
      <DropdownMenuContent className="w-64">
        <DropdownMenuGroup>
          <DropdownMenuLabel>Filter</DropdownMenuLabel>
          <DropdownMenuItem onClick={act("facet")}>Text facet</DropdownMenuItem>
          <DropdownMenuItem onClick={act("numeric-facet")}>Numeric facet</DropdownMenuItem>
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
