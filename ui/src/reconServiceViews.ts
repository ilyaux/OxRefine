import { call } from "@/api"

const KEY = "reconciliation.standardServices"
const saving = new Map<string, Promise<void>>()
let updates: Promise<void> = Promise.resolve()
const object = (value: unknown): Record<string, unknown> | null =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null

/** Keep only the service's page-link template, alongside any descriptors already saved. */
export function saveServiceView(
  url: string,
  manifest: { view?: { url?: string } }
): Promise<void> {
  const template = manifest.view?.url
  if (typeof template !== "string" || !template.trim()) return Promise.resolve()
  const key = JSON.stringify([url, template])
  const pending = saving.get(key)
  if (pending) return pending
  const work = updates
    .catch(() => {})
    .then(async () => {
      const { data: preferences } = await call<Record<string, unknown>>(
        "preferences",
        {},
        { quiet: true }
      )
      let current = preferences[KEY]
      let changed = typeof current === "string"
      if (typeof current === "string") {
        try {
          current = JSON.parse(current)
        } catch {
          throw new Error(
            "The saved reconciliation service registry is invalid."
          )
        }
      }
      if (current === undefined || current === null) current = []
      if (!Array.isArray(current))
        throw new Error(
          "The saved reconciliation service registry is not a list."
        )
      let found = false
      const services = current.map((item) => {
        const entry = object(item)
        if (!entry || (entry.url !== url && entry.service !== url)) return item
        found = true
        const view = object(entry.view)
        if (view?.url === template) return item
        changed = true
        return { ...entry, view: { ...view, url: template } }
      })
      if (!found) {
        services.push({ url, view: { url: template } })
        changed = true
      }
      if (changed)
        await call(
          "set_preference",
          { name: KEY, value: services },
          { quiet: true }
        )
    })
  updates = work
  saving.set(key, work)
  // Only concurrent identical writes are cached; a later read must see current server preferences.
  void work.then(
    () => saving.delete(key),
    () => saving.delete(key)
  )
  return work
}
