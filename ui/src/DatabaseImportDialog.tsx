import { useEffect, useRef, useState } from "react"
import { call } from "@/api"
import { databaseConnectionError, databaseError, databaseJob, databaseProfile, databaseQueryOptions, databaseTableQuery, newDatabaseProfile, type DatabaseDriver, type DatabasePreview, type DatabaseProfile, type DatabaseProfiles, type DatabaseSchema } from "@/databaseImport"
import { CellButton, FullView, Metric, RailLabel, Spacer, StripAction, SummaryStrip } from "@/components/instrument"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Spinner } from "@/components/ui/spinner"
import { toast } from "@/components/ui/toast"

const PAGE = 20
type Busy = "profiles" | "save" | "delete" | "test" | "schema" | "load" | "paging" | "create" | null

export function DatabaseImportDialog({ onCreate, onClose }: {
  onCreate: (args: { id: string; name: string }) => Promise<void>
  onClose: () => void
}) {
  const [catalog, setCatalog] = useState<DatabaseProfiles | null>(null)
  const [profile, setProfile] = useState(newDatabaseProfile)
  const [password, setPassword] = useState("")
  const [credentialVersion, setCredentialVersion] = useState(0)
  const [sql, setSql] = useState("")
  const [maxRows, setMaxRows] = useState("100000")
  const [timeout, setTimeout] = useState("30")
  const [name, setName] = useState("Database import")
  const [reply, setReply] = useState<{ key: string; data: DatabasePreview; start: number } | null>(null)
  const [schemaReply, setSchemaReply] = useState<{ key: string; data: DatabaseSchema } | null>(null)
  const [tested, setTested] = useState<string | null>(null)
  const [failure, setFailure] = useState("")
  const [busy, setBusy] = useState<Busy>("profiles")
  const [stopping, setStopping] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const live = useRef(true)
  const sequence = useRef(0)
  const work = useRef<{ stop: () => void } | null>(null)
  const connectionKey = JSON.stringify({ profile, credentialVersion })
  const sourceKey = JSON.stringify({ connectionKey, sql, maxRows, timeout })
  const preview = reply?.key === sourceKey ? reply : null
  const schema = schemaReply?.key === connectionKey ? schemaReply.data : null
  const options = databaseQueryOptions(maxRows, timeout)
  const connectionError = databaseConnectionError(profile)
  const driver = catalog?.drivers.find((candidate) => candidate.id === profile.driver)
  const locked = busy === "profiles" || busy === "save" || busy === "delete" || busy === "create"
  const running = busy === "test" || busy === "schema" || busy === "load"
  const canCreate = !!preview?.data.total && !!name.trim() && !busy

  useEffect(() => {
    live.current = true
    call<DatabaseProfiles>("database_profiles", {})
      .then(({ data }) => { if (live.current) setCatalog(data) })
      .catch((error) => { if (live.current) setFailure(databaseError(error, "")) })
      .finally(() => { if (live.current) setBusy(null) })
    return () => { live.current = false; work.current?.stop() }
  }, [])

  const invalidate = (connection = false) => {
    sequence.current++
    work.current?.stop(); work.current = null
    setReply(null); setFailure(""); setStopping(false); setBusy(null); setDeleting(false)
    if (connection) { setTested(null); setSchemaReply(null) }
  }
  const changeProfile = (patch: Partial<DatabaseProfile>) => { invalidate(true); setProfile((current) => ({ ...current, ...patch })) }
  const selectProfile = (id: string) => {
    invalidate(true)
    setProfile(id ? databaseProfile(catalog!.profiles.find((candidate) => candidate.id === id)!) : newDatabaseProfile())
    setPassword(""); setCredentialVersion((n) => n + 1)
  }
  const close = () => {
    if (busy === "create" || busy === "save" || busy === "delete") return
    live.current = false; sequence.current++
    work.current?.stop(); setPassword("")
    if (running) toast.add({ title: "Database import closed", description: "A stop was requested for this operation. It may continue until the next cancellation check. No project was created." })
    onClose()
  }
  const reloadProfiles = async () => {
    if (busy) return
    setBusy("profiles"); setFailure("")
    try {
      const { data } = await call<DatabaseProfiles>("database_profiles", {})
      if (!live.current) return
      setCatalog(data)
      if (profile.id) {
        invalidate(true)
        setProfile(data.profiles.find((candidate) => candidate.id === profile.id) ?? newDatabaseProfile())
        setPassword(""); setCredentialVersion((n) => n + 1)
      }
    }
    catch (error) { if (live.current) setFailure(databaseError(error, password)) }
    finally { if (live.current) setBusy(null) }
  }
  const saveProfile = async () => {
    if (busy || !profile.name.trim() || connectionError || !driver?.available) return
    invalidate(true); setBusy("save")
    try {
      const { data } = await call<{ profile: DatabaseProfile }>("database_save_profile", { params: { profile: databaseProfile({ ...profile, name: profile.name.trim() }) } })
      if (!live.current) return
      setProfile(data.profile)
      setCatalog((current) => current && ({ ...current, profiles: [...current.profiles.filter((candidate) => candidate.id !== data.profile.id), data.profile] }))
    } catch (error) { if (live.current) setFailure(databaseError(error, password)) }
    finally { if (live.current) setBusy(null) }
  }
  const deleteProfile = async () => {
    if (busy || !profile.id) return
    invalidate(true); setBusy("delete")
    try {
      await call("database_delete_profile", { id: profile.id, revision: profile.revision })
      if (!live.current) return
      setCatalog((current) => current && ({ ...current, profiles: current.profiles.filter((candidate) => candidate.id !== profile.id) }))
      setProfile(newDatabaseProfile()); setPassword(""); setCredentialVersion((n) => n + 1)
    } catch (error) { if (live.current) setFailure(databaseError(error, password)) }
    finally { if (live.current) setBusy(null) }
  }
  const execute = async (kind: "test" | "schema" | "load") => {
    if (busy || connectionError || !driver?.available || (kind === "load" && (!sql.trim() || options.error))) return
    const ticket = ++sequence.current
    setFailure(""); setStopping(false); setBusy(kind)
    if (kind === "test") setTested(null)
    if (kind === "load") setReply(null)
    const operation = databaseJob<DatabasePreview | DatabaseSchema | { ok: boolean }>(kind === "load" ? "database_prepare" : kind === "schema" ? "database_schema" : "database_test", profile, password, kind === "load" ? { sql, ...options } : {})
    work.current = operation
    try {
      const { data } = await operation.promise
      if (!live.current || sequence.current !== ticket) return
      if (kind === "load") setReply({ key: sourceKey, data: data as DatabasePreview, start: 0 })
      else if (kind === "schema") setSchemaReply({ key: connectionKey, data: data as DatabaseSchema })
      else if ((data as { ok: boolean }).ok) setTested(connectionKey)
    } catch (error) {
      if (live.current && sequence.current === ticket) setFailure((error as Error).message === "Cancelled" ? "Operation cancelled. No project was created." : databaseError(error, password))
    } finally {
      if (work.current === operation) work.current = null
      if (live.current && sequence.current === ticket) setBusy(null)
    }
  }
  const page = async (start: number) => {
    if (!preview || busy) return
    const ticket = ++sequence.current
    setBusy("paging"); setFailure("")
    try {
      const { data } = await call<DatabasePreview>("database_preview", { id: preview.data.id, start, limit: PAGE })
      if (live.current && sequence.current === ticket) setReply({ key: sourceKey, data, start })
    } catch (error) { if (live.current && sequence.current === ticket) setFailure(databaseError(error, password)) }
    finally { if (live.current && sequence.current === ticket) setBusy(null) }
  }
  const create = async () => {
    if (!canCreate || !preview) return
    const ticket = ++sequence.current
    setBusy("create"); setFailure("")
    try { await onCreate({ id: preview.data.id, name: name.trim() }) }
    catch (error) { if (live.current && sequence.current === ticket) setFailure(databaseError(error, password)) }
    finally { if (live.current && sequence.current === ticket) setBusy(null) }
  }
  const input = (field: "name" | "host" | "database" | "username" | "sqlitePath", label: string, help?: string) => <label className="flex flex-col gap-1 px-4 py-1.5 text-xs"><span>{label}</span><Input aria-label={label} value={profile[field]} disabled={locked} onChange={(event) => changeProfile({ [field]: event.target.value })} className="h-8" />{help && <span className="text-muted-foreground">{help}</span>}</label>

  return (
    <FullView root="Projects" crumbs={["Database import"]} closeLabel={busy === "create" ? "Creating…" : "Cancel"} onClose={close}>
      <SummaryStrip>
        <label className="flex w-80 min-w-0 flex-col justify-center gap-1 border-r px-5 py-3"><span className="text-[11.5px] text-muted-foreground">Project name</span><Input aria-label="Database project name" value={name} disabled={locked} onChange={(event) => setName(event.target.value)} /></label>
        <Metric label="rows">{preview ? preview.data.total.toLocaleString() : "—"}</Metric><Spacer />
        <StripAction ink disabled={!canCreate} onClick={create}>{busy === "create" && <Spinner />}Create project</StripAction>
      </SummaryStrip>
      <div className="flex min-h-0 flex-1">
        <aside aria-label="Database connection" className="w-80 shrink-0 overflow-y-auto border-r pb-5">
          <RailLabel>Saved connections</RailLabel>
          <div className="px-4"><select aria-label="Saved connection" value={profile.id} disabled={locked || !catalog} onChange={(event) => selectProfile(event.target.value)} className="h-8 w-full rounded-[3px] border border-input bg-background px-2 text-sm"><option value="">New connection</option>{catalog?.profiles.map((candidate) => <option key={candidate.id} value={candidate.id}>{candidate.name}</option>)}</select></div>
          <div className="mt-2 flex h-9 border-y"><CellButton disabled={!!busy} onClick={reloadProfiles} className="flex-1 justify-center text-xs">{profile.id ? "Reload saved profile" : "Refresh profiles"}</CellButton><CellButton disabled={locked} onClick={() => selectProfile("")} className="flex-1 justify-center text-xs">New connection</CellButton></div>
          {profile.id && <p className="px-4 pt-2 text-xs text-muted-foreground">Reloading replaces unsaved edits to this connection.</p>}
          {input("name", "Connection name")}
          <label className="flex flex-col gap-1 px-4 py-1.5 text-xs"><span>Database driver</span><select aria-label="Database driver" value={profile.driver} disabled={locked || !catalog} onChange={(event) => { const next = event.target.value as DatabaseDriver; changeProfile({ driver: next, port: next === "postgres" ? 5432 : 3306 }); setPassword(""); setCredentialVersion((n) => n + 1) }} className="h-8 rounded-[3px] border border-input bg-background px-2 text-sm">{catalog?.drivers.map((candidate) => <option key={candidate.id} value={candidate.id} disabled={!candidate.available}>{candidate.label}{!candidate.available ? " — unavailable in this build" : ""}</option>)}</select></label>
          {catalog && !driver?.available && <p role="alert" className="px-4 py-2 text-xs text-destructive">{driver?.reason ?? "This database driver is unavailable in this build."}</p>}
          {profile.driver === "sqlite" ? input("sqlitePath", "SQLite file path", "A path on the machine running OxRefine.") : <>
            {input("host", "Host")}
            <label className="flex flex-col gap-1 px-4 py-1.5 text-xs"><span>Port</span><Input aria-label="Database port" type="number" min={1} max={65535} value={profile.port} disabled={locked} onChange={(event) => changeProfile({ port: Number(event.target.value) })} className="h-8 font-mono" /></label>
            {input("database", "Database name")}{input("username", "Database user")}
            <label className="flex flex-col gap-1 px-4 py-1.5 text-xs"><span>Password</span><Input aria-label="Database password" type="password" autoComplete="off" value={password} disabled={locked} onChange={(event) => { invalidate(true); setPassword(event.target.value); setCredentialVersion((n) => n + 1) }} className="h-8" /><span className="text-muted-foreground">Used for this open import view only. Passwords are not saved with profiles or projects.</span></label>
            <label className="flex flex-col gap-1 px-4 py-1.5 text-xs"><span>Transport security</span><select aria-label="Database transport security" value={profile.tls} disabled={locked} onChange={(event) => changeProfile({ tls: event.target.value as DatabaseProfile["tls"] })} className="h-8 rounded-[3px] border border-input bg-background px-2 text-sm"><option value="verify-full">TLS: verify certificate and host</option><option value="disable">No TLS</option></select></label>
          </>}
          <div className="mt-3 flex h-10 border-y"><CellButton disabled={!!busy || !!connectionError || !profile.name.trim() || !driver?.available} onClick={saveProfile} className="flex-1 justify-center text-xs">{busy === "save" ? "Saving…" : "Save profile"}</CellButton><CellButton disabled={!!busy || !profile.id} onClick={() => setDeleting(true)} className="text-xs">Delete profile</CellButton></div>
          {deleting && <div className="border-b px-4 py-3 text-xs"><p>Delete saved connection “{profile.name}”?</p><div className="mt-2 flex gap-2"><button type="button" className="underline underline-offset-4" onClick={deleteProfile}>Delete saved profile</button><button type="button" className="underline underline-offset-4" onClick={() => setDeleting(false)}>Keep profile</button></div></div>}
          <div className="flex h-10 border-b"><CellButton disabled={!!busy || !!connectionError || !driver?.available} onClick={() => execute("test")} className="flex-1 justify-center text-xs">Test connection</CellButton><CellButton disabled={!!busy || !!connectionError || !driver?.available} onClick={() => execute("schema")} className="flex-1 justify-center text-xs">Browse schema</CellButton></div>
          {connectionError && <p className="px-4 pt-2 text-xs text-muted-foreground">{connectionError}</p>}
          {tested === connectionKey && <p role="status" className="px-4 pt-2 text-xs">Connection test succeeded.</p>}
          {schema && <><RailLabel>Tables and views</RailLabel>{!schema.tables.length ? <p className="px-4 text-xs text-muted-foreground">No tables or views are visible to this user.</p> : schema.tables.map((table, index) => <details key={index} className="border-b px-4 py-2 text-xs"><summary className="cursor-pointer break-all font-medium">{[table.schema, table.name].filter(Boolean).join(".")}</summary><ul className="mt-2 space-y-1 text-muted-foreground">{table.columns.map((column, i) => <li key={i} className="break-all">{column.name} · {column.type}{column.nullable ? " · nullable" : ""}</li>)}</ul><button type="button" disabled={locked} onClick={() => { invalidate(); setSql(databaseTableQuery(table, profile.driver)) }} className="mt-2 underline underline-offset-4">Use SELECT for this table</button></details>)}</>}
        </aside>
        <main className="flex min-w-0 flex-1 flex-col">
          <div className="shrink-0 border-b px-5 py-4">
            <label htmlFor="database-sql" className="mb-2 block font-medium">SQL query</label>
            <Textarea id="database-sql" aria-label="SQL query" value={sql} disabled={locked} spellCheck={false} placeholder="SELECT * FROM …" onChange={(event) => { invalidate(); setSql(event.target.value) }} onKeyDown={(event) => { if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) { event.preventDefault(); void execute("load") } }} className="field-sizing-fixed h-32 font-mono text-[12.5px]" />
            <div className="mt-3 flex flex-wrap items-end gap-4"><label className="flex flex-col gap-1 text-xs"><span>Maximum rows</span><Input aria-label="Maximum database rows" type="number" min={1} max={1000000} value={maxRows} disabled={locked} onChange={(event) => { invalidate(); setMaxRows(event.target.value) }} className="h-8 w-30 font-mono" /></label><label className="flex flex-col gap-1 text-xs"><span>Timeout in seconds</span><Input aria-label="Database timeout" type="number" min={1} max={120} value={timeout} disabled={locked} onChange={(event) => { invalidate(); setTimeout(event.target.value) }} className="h-8 w-30 font-mono" /></label><span className="pb-2 text-xs text-muted-foreground">Ctrl ↵ loads the result</span></div>
            {options.error && <p role="alert" className="mt-2 text-xs text-destructive">{options.error}</p>}
          </div>
          <div className="flex h-11 shrink-0 border-b"><CellButton ink disabled={!!busy || !!connectionError || !driver?.available || !sql.trim() || !!options.error} onClick={() => execute("load")}>{busy === "load" && <Spinner />}Load query result</CellButton><span role="status" className="flex min-w-0 items-center px-4 text-xs text-muted-foreground">{stopping && running ? "Stop requested; waiting for a cancellation check…" : busy === "test" ? "Testing connection…" : busy === "schema" ? "Reading database schema…" : busy === "load" ? "Running the query and loading its result…" : busy === "paging" ? "Loading preview rows…" : busy === "create" ? "Saving the prepared result…" : preview ? `${preview.data.total.toLocaleString()} rows ready` : "Load a result before creating a project"}</span><Spacer />{running && <CellButton rule="left" disabled={stopping} onClick={() => { setStopping(true); work.current?.stop() }}>{stopping ? "Stopping…" : "Stop"}</CellButton>}</div>
          {failure && <p role="alert" className="shrink-0 border-b px-5 py-3 text-sm text-destructive">{failure}</p>}
          {preview?.data.warnings?.map((warning, index) => <p key={index} role="status" className="border-b px-5 py-2 text-xs text-warn-text">{warning}</p>)}
          {!preview ? <p className="max-w-2xl px-5 py-6 text-sm text-muted-foreground">Choose a connection, browse its schema or enter SQL, then load the query result. Loading leaves your open project selected. Only Create project opens the prepared result.</p> : !preview.data.total ? <p className="px-5 py-6 text-sm text-muted-foreground">The query returned no rows. Edit the SQL and load again.</p> : <>
            <div className="min-h-0 flex-1 overflow-auto"><table className="border-collapse text-sm"><thead><tr>{preview.data.columns.map((column, index) => <th key={index} scope="col" className="sticky top-0 border-r border-b bg-background px-4 py-2 text-left font-medium"><span>{column}</span><span className="ml-2 text-xs font-normal text-muted-foreground">{preview.data.columnTypes[index]}</span></th>)}</tr></thead><tbody>{preview.data.rows.map(([index, cells]) => <tr key={index}>{preview.data.columns.map((_, column) => <td key={column} className="max-w-96 border-r border-b px-4 py-2"><span className="block truncate" title={cells[column] == null ? undefined : String(cells[column])}>{cells[column] == null ? <span className="text-muted-foreground">(null)</span> : String(cells[column])}</span></td>)}</tr>)}</tbody></table></div>
            <div className="flex h-11 shrink-0 items-center border-t"><span className="px-4 font-mono text-xs text-muted-foreground">{preview.start + 1}–{Math.min(preview.start + preview.data.rows.length, preview.data.total)} / {preview.data.total.toLocaleString()}</span><Spacer /><CellButton rule="left" disabled={!!busy || !preview.start} onClick={() => page(Math.max(0, preview.start - PAGE))}>Previous</CellButton><CellButton rule="left" disabled={!!busy || preview.start + preview.data.rows.length >= preview.data.total} onClick={() => page(preview.start + PAGE)}>Next</CellButton></div>
          </>}
          <p className="mt-auto shrink-0 border-t px-5 py-2 text-xs text-muted-foreground">Changing the connection or query requires a new load. Results are kept for up to 30 minutes, within the server's row and memory limits. Create uses the prepared snapshot without running SQL again.</p>
        </main>
      </div>
    </FullView>
  )
}
