import { useEffect, useRef, useState } from "react"
import { call, scatterFacet, type Engine, type ScatterData, type ScatterFacet } from "@/api"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Spinner } from "@/components/ui/spinner"
import { FullView, Tag } from "@/components/instrument"

type Box = Pick<ScatterFacet, "from_x" | "to_x" | "from_y" | "to_y">
const FULL: Box = { from_x: 0, to_x: 1, from_y: 0, to_y: 1 }
export const selects = (b: Box) => b.from_x > 0 || b.to_x < 1 || b.from_y > 0 || b.to_y < 1
const DOTS: [string, number][] = [["Small", 0.4], ["Regular", 0.8], ["Big", 1.4]]

/**
 * A scatterplot drawn on a canvas: every row's point faint, the points of the rows the other facets
 * let through in the data colour. With `onSelect`, dragging a rectangle selects (as fractions of the
 * side, y upwards); a click without dragging clears the selection.
 */
function Plot({ data, size, dot, box, onSelect, label }: { data: ScatterData; size: number; dot: number; box?: Box; onSelect?: (b: Box) => void; label: string }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const [drag, setDrag] = useState<{ x0: number; y0: number; x1: number; y1: number } | null>(null)
  useEffect(() => {
    const c = canvas.current
    const g = c?.getContext("2d")
    if (!c || !g) return
    const ratio = window.devicePixelRatio || 1
    c.width = size * ratio
    c.height = size * ratio
    g.setTransform(ratio, 0, 0, ratio, 0, 0)
    g.clearRect(0, 0, size, size)
    const css = getComputedStyle(c)
    const k = size / data.l
    const side = Math.max(1.5, dot * 2.5)
    const draw = (points: number[], color: string) => {
      g.fillStyle = color
      for (let i = 0; i < points.length; i += 2) g.fillRect(points[i] * k - side / 2, (data.l - points[i + 1]) * k - side / 2, side, side)
    }
    draw(data.all, css.getPropertyValue("--faint"))
    draw(data.points, css.getPropertyValue("--data"))
  }, [data, size, dot])

  const at = (e: React.PointerEvent) => {
    const r = e.currentTarget.getBoundingClientRect()
    return { x: Math.min(Math.max(e.clientX - r.left, 0), size), y: Math.min(Math.max(e.clientY - r.top, 0), size) }
  }
  // the rectangle on screen: the one being dragged, else the selection
  const shown = drag
    ? { left: Math.min(drag.x0, drag.x1), top: Math.min(drag.y0, drag.y1), width: Math.abs(drag.x1 - drag.x0), height: Math.abs(drag.y1 - drag.y0) }
    : box && selects(box)
      ? { left: box.from_x * size, top: (1 - box.to_y) * size, width: (box.to_x - box.from_x) * size, height: (box.to_y - box.from_y) * size }
      : null
  return (
    <div
      className="relative shrink-0 border border-input bg-background"
      style={{ width: size + 2, height: size + 2, cursor: onSelect ? "crosshair" : undefined, touchAction: onSelect ? "none" : undefined }}
      onPointerDown={
        onSelect &&
        ((e) => {
          e.currentTarget.setPointerCapture(e.pointerId)
          const p = at(e)
          setDrag({ x0: p.x, y0: p.y, x1: p.x, y1: p.y })
        })
      }
      onPointerMove={(e) => drag && setDrag({ ...drag, ...((p) => ({ x1: p.x, y1: p.y }))(at(e)) })}
      onPointerUp={() => {
        if (!drag || !onSelect) return
        const [x0, x1] = [Math.min(drag.x0, drag.x1), Math.max(drag.x0, drag.x1)]
        const [y0, y1] = [Math.min(drag.y0, drag.y1), Math.max(drag.y0, drag.y1)]
        setDrag(null)
        onSelect(x1 - x0 < 2 || y1 - y0 < 2 ? FULL : { from_x: x0 / size, to_x: x1 / size, from_y: 1 - y1 / size, to_y: 1 - y0 / size })
      }}
    >
      <canvas ref={canvas} role="img" aria-label={label} style={{ width: size, height: size }} />
      {shown && <div aria-hidden="true" className="pointer-events-none absolute border border-data bg-data/10" style={shown} />}
    </div>
  )
}

const num = (n: number) => Number(n.toPrecision(6)).toLocaleString()

