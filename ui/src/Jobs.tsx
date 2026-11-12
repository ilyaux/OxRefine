import { useEffect, useState } from "react"
import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"
import { call, onPending } from "@/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

type Job = { id: number; label: string; done: number; total: number }

const size = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${(n / 1e3).toFixed(0)} KB` : `${n}`)
/** Recipe steps are counted, everything else (reads, downloads) is bytes. */
const counted = (j: Job) => j.label.startsWith("Applying")
const SEGMENTS = 40

/** OpenRefine's process panel, docked: while a command runs for more than a moment, what it is doing, how
 * far along, for how long, with Cancel. */
export function Jobs() {
  const [pending, setPending] = useState(0)
  const [jobs, setJobs] = useState<Job[]>([])
  const [open, setOpen] = useState(true)
  const [now, setNow] = useState(() => Date.now())
  // when each job was first seen, for its running time
  const [seen, setSeen] = useState<Record<number, number>>({})
  useEffect(() => onPending(setPending), [])
  useEffect(() => {
    if (!pending) return
    let live = true
    const poll = () =>
      call<Job[]>("processes", {}, { quiet: true }).then(
        ({ data }) => {
          if (!live) return
          const t = Date.now()
          setSeen((s) => (data.some((j) => !(j.id in s)) ? { ...s, ...Object.fromEntries(data.filter((j) => !(j.id in s)).map((j) => [j.id, t])) } : s))
          setNow(t)
          setJobs(data.filter((j) => j.label !== "Working" || j.done > 0))
        },
        () => {},
      )
    // quick commands finish before the first poll and never show the panel
    const t = setInterval(poll, 300)
    return () => {
      live = false
      clearInterval(t)
    }
  }, [pending])

  const shown = pending ? jobs : []
  if (!shown.length) return null
  return (
    <section aria-label="Jobs" role="status" aria-live="polite" className="fixed right-6 bottom-12 z-50 w-130 border border-foreground bg-background shadow-lg">
      <div className="flex h-10 items-stretch border-b">
        <div className="flex flex-1 items-center gap-2 px-4">
          <span className="font-semibold">Jobs</span>
          <span className="font-mono text-[11.5px] text-muted-foreground">{shown.length} running</span>
        </div>
        <button
          type="button"
          aria-label={open ? "Collapse the jobs panel" : "Expand the jobs panel"}
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="flex w-10 items-center justify-center border-l text-muted-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        >
          {open ? <ChevronDownIcon className="size-4" /> : <ChevronUpIcon className="size-4" />}
        </button>
      </div>
      {open &&
        shown.map((j) => {
          const share = j.total ? Math.min(1, j.done / j.total) : null
          const secs = (now - (seen[j.id] ?? now)) / 1000
          const filled = share === null ? 0 : Math.round(share * SEGMENTS)
          return (
            <div key={j.id} className="flex flex-col gap-2.5 border-b px-4 py-4 last:border-b-0">
              <div className="flex items-baseline gap-3">
                <span className="min-w-0 flex-1 truncate font-medium">{j.label}</span>
                <span className="font-mono text-[11.5px] text-muted-foreground">{secs.toFixed(1)} s</span>
              </div>
              <div className="flex items-baseline gap-2">
                <span className="font-mono text-[22px] leading-7">{j.done || j.total ? (counted(j) ? j.done.toLocaleString() : size(j.done)) : "…"}</span>
                {j.total > 0 && <span className="font-mono text-[12.5px] text-muted-foreground">/ {counted(j) ? `${j.total.toLocaleString()} steps` : size(j.total)}</span>}
                <span className="flex-1" />
                {share !== null && <span className="font-mono text-[12.5px] font-medium">{Math.round(share * 100)}%</span>}
              </div>
              <div
                role="progressbar"
                aria-label={j.label}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={share === null ? undefined : Math.round(share * 100)}
                className="flex h-2.5 gap-0.5"
              >
                {Array.from({ length: SEGMENTS }, (_, i) => (
                  <span key={i} className={cn("flex-1", share === null ? "animate-pulse bg-row-rule" : i < filled ? "bg-foreground" : "bg-row-rule")} />
                ))}
              </div>
              <div className="flex items-center justify-between">
                <span className="font-mono text-[11.5px] text-muted-foreground">you can keep working</span>
                <Button variant="outline" size="sm" onClick={() => call("cancel_process", { id: j.id }, { quiet: true }).catch(() => {})}>
                  Cancel
                </Button>
              </div>
            </div>
          )
        })}
    </section>
  )
}
