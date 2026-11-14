import { call } from "@/api"

export type CommonsCategory = { name: string; depth: number }
type Recon = { j?: string; m?: { id: string; name?: string } | null }
export type CommonsRow = [number, (string | null)[], boolean, boolean, string?, (number | null)?, Record<string, Recon>?]
export type CommonsPreview = {
  id: string
  requestId: string
  columns: string[]
  rows: CommonsRow[]
  total: number
  categories: CommonsCategory[]
  requests: number
  warnings: string[]
  expiresInSeconds: number
}

export function commonsSource(titles: string, depthText: string) {
  const names = titles.split(/\r?\n/).map((name) => name.trim()).filter(Boolean)
  const depth = Number(depthText)
  if (!names.length) return { error: "Enter at least one Commons category title." } as const
  if (names.length > 20) return { error: "Use at most 20 categories in one import." } as const
  if (names.some((name) => /^https?:\/\//i.test(name))) return { error: "Enter category titles rather than web addresses." } as const
  if (!depthText.trim() || !Number.isInteger(depth) || depth < 0 || depth > 10) return { error: "Subcategory depth must be a whole number from 0 to 10." } as const
  return { categories: names.map((name) => ({ name, depth })) } as const
}

/** Find only this request's unique job marker, including when work starts after the view closes. */
export function prepareCommons(categories: CommonsCategory[], includeWikitext: boolean, requestId: string) {
  let done = false, stopWanted = false, stopSent = false
  let job: number | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  const stop = () => {
    stopWanted = true
    if (!done && job !== null && !stopSent) {
      stopSent = true
      void call("cancel_process", { id: job }, { quiet: true }).catch(() => { /* Jobs offers a manual retry if the connection failed. */ })
    }
  }
  const poll = async () => {
    if (done) return
    try {
      const { data } = await call<{ id: number; state: string; stages: { label: string }[] }[]>("processes", {}, { quiet: true })
      if (done) return
      const own = data.find((candidate) => candidate.id > 0 && candidate.state === "running" && candidate.stages.some((stage) => stage.label === `Commons request ${requestId}`))
      if (own) { job = own.id; if (stopWanted) stop(); return }
    } catch { /* The prepare call reports its own connection error. */ }
    if (!done) timer = setTimeout(() => void poll(), 300)
  }
  const promise = call<CommonsPreview>("commons_prepare", { params: { categories, includeWikitext, requestId } })
    .finally(() => { done = true; clearTimeout(timer) })
  void poll()
  return { promise, stop }
}