/** The scatterplot facet's panel: the plot to select on, and the selection in the columns' units. */
export function ScatterPanel({ facet, data, onChange }: { facet: ScatterFacet; data: ScatterData | null; onChange: (f: ScatterFacet) => void }) {
  if (!data) return null
  if (data.error) return <p className="px-3.5 text-xs text-destructive">{data.error}</p>
  const span = (from: number, to: number, min: number, max: number) => `${num(min + from * (max - min))} – ${num(min + to * (max - min))}`
  return (
    <div className="flex flex-col gap-2 px-3.5">
      <Plot data={data} size={facet.l} dot={facet.dot} box={facet} onSelect={(b) => onChange({ ...facet, ...b })} label={`Scatterplot of ${facet.cx} against ${facet.cy}`} />
      <p className="font-mono text-[11.5px] text-muted-foreground">
        {selects(facet) ? (
          <>
            {facet.cx} {span(facet.from_x, facet.to_x, data.min_x, data.max_x)}
            <br />
            {facet.cy} {span(facet.from_y, facet.to_y, data.min_y, data.max_y)}
          </>
        ) : (
          "Drag a rectangle to select rows."
        )}
      </p>
    </div>
  )
}

/**
 * OpenRefine's scatterplot matrix: a plot for each pair of numeric columns (the pairs with
 * `column` when there are many); picking one adds a scatterplot facet on that pair.
 */
export function ScatterMatrix({ column, columns, engine, onFacet, onClose }: { column: string; columns: string[]; engine: Engine; onFacet: (f: ScatterFacet) => void; onClose: () => void }) {
  const [numeric, setNumeric] = useState<string[] | null>(null)
  const [plots, setPlots] = useState<Record<string, ScatterData>>({})
  const [dot, setDot] = useState(0.8)
  const SIZE = 132
  useEffect(() => {
    call<{ type: string }[]>("profile", {}, { quiet: true }).then(
      ({ data }) => setNumeric(columns.filter((_, i) => data[i]?.type === "number")),
      () => setNumeric([]),
    )
  }, [columns])
  const pairs = (numeric ?? []).flatMap((cx, i, all) => all.slice(i + 1).map((cy) => [cx, cy] as const)).filter(([cx, cy]) => (numeric?.length ?? 0) <= 8 || cx === column || cy === column)
  // the chosen column's pairs first
  pairs.sort((a, b) => Number(b.includes(column)) - Number(a.includes(column)))
  const key = pairs.map((p) => p.join("\u0000")).join("\u0001")
  useEffect(() => {
    let live = true
    for (const [cx, cy] of pairs) {
      call<ScatterData>("scatterplot", { engine, params: { cx, cy, l: SIZE } }, { quiet: true }).then(
        ({ data }) => live && setPlots((p) => ({ ...p, [`${cx}\u0000${cy}`]: data })),
        () => {},
      )
    }
    return () => {
      live = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, engine])
  return (
    <FullView
      crumbs={["Scatterplot matrix", <Tag key="c">{column}</Tag>]}
      onClose={onClose}
      bar={
        <ToggleGroup aria-label="Dot size" variant="outline" size="sm" value={[String(dot)]} onValueChange={(v) => v[0] && setDot(Number(v[0]))} className="self-center px-3">
          {DOTS.map(([label, d]) => (
            <ToggleGroupItem key={label} value={String(d)}>
              {label}
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      }
    >
      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {numeric === null ? (
          <Spinner />
        ) : pairs.length === 0 ? (
          <p className="text-muted-foreground">A scatterplot needs two numeric columns. Make a column's values numbers first (Transform → To number).</p>
        ) : (
          <>
            <p className="pb-4 text-xs text-muted-foreground">Pick a pair to facet on: drag on its plot in the facet panel to select rows. Plots show the rows the facets select.</p>
            <div className="flex flex-wrap gap-4">
              {pairs.map(([cx, cy]) => {
                const data = plots[`${cx}\u0000${cy}`]
                return (
                  <button
                    key={`${cx}\u0000${cy}`}
                    type="button"
                    onClick={() => {
                      onFacet(scatterFacet(cx, cy, dot))
                      onClose()
                    }}
                    className="flex flex-col items-start gap-1.5 p-1 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    {data && !data.error ? (
                      <Plot data={data} size={SIZE} dot={dot} label={`${cx} against ${cy}`} />
                    ) : (
                      <div className="flex items-center justify-center border border-input text-xs text-muted-foreground" style={{ width: SIZE + 2, height: SIZE + 2 }}>
                        {data?.error ?? <Spinner />}
                      </div>
                    )}
                    <span className="max-w-34 truncate text-xs" title={`${cx} (x) vs. ${cy} (y)`}>
                      {cx} <span className="text-muted-foreground">×</span> {cy}
                    </span>
                  </button>
                )
              })}
            </div>
          </>
        )}
      </div>
    </FullView>
  )
}
