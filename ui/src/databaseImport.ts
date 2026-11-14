import { call } from "@/api"

export type DatabaseDriver = "sqlite" | "postgres" | "mysql" | "mariadb"
export type DatabaseProfile = {
  id: string
  revision: string
  name: string
  driver: DatabaseDriver
  host: string
  port: number
  database: string
  username: string
  sqlitePath: string
  tls: "verify-full" | "disable"
}
export type DatabaseProfiles = {
  profiles: DatabaseProfile[]
  drivers: { id: DatabaseDriver; label: string; available: boolean; reason?: string }[]
}
export type DatabaseTable = { schema: string; name: string; columns: { name: string; type: string; nullable: boolean }[] }
export type DatabaseSchema = { tables: DatabaseTable[] }
export type DatabasePreview = {
  id: string
  profileRevision: string
  columns: string[]
  columnTypes: string[]
  rows: [number, (string | number | boolean | null)[], boolean, boolean, string?, (number | null)?][]
  total: number
  expiresInSeconds: number
  warnings?: string[]
}

export const newDatabaseProfile = (): DatabaseProfile => ({ id: "", revision: "", name: "", driver: "sqlite", host: "localhost", port: 5432, database: "", username: "", sqlitePath: "", tls: "verify-full" })

/** Whitelist saved fields: credentials never become part of a saved profile or project source. */
export function databaseProfile(profile: DatabaseProfile): DatabaseProfile {
  const { id, revision, name, driver, host, port, database, username, sqlitePath, tls } = profile
  return { id, revision, name, driver, host, port, database, username, sqlitePath, tls }
}

export function databaseConnectionError(profile: DatabaseProfile) {
  if (!profile.name.trim()) return "Enter a connection name."
  if (profile.driver === "sqlite") return profile.sqlitePath.trim() ? "" : "Enter the SQLite file path on the server."
  if (!profile.host.trim() || !profile.database.trim() || !profile.username.trim()) return "Enter the database host, database name and user."
  if (!Number.isInteger(profile.port) || profile.port < 1 || profile.port > 65535) return "Port must be a whole number from 1 to 65535."
  return ""
}

export function databaseQueryOptions(maxRowsText: string, timeoutText: string) {
  const maxRows = Number(maxRowsText), timeoutSeconds = Number(timeoutText)
  if (!maxRowsText.trim() || !Number.isSafeInteger(maxRows) || maxRows < 1 || maxRows > 1_000_000) return { error: "Maximum rows must be a whole number from 1 to 1,000,000." } as const
  if (!timeoutText.trim() || !Number.isSafeInteger(timeoutSeconds) || timeoutSeconds < 1 || timeoutSeconds > 120) return { error: "Timeout must be a whole number from 1 to 120 seconds." } as const
  return { maxRows, timeoutSeconds } as const
}

/** Schema names are SQL identifiers, including punctuation and embedded quote characters. */
export function databaseTableQuery(table: Pick<DatabaseTable, "schema" | "name">, driver: DatabaseDriver) {
  const mark = driver === "mysql" || driver === "mariadb" ? "`" : '"'
  const quote = (name: string) => mark + name.replaceAll(mark, mark + mark) + mark
  return `SELECT * FROM ${[table.schema, table.name].filter(Boolean).map(quote).join(".")}`
}

export const databaseRequestId = () => globalThis.crypto?.randomUUID?.() ?? `database-${Date.now()}-${Math.random().toString(36).slice(2)}`

/** The password stays in this request; cancellation targets only its exact job marker. */
export function databaseJob<T>(command: "database_test" | "database_schema" | "database_prepare", profile: DatabaseProfile, password: string, extra: object = {}) {
  const requestId = databaseRequestId()
  let done = false, stopWanted = false, stopSent = false, job: number | null = null
  let timer: ReturnType<typeof setTimeout> | undefined
  const stop = () => {
    stopWanted = true
    if (!done && job !== null && !stopSent) {
      stopSent = true
      void call("cancel_process", { id: job }, { quiet: true }).catch(() => { /* Jobs allows a manual retry. */ })
    }
  }
  const poll = async () => {
    if (done) return
    try {
      const { data } = await call<{ id: number; state: string; stages: { label: string }[] }[]>("processes", {}, { quiet: true })
      if (done) return
      const own = data.find((candidate) => candidate.id > 0 && candidate.state === "running" && candidate.stages.some((stage) => stage.label === `Database request ${requestId}`))
      if (own) { job = own.id; if (stopWanted) stop(); return }
    } catch { /* The operation itself reports connection errors. */ }
    if (!done) timer = setTimeout(() => void poll(), 300)
  }
  const promise = call<T>(command, { params: { ...extra, profile: databaseProfile(profile), credentials: { password }, requestId } })
    .finally(() => { done = true; clearTimeout(timer) })
  void poll()
  return { promise, stop }
}

export function databaseError(error: unknown, password: string) {
  const message = String((error as Error)?.message ?? error)
  return password ? message.replaceAll(password, "[password]") : message
}
