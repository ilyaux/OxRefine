import { useEffect, useRef, useState, type KeyboardEvent, type PointerEvent } from "react"
import { cn, maxOf } from "@/lib/utils"

type Pair = [number, number]
type Part = "range" | "from" | "to"
type Gesture = {
  pointer: number; target: SVGSVGElement | HTMLButtonElement; revision: object; domain: string
  part: Part; x: number; anchor: number; initial: Pair; values: Pair; moved: boolean
}
type Draft = { revision: object; domain: string; values: Pair }
const release = (current: Gesture) => {
  if (current.target.hasPointerCapture(current.pointer)) current.target.releasePointerCapture(current.pointer)
}

/** One local gesture, one facet update on release. Numeric upper bounds stay exclusive; dates inclusive. */
export function HistogramRange({ label, bins, baseBins, ticks, min, max, step, from, to, inclusive = false, revision, format, onCommit }: {
  label: string; bins: number[]; baseBins: number[]; ticks: string[]
  min: number; max: number; step: number; from: number; to: number; inclusive?: boolean
  revision: object; format: (value: number) => string; onCommit: (from: number, to: number) => void
}) {
  const plot = useRef<SVGSVGElement>(null)
  const gesture = useRef<Gesture | null>(null)
  const [draft, setDraft] = useState<Draft | null>(null)
  const domain = `${min}:${max}:${step}:${inclusive}`
  if (draft && (draft.revision !== revision || draft.domain !== domain)) setDraft(null)
  const span = max - min
  const gap = inclusive ? 0 : Math.min(step, span)
  const enabled = [min, max, step, from, to, span, span / step].every(Number.isFinite) && span > 0 && step > 0 && baseBins.length > 0 &&
    (inclusive || min + gap > min && max - gap < max)
  const clamp = (value: number) => Math.max(min, Math.min(max, value))
  const ordered = (a: number, b: number): Pair => {
    let lo = clamp(Math.min(a, b)), hi = clamp(Math.max(a, b))
    if (hi - lo < gap) { hi = Math.min(max, lo + gap); lo = Math.max(min, hi - gap) }
    return [lo, hi]
  }
  const values = draft?.revision === revision && draft.domain === domain ? draft.values : ordered(from, to)
  useEffect(() => () => {
    const current = gesture.current
    gesture.current = null
    if (current) release(current)
  }, [revision, domain])
  const cancel = () => {
    const current = gesture.current
    gesture.current = null
    setDraft(null)
    if (current) release(current)
  }
  const at = (clientX: number): number | null => {
    const rect = plot.current?.getBoundingClientRect()
    if (!rect || !Number.isFinite(rect.width) || rect.width <= 0 || !Number.isFinite(clientX)) return null
    const fraction = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width))
    // Keep the engine's padded numeric max, which includes the last real value under [from,to).
    return fraction === 1 ? max : clamp(min + Math.round(fraction * span / step) * step)
  }
  const begin = (event: PointerEvent<SVGSVGElement | HTMLButtonElement>, part: Part) => {
    if (!enabled || gesture.current || event.button !== 0 || event.isPrimary === false) return
    const anchor = at(event.clientX)
    if (anchor === null) return
    try { event.currentTarget.setPointerCapture(event.pointerId) } catch { return }
    event.currentTarget.focus()
    event.preventDefault()
    const current: Gesture = { pointer: event.pointerId, target: event.currentTarget, revision, domain, part, x: event.clientX, anchor, initial: values, values, moved: false }
    gesture.current = current
    setDraft({ revision, domain, values })
  }
  const update = (event: PointerEvent<HTMLDivElement>) => {
    const current = gesture.current
    if (!current || current.pointer !== event.pointerId) return null
    if (current.revision !== revision || current.domain !== domain) { cancel(); return null }
    const value = at(event.clientX)
    if (value === null) return null
    current.moved ||= Math.abs(event.clientX - current.x) >= 2
    current.values = current.part === "range" ? ordered(current.anchor, value) : current.part === "from" ? ordered(value, current.initial[1]) : ordered(current.initial[0], value)
    setDraft({ revision, domain, values: current.values })
    return current
  }
  const finish = (event: PointerEvent<HTMLDivElement>) => {
    const current = update(event)
    if (!current) return
    gesture.current = null
    setDraft(null)
    release(current)
    if (current.moved) onCommit(...current.values)
  }
  const adjustWithKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    const index = event.currentTarget.dataset.bound === "from" ? 0 : 1
    const delta = event.key === "ArrowLeft" || event.key === "ArrowDown" ? -step : event.key === "ArrowRight" || event.key === "ArrowUp" ? step : event.key === "PageDown" ? -step * 10 : event.key === "PageUp" ? step * 10 : null
    if (delta === null && event.key !== "Home" && event.key !== "End") return
    event.preventDefault()
    cancel()
    const low = index === 0 ? min : values[0] + gap, high = index === 0 ? values[1] - gap : max
    const next = event.key === "Home" ? low : event.key === "End" ? high : Math.max(low, Math.min(high, values[index] + delta!))
    if (Number.isFinite(next) && next !== values[index]) onCommit(...(index === 0 ? [next, values[1]] : [values[0], next]) as Pair)
  }
  const top = maxOf(baseBins, 1), width = 100 / baseBins.length, barGap = Math.min(0.3, width / 4)
  return (
    <div className="px-3.5">
      <div className="relative" onPointerMove={update} onPointerUp={finish}
        onPointerCancel={(event) => { if (gesture.current?.pointer === event.pointerId) cancel() }}
        onLostPointerCapture={(event) => { if (gesture.current?.pointer === event.pointerId) cancel() }}
        onKeyDown={(event) => { if (event.key === "Escape" && gesture.current) { event.preventDefault(); cancel() } }}>
        <svg ref={plot} viewBox="0 0 100 40" preserveAspectRatio="none" className={cn("block h-12 w-full", enabled && "touch-none cursor-crosshair select-none")} role="img" aria-label={label} tabIndex={-1} onPointerDown={(event) => begin(event, "range")}>
          {baseBins.map((count, i) => (
            <g key={i}>
              <rect x={i * width + barGap / 2} y={40 - count / top * 40} width={width - barGap} height={count / top * 40} className="fill-row-rule" />
              <rect x={i * width + barGap / 2} y={40 - bins[i] / top * 40} width={width - barGap} height={bins[i] / top * 40} className={min + i * step + step > values[0] && (inclusive ? min + i * step <= values[1] : min + i * step < values[1]) ? "fill-data" : "fill-faint"} />
            </g>
          ))}
          {enabled && <rect x={(values[0] - min) / span * 100} y={0} width={(values[1] - values[0]) / span * 100} height={40} className="pointer-events-none fill-data/10 stroke-data" strokeWidth={0.3} />}
        </svg>
        {enabled && ([0, 1] as const).map((index) => (
          <button key={index} type="button" role="slider" data-bound={index === 0 ? "from" : "to"} aria-label={`${label}: ${index === 0 ? "from" : "to"}`} aria-orientation="horizontal"
            aria-valuemin={index === 0 ? min : values[0] + gap} aria-valuemax={index === 0 ? values[1] - gap : max} aria-valuenow={values[index]} aria-valuetext={format(values[index])}
            title={`${index === 0 ? "From" : "To"}: ${format(values[index])}`} style={{ left: `${(values[index] - min) / span * 100}%` }}
            className="absolute top-0 h-12 w-6 -translate-x-1/2 touch-none cursor-ew-resize outline-none focus-visible:ring-2 focus-visible:ring-ring"
            onPointerDown={(event) => begin(event, index === 0 ? "from" : "to")} onKeyDown={adjustWithKey}>
            <span aria-hidden="true" className="absolute inset-y-0 left-1/2 w-px bg-data" />
            <span aria-hidden="true" className="absolute bottom-0 left-1/2 size-2 -translate-x-1/2 border border-data bg-background" />
          </button>
        ))}
      </div>
      <div className="h-px bg-foreground" />
      <div className="flex justify-between pt-1 font-mono text-[11px] text-muted-foreground">{ticks.map((tick, i) => <span key={i}>{tick}</span>)}</div>
    </div>
  )
}
