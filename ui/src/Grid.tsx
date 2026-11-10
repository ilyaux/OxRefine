import { useEffect, useRef, useState } from "react"
import { useVirtualizer } from "@tanstack/react-virtual"
import { ChevronDownIcon, FlagIcon, StarIcon } from "lucide-react"
import { call, type Engine } from "@/api"
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
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

type Row = [number, (string | null)[], boolean, boolean]
const BLOCK = 200
const ROW_H = 33
// ponytail: native scroll height caps near ~1M rows at 33px (browser max element height);
// switch to scaled scroll offsets when bigger files need to scroll end-to-end.

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
}: {
  columns: string[]
  engine: Engine
  version: string
  onTotal: (n: number) => void
  onAction: (a: ColumnAction) => void
}) {
  const outer = useRef<HTMLDivElement>(null)
  const [scrollEl, setScrollEl] = useState<HTMLElement | null>(null)
  const [total, setTotal] = useState(0)
  const [blocks, setBlocks] = useState(() => new Map<number, Row[]>())
  const pending = useRef(new Set<number>())
  const current = useRef(version)

  // The shadcn Table wraps itself in a scroll container; we virtualize that element.
  useEffect(() => setScrollEl(outer.current?.querySelector<HTMLElement>("[data-slot=table-container]") ?? null), [])

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

  const v = useVirtualizer({
    count: total,
    getScrollElement: () => scrollEl,
    estimateSize: () => ROW_H,
    overscan: 30,
  })
  const items = v.getVirtualItems()
  const first = items[0]?.index ?? 0
  const last = items[items.length - 1]?.index ?? 0

  useEffect(() => {
    for (let b = Math.floor(first / BLOCK); b <= Math.floor(last / BLOCK); b++) if (!blocks.has(b)) load(b)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [first, last, blocks])

  const top = items[0]?.start ?? 0
  const bottom = v.getTotalSize() - (items[items.length - 1]?.end ?? 0)

  return (
    <div
      ref={outer}
      className="h-full [&_[data-slot=table-container]]:h-full [&_[data-slot=table-container]]:overflow-auto"
    >
      <Table>
        <TableHeader className="sticky top-0 z-10 bg-background">
          <TableRow>
            <TableHead className="w-16 text-right text-muted-foreground">#</TableHead>
            {columns.map((c) => (
              <TableHead key={c}>
                <ColumnMenu column={c} onAction={onAction} />
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {top > 0 && <tr style={{ height: top }} />}
          {items.map((it) => {
            const row = blocks.get(Math.floor(it.index / BLOCK))?.[it.index % BLOCK]
            return (
              <TableRow key={it.index} style={{ height: ROW_H }}>
                <TableCell className="text-right font-mono text-xs text-muted-foreground tabular-nums">
                  <span className="inline-flex items-center gap-1">
                    {row?.[2] && <StarIcon aria-label="Starred" className="size-3 fill-current" />}
                    {row?.[3] && <FlagIcon aria-label="Flagged" className="size-3 fill-current" />}
                    {row ? row[0] + 1 : ""}
                  </span>
                </TableCell>
                {columns.map((c, i) => (
                  <TableCell key={c} className="max-w-xs truncate" title={row?.[1][i] ?? undefined}>
                    {row?.[1][i]}
                  </TableCell>
                ))}
              </TableRow>
            )
          })}
          {bottom > 0 && <tr style={{ height: bottom }} />}
        </TableBody>
      </Table>
    </div>
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
