import { useEffect, useState } from "react"
import { call, onPending } from "@/api"
import { Button } from "@/components/ui/button"
import { Card, CardAction, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Progress } from "@/components/ui/progress"

type Job = { id: number; label: string; done: number; total: number }

const size = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${(n / 1e3).toFixed(0)} KB` : `${n}`)
/** Bytes for reads and downloads, plain counts for recipe steps. */
const amount = (j: Job) => (j.label.startsWith("Applying") ? `${j.done} of ${j.total}` : j.total ? `${size(j.done)} of ${size(j.total)}` : size(j.done))

/** OpenRefine's process panel: while a command runs for more than a moment, show what it is doing, with Cancel. */
export function Jobs() {
  const [pending, setPending] = useState(0)
  const [jobs, setJobs] = useState<Job[]>([])
  useEffect(() => onPending(setPending), [])
  useEffect(() => {
    if (!pending) return setJobs([])
    let live = true
    const poll = () =>
      call<Job[]>("processes", {}, { quiet: true }).then(
        ({ data }) => live && setJobs(data.filter((j) => j.label !== "Working" || j.done > 0)),
        () => {},
      )
    // quick commands finish before the first poll and never show the card
    const t = setInterval(poll, 300)
    return () => {
      live = false
      clearInterval(t)
    }
  }, [pending])

  if (!jobs.length) return null
  return (
    <div className="fixed right-4 bottom-12 z-50 flex w-80 flex-col gap-2" role="status" aria-live="polite">
      {jobs.map((j) => (
        <Card key={j.id} size="sm">
          <CardHeader>
            <CardTitle className="truncate">{j.label}…</CardTitle>
            <CardDescription className="tabular-nums">{j.done || j.total ? amount(j) : "Please wait"}</CardDescription>
            <CardAction>
              <Button variant="outline" size="sm" onClick={() => call("cancel_process", { id: j.id }, { quiet: true }).catch(() => {})}>
                Cancel
              </Button>
            </CardAction>
            <Progress className="col-span-full" value={j.total ? Math.min(100, (100 * j.done) / j.total) : null} aria-label={j.label} />
          </CardHeader>
        </Card>
      ))}
    </div>
  )
}
