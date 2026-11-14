import { useRef, useState } from "react"
import { Button } from "@/components/ui/button"

/** Boundaries and the importer both count Unicode scalars, including one slot for an emoji. */
const widthsAt = (positions: number[]) => positions.map((p, i) => p - (positions[i - 1] ?? 0))

export function FixedWidthRuler({ lines, widths, onChange }: { lines: string[]; widths: number[]; onChange: (widths: number[]) => void }) {
  const ruler = useRef<HTMLDivElement>(null)
  const dragging = useRef<number | null>(null)
  const [selected, setSelected] = useState<number | null>(null)
  const positions: number[] = []
  for (const width of widths) positions.push((positions.at(-1) ?? 0) + width)
  const used = new Set(positions)
  const extent = Math.min(512, Math.max(40, ...lines.map((s) => Array.from(s).length), ...positions.filter((p) => p <= 512)))
  const hidden = positions.filter((p) => p > extent).length
  const point = (e: { clientX: number }) => {
    const box = ruler.current!.getBoundingClientRect()
    return Math.round(((e.clientX - box.left) / box.width) * extent)
  }
  const move = (i: number, to: number) => {
    const at = [...positions]
    at[i] = Math.max((at[i - 1] ?? 0) + 1, Math.min(to, extent, (at[i + 1] ?? extent + 1) - 1))
    onChange(widthsAt(at))
  }
  const remove = (i: number) => {
    onChange(widthsAt(positions.filter((_, j) => j !== i)))
    setSelected(null)
  }
  const add = (at: number) => {
    at = Math.max(1, Math.min(extent, at))
    if (positions.includes(at)) return
    const next = [...positions, at].sort((a, b) => a - b)
    onChange(widthsAt(next))
    setSelected(next.indexOf(at))
  }
  const available = Array.from({ length: extent }, (_, i) => i + 1).filter((p) => !used.has(p))
  const inSample = available.filter((p) => p <= Math.max(1, ...lines.map((s) => Array.from(s).length)))
  return (
    <section aria-label="Fixed-width ruler" className="shrink-0 border-b">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 border-b px-5 py-1.5 text-xs">
        <span className="text-muted-foreground">Click to add a split. Drag or use arrow keys to move; Delete removes.</span>
        <Button variant="ghost" size="xs" disabled={!available.length} onClick={() => add(inSample.length ? inSample[Math.floor(inSample.length / 2)] : available[0])}>Add split</Button>
        <Button variant="ghost" size="xs" disabled={selected === null || selected >= positions.length} onClick={() => selected !== null && remove(selected)}>Remove selected split</Button>
        {!!hidden && <span className="text-muted-foreground">{hidden} splits past character 512: edit Column widths.</span>}
      </div>
      <div className="max-h-56 overflow-auto px-5" tabIndex={0} aria-label="Source characters; scroll horizontally for longer lines">
        <div ref={ruler} className="relative font-mono text-xs leading-6" style={{ width: `${extent * 2}ch` }}>
          <div className="relative h-8 cursor-crosshair border-b border-foreground/40" onClick={(e) => add(point(e))} aria-hidden="true">
            {Array.from({ length: Math.floor(extent / 5) }, (_, i) => (i + 1) * 5).map((p) => (
              <span key={p} className="pointer-events-none absolute bottom-0 border-l border-row-rule pl-0.5 text-[10px] text-muted-foreground" style={{ left: `${p * 2}ch` }}>{p}</span>
            ))}
          </div>
          {lines.map((line, i) => (
            <div key={i} className="h-6 whitespace-nowrap" aria-label={`Source line ${i + 1}: ${line}`}>
              {Array.from(line).map((c, j) => <span key={j} aria-hidden="true" className="inline-block w-[2ch] overflow-hidden text-center align-top">{c === "\t" ? " " : c}</span>)}
            </div>
          ))}
          {positions.map((p, i) => p <= extent && (
            <span key={i} className="pointer-events-none absolute top-0 bottom-0 border-l border-data" style={{ left: `${p * 2}ch` }}>
              <button
                type="button"
                role="slider"
                aria-label={`Split ${i + 1}, after character ${p}`}
                aria-valuemin={(positions[i - 1] ?? 0) + 1}
                aria-valuemax={Math.min(extent, (positions[i + 1] ?? extent + 1) - 1)}
                aria-valuenow={p}
                aria-orientation="horizontal"
                title={`After character ${p}. Drag or use arrow keys; Delete removes.`}
                className="pointer-events-auto absolute -left-2 top-0 h-8 w-4 cursor-col-resize touch-none border-x border-data bg-data/15 outline-none hover:bg-data/30 focus-visible:ring-2 focus-visible:ring-ring"
                onFocus={() => setSelected(i)}
                onClick={(e) => e.stopPropagation()}
                onDoubleClick={() => remove(i)}
                onPointerDown={(e) => { e.preventDefault(); e.currentTarget.focus(); setSelected(i); dragging.current = i; e.currentTarget.setPointerCapture(e.pointerId) }}
                onPointerMove={(e) => dragging.current === i && move(i, point(e))}
                onPointerUp={(e) => { dragging.current = null; e.currentTarget.releasePointerCapture(e.pointerId) }}
                onPointerCancel={() => { dragging.current = null }}
                onKeyDown={(e) => {
                  if (e.key === "Delete" || e.key === "Backspace") { e.preventDefault(); remove(i) }
                  else if (e.key === "ArrowLeft" || e.key === "ArrowRight") { e.preventDefault(); move(i, p + (e.key === "ArrowLeft" ? -1 : 1)) }
                  else if (e.key === "Home" || e.key === "End") { e.preventDefault(); move(i, e.key === "Home" ? 1 : extent) }
                }}
              />
            </span>
          ))}
        </div>
      </div>
    </section>
  )
}
