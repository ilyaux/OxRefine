import { useEffect, useRef, useState } from "react"
import { ChevronDownIcon, ChevronUpIcon, XIcon } from "lucide-react"
import { call, onPending } from "@/api"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"
import { ProcessMemory, type ProcessMemoryData } from "@/ProcessMemory"

type Stage = {
  label: string
  done: number
  total: number
  unit: string
  state: "running" | "completed" | "failed" | "cancelled"
  elapsedMs: number
}
type Job = Stage & { id: number; cancelRequested: boolean; stages: Stage[] }

const size = (n: number) =>
  n >= 1e9
    ? `${(n / 1e9).toFixed(1)} GB`
    : n >= 1e6
      ? `${(n / 1e6).toFixed(1)} MB`
      : n >= 1e3
        ? `${(n / 1e3).toFixed(0)} KB`
        : `${n}`
const count = (j: Stage, n: number) =>
  j.unit === "bytes" ? size(n) : n.toLocaleString()
const SEGMENTS = 40

/** OpenRefine's process panel, docked: while a command runs for more than a moment, what it is doing, how
 * far along, for how long, with Cancel. */
export function Jobs({ memory }: { memory?: ProcessMemoryData | null } = {}) {
  const [pending, setPending] = useState(0)
  const [jobs, setJobs] = useState<Job[]>([])
  const [open, setOpen] = useState(true)
  const seen = useRef(new Set<number>())
  const busy = pending > 0
  useEffect(() => onPending(setPending), [])
  useEffect(() => {
    let live = true
    let timer: ReturnType<typeof setTimeout>
    if (!busy && !seen.current.size) {
      return
    }
    const poll = async () => {
      try {
        const { data } = await call<Job[]>("processes", {}, { quiet: true })
        if (!live) return
        if (busy) {
          const active = data.filter(
            (j) => j.state === "running" && (j.label !== "Working" || j.done > 0)
          )
          if (active.some((j) => !seen.current.has(j.id)))
            seen.current = new Set(active.map((j) => j.id))
        }
        setJobs(data.filter((j) => seen.current.has(j.id)))
      } catch {
        // The command reports its own connection error; progress polling doesn't add a second one.
      }
      if (live && busy) timer = setTimeout(poll, 300)
    }
    // quick commands finish before the first poll and never show the panel
    timer = setTimeout(poll, busy ? 300 : 0)
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [busy])

  const shown = jobs
  const running = shown.filter((j) => j.state === "running").length
  if (!shown.length) return null
  return (
    <section
      aria-label="Jobs"
      role="status"
      aria-live="polite"
      className="fixed right-6 bottom-12 z-50 w-130 border border-foreground bg-background shadow-lg"
    >
      <div className="flex h-10 items-stretch border-b">
        <div className="flex flex-1 items-center gap-2 px-4">
          <span className="font-semibold">Jobs</span>
          <span className="font-mono text-[11.5px] text-muted-foreground">
            {running ? `${running} running` : "Finished"}
          </span>
        </div>
        {!running && (
          <button
            type="button"
            aria-label="Dismiss finished jobs"
            onClick={() => {
              seen.current.clear()
              setJobs([])
            }}
            className="flex w-10 items-center justify-center border-l hover:bg-accent"
          >
            <XIcon className="size-4" />
          </button>
        )}
        <button
          type="button"
          aria-label={
            open ? "Collapse the jobs panel" : "Expand the jobs panel"
          }
          aria-expanded={open}
          onClick={() => setOpen(!open)}
          className="flex w-10 items-center justify-center border-l text-muted-foreground outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
        >
          {open ? (
            <ChevronDownIcon className="size-4" />
          ) : (
            <ChevronUpIcon className="size-4" />
          )}
        </button>
      </div>
      {memory !== undefined && (
        <div className="border-b px-4 py-2">
          <ProcessMemory memory={memory} />
        </div>
      )}
      {open &&
        shown.map((j) => {
          const share = j.total ? Math.min(1, j.done / j.total) : null
          const secs = j.elapsedMs / 1000
          const filled = share === null ? 0 : Math.round(share * SEGMENTS)
          return (
            <div
              key={j.id}
              className="flex flex-col gap-2.5 border-b px-4 py-4 last:border-b-0"
            >
              <div className="flex items-baseline gap-3">
                <span className="min-w-0 flex-1 truncate font-medium">
                  {j.label}
                </span>
                <span className="font-mono text-[11.5px] text-muted-foreground">
                  {secs.toFixed(1)} s
                </span>
              </div>
              <div className="flex items-baseline gap-2">
                <span className="font-mono text-[22px] leading-7">
                  {j.done || j.total
                    ? count(j, j.done)
                    : j.state === "running"
                      ? "…"
                      : j.state}
                </span>
                {j.total > 0 && (
                  <span className="font-mono text-[12.5px] text-muted-foreground">
                    / {count(j, j.total)}
                    {j.unit !== "bytes" && ` ${j.unit}`}
                  </span>
                )}
                <span className="flex-1" />
                {share !== null && (
                  <span className="font-mono text-[12.5px] font-medium">
                    {Math.round(share * 100)}%
                  </span>
                )}
              </div>
              <div
                role="progressbar"
                aria-label={j.label}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={
                  share === null ? undefined : Math.round(share * 100)
                }
                className="flex h-2.5 gap-0.5"
              >
                {Array.from({ length: SEGMENTS }, (_, i) => (
                  <span
                    key={i}
                    className={cn(
                      "flex-1",
                      share === null
                        ? j.state === "running"
                          ? "animate-pulse bg-row-rule"
                          : "bg-row-rule"
                        : i < filled
                          ? "bg-foreground"
                          : "bg-row-rule"
                    )}
                  />
                ))}
              </div>
              <ol
                aria-label="Work stages"
                className="max-h-40 space-y-1 overflow-auto font-mono text-[11.5px] text-muted-foreground"
              >
                {j.stages.map((s, i) => (
                  <li key={i} className="flex items-baseline gap-2">
                    <span className="min-w-0 flex-1">{s.label}</span>
                    <span>{s.state}</span>
                    <span>{(s.elapsedMs / 1000).toFixed(1)} s</span>
                  </li>
                ))}
              </ol>
              <div className="flex items-center justify-between">
                <span className="font-mono text-[11.5px] text-muted-foreground">
                  {j.state === "running"
                    ? j.cancelRequested
                      ? "Stopping at the next cancellation check…"
                      : "Changes will appear when the command finishes"
                    : j.state}
                </span>
                {j.state === "running" && (
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={j.cancelRequested}
                    onClick={() =>
                      call(
                        "cancel_process",
                        { id: j.id },
                        { quiet: true }
                      ).catch(() => {})
                    }
                  >
                    {j.cancelRequested ? "Stopping…" : "Cancel"}
                  </Button>
                )}
              </div>
            </div>
          )
        })}
    </section>
  )
}
