import { useEffect, useState } from "react"
import { call, isTauri } from "@/api"

export type ProcessMemoryData = {
  pid: number | null
  currentBytes: number | null
  peakBytes: number | null
  metric: "working-set" | "rss" | "unavailable"
  peakSource: "os" | null
  error: string | null
}

const unavailable = (): ProcessMemoryData => ({
  pid: null,
  currentBytes: null,
  peakBytes: null,
  metric: "unavailable",
  peakSource: null,
  error: "Process memory is unavailable.",
})
const bytes = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0

function sample(value: unknown): ProcessMemoryData {
  if (!value || typeof value !== "object") return unavailable()
  const data = value as Partial<ProcessMemoryData>
  if (!["working-set", "rss"].includes(data.metric ?? "")) return unavailable()
  const current = bytes(data.currentBytes) ? data.currentBytes : null
  const peak =
    bytes(data.peakBytes) &&
    data.peakSource === "os" &&
    (current === null || data.peakBytes >= current)
      ? data.peakBytes
      : null
  return {
    pid: bytes(data.pid) && data.pid > 0 ? data.pid : null,
    currentBytes: current,
    peakBytes: peak,
    metric: data.metric as ProcessMemoryData["metric"],
    peakSource: peak === null ? null : "os",
    error:
      typeof data.error === "string"
        ? data.error
        : current === null || peak === null
          ? "Process memory is partly unavailable."
          : null,
  }
}

/** One App-owned poller feeds both the status bar and Jobs. Quiet reads create no work/jobs. */
export function useProcessMemory(): ProcessMemoryData | null {
  const [memory, setMemory] = useState<ProcessMemoryData | null>(null)
  useEffect(() => {
    let live = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const poll = async () => {
      try {
        const { data } = await call<unknown>("memory", {}, { quiet: true })
        if (live) setMemory(sample(data))
      } catch {
        if (live) setMemory(unavailable())
      }
      // Schedule after completion, so a slow server never accumulates overlapping requests.
      if (live) timer = setTimeout(poll, 2000)
    }
    // A zero-delay start also lets React's development effect cleanup cancel its first mount.
    timer = setTimeout(poll, 0)
    return () => {
      live = false
      clearTimeout(timer)
    }
  }, [])
  return memory
}

function format(value: number | null) {
  if (value === null) return "—"
  if (value >= 1024 ** 3) return `${(value / 1024 ** 3).toFixed(1)} GiB`
  if (value >= 1024 ** 2) return `${(value / 1024 ** 2).toFixed(1)} MiB`
  if (value >= 1024) return `${(value / 1024).toFixed(0)} KiB`
  return `${value} B`
}

export function ProcessMemory({ memory }: { memory: ProcessMemoryData | null }) {
  const subject = isTauri ? "OxRefine backend process" : "OxRefine server process"
  const metric = memory?.metric === "working-set" ? "working set" : "resident memory"
  const title = `${subject}${memory?.pid ? ` (PID ${memory.pid})` : ""}: ${metric}, including all loaded projects and shared pages. Browser/WebView subprocesses and JavaScript heap are not measured here. Peak is the OS high-water mark since this process started.${memory?.error ? ` ${memory.error}` : ""}`
  return (
    <span
      aria-label={`${isTauri ? "Backend" : "Server"} process memory`}
      aria-live="off"
      title={title}
      className="whitespace-nowrap font-mono text-[11.5px] text-muted-foreground"
    >
      RAM {memory ? format(memory.currentBytes) : "…"} · peak {memory ? format(memory.peakBytes) : "…"}
    </span>
  )
}
