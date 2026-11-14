import { useEffect, useRef, useState } from "react"
import { call } from "@/api"
import { commonsSource, prepareCommons, type CommonsPreview } from "@/commonsImport"
import { CellButton, FullView, Metric, RailLabel, Spacer, StripAction, SummaryStrip } from "@/components/instrument"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Spinner } from "@/components/ui/spinner"
import { toast } from "@/components/ui/toast"

const PAGE = 20
const requestId = () => globalThis.crypto?.randomUUID?.() ?? `commons-${Date.now()}-${Math.random().toString(36).slice(2)}`

export function CommonsImportDialog({ onCreate, onClose }: {
  onCreate: (args: { id: string; name: string }) => Promise<void>
  onClose: () => void
}) {
  const [titles, setTitles] = useState("")
  const [depth, setDepth] = useState("0")
  const [wikitext, setWikitext] = useState(false)
  const [name, setName] = useState("Commons import")
  const [reply, setReply] = useState<{ key: string; data: CommonsPreview; start: number } | null>(null)
  const [failure, setFailure] = useState<{ key: string; text: string } | null>(null)
  const [busy, setBusy] = useState<"loading" | "paging" | "creating" | null>(null)
  const [stopping, setStopping] = useState(false)
  const live = useRef(true)
  const sequence = useRef(0)
  const work = useRef<ReturnType<typeof prepareCommons> | null>(null)
  const parsed = commonsSource(titles, depth)
  const key = JSON.stringify({ titles: titles.trim(), depth, wikitext })
  const preview = reply?.key === key ? reply : null
  const error = failure?.key === key ? failure.text : ""
  const creating = busy === "creating"
  const canCreate = !!preview?.data.total && !!name.trim() && !busy

  useEffect(() => {
    live.current = true
    return () => { live.current = false; work.current?.stop() }
  }, [])
  const close = () => {
    if (creating) return
    live.current = false
    sequence.current++
    work.current?.stop()
    if (busy === "loading") toast.add({ title: "Commons preview closed", description: "A stop was requested for this load. It may continue until the next cancellation check. No project was created." })
    onClose()
  }
  const browse = async () => {
    if (!parsed.categories || busy) return
    const ticket = ++sequence.current
    setReply(null); setFailure(null); setStopping(false); setBusy("loading")
    const loading = prepareCommons(parsed.categories, wikitext, requestId())
    work.current = loading
    try {
      const { data } = await loading.promise
      if (live.current && sequence.current === ticket) setReply({ key, data, start: 0 })
    } catch (e) {
      if (live.current && sequence.current === ticket) setFailure({ key, text: (e as Error).message === "Cancelled" ? "Loading cancelled. No project was created." : String((e as Error).message ?? e) })
    } finally {
      if (work.current === loading) work.current = null
      if (live.current && sequence.current === ticket) setBusy(null)
    }
  }
  const page = async (start: number) => {
    if (!preview || busy) return
    const ticket = ++sequence.current
    setFailure(null); setBusy("paging")
    try {
      const { data } = await call<CommonsPreview>("commons_preview", { id: preview.data.id, start, limit: PAGE })
      if (live.current && sequence.current === ticket) setReply({ key, data, start })
    } catch (e) {
      if (live.current && sequence.current === ticket) setFailure({ key, text: String((e as Error).message ?? e) })
    } finally { if (live.current && sequence.current === ticket) setBusy(null) }
  }
  const create = async () => {
    if (!canCreate || !preview) return
    const ticket = ++sequence.current
    setFailure(null); setBusy("creating")
    try { await onCreate({ id: preview.data.id, name: name.trim() }) }
    catch (e) { if (live.current && sequence.current === ticket) setFailure({ key, text: String((e as Error).message ?? e) }) }
    finally { if (live.current && sequence.current === ticket) setBusy(null) }
  }

  return (
    <FullView root="Projects" crumbs={["Wikimedia Commons import"]} closeLabel={creating ? "Creating…" : "Cancel"} onClose={close}>
      <SummaryStrip>
        <label className="flex w-80 min-w-0 flex-col justify-center gap-1 border-r px-5 py-3">
          <span className="text-[11.5px] text-muted-foreground">Project name</span>
          <Input aria-label="Commons project name" value={name} disabled={creating} onChange={(event) => setName(event.target.value)} />
        </label>
        <Metric label="files">{preview ? preview.data.total.toLocaleString() : "—"}</Metric>
        <Spacer />
        <StripAction ink disabled={!canCreate} onClick={create}>{creating && <Spinner />}Create project</StripAction>
      </SummaryStrip>
      <div className="flex min-h-0 flex-1">
        <aside aria-label="Commons import options" className="w-80 shrink-0 overflow-y-auto border-r pb-5">
          <RailLabel>Categories</RailLabel>
          <div className="flex min-w-0 flex-col gap-2 px-4">
            <label className="text-xs text-muted-foreground" htmlFor="commons-categories">One Commons category title per line</label>
            <Textarea id="commons-categories" className="field-sizing-fixed h-36 min-w-0" placeholder="Category:Your category" value={titles} disabled={creating} onChange={(event) => setTitles(event.target.value)} />
            <p className="text-xs text-muted-foreground">The Category: prefix is optional. Commas and underscores stay in the title.</p>
            <label className="mt-2 flex flex-col gap-1 text-xs" htmlFor="commons-depth">Subcategory depth for all listed categories</label>
            <Input id="commons-depth" aria-label="Commons subcategory depth" type="number" min={0} max={10} step={1} value={depth} disabled={creating} onChange={(event) => setDepth(event.target.value)} className="w-24 font-mono" />
            <p className="text-xs text-muted-foreground">0 includes only files in the categories themselves. 1 also includes their direct subcategories.</p>
            <label className="mt-3 flex items-start gap-2 text-xs"><input type="checkbox" checked={wikitext} disabled={creating} onChange={(event) => setWikitext(event.target.checked)} className="mt-0.5 accent-foreground" />Include file-page wikitext</label>
            <p className="text-xs text-muted-foreground">Filename cells are matched to Commons M-ids. Media files stay on Commons.</p>
          </div>
          <RailLabel>Limits</RailLabel>
          <p className="px-4 text-xs leading-relaxed text-muted-foreground">Up to 20 category roots, depth 10 and 10,000 files. Exceeding a limit fails the load instead of importing a partial result.</p>
          <div className="mt-4 border-y">
            <CellButton ink onClick={browse} disabled={!!parsed.error || !!busy} className="w-full justify-center py-3">{busy === "loading" && <Spinner />}Browse and preview</CellButton>
          </div>
          {parsed.error && !!titles && <p role="alert" className="px-4 pt-3 text-xs text-destructive">{parsed.error}</p>}
        </aside>
        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex h-10 shrink-0 items-center gap-3 border-b px-4">
            <span className="font-semibold">Preview</span>
            <span role="status" className="text-xs text-muted-foreground">{busy === "loading" ? stopping ? "Stop requested; waiting for a cancellation check…" : "Loading category members from Commons…" : busy === "paging" ? "Loading preview rows…" : creating ? "Saving the project; wait for the result. Cancel in Jobs is cooperative and cannot undo a committed save." : preview ? `${preview.data.total.toLocaleString()} files ready` : "Browse before creating a project"}</span>
            <Spacer />
            {busy === "loading" && <CellButton rule="left" disabled={stopping} onClick={() => { setStopping(true); work.current?.stop() }}>{stopping ? "Stopping…" : "Stop loading"}</CellButton>}
          </div>
          {error && <p role="alert" className="border-b px-4 py-3 text-sm text-destructive">{error}</p>}
          {preview?.data.warnings?.map((warning, index) => <p key={index} role="status" className="border-b px-4 py-2 text-xs text-warn-text">{warning}</p>)}
          {!preview ? (
            <p className="max-w-2xl px-5 py-6 text-sm text-muted-foreground">{busy === "loading" ? "This can take time for deep categories or when Commons limits requests. Progress and cancellation are available in Jobs." : "Enter category titles and browse to load a preview. Your open project stays selected until you explicitly create the new one."}</p>
          ) : !preview.data.total ? (
            <p className="px-5 py-6 text-sm text-muted-foreground">No files were found at this depth. Change the categories or depth and browse again.</p>
          ) : (
            <>
              <div className="min-h-0 flex-1 overflow-auto">
                <table className="w-full border-collapse text-sm"><thead><tr>{preview.data.columns.map((column, index) => <th key={index} scope="col" className="sticky top-0 border-r border-b bg-background px-4 py-2 text-left font-medium">{column}</th>)}</tr></thead>
                  <tbody>{preview.data.rows.map((row) => <tr key={row[0]}>{preview.data.columns.map((_, column) => {
                    const value = row[1][column]
                    const match = row[6]?.[String(column)]?.m
                    return <td key={column} className="max-w-3xl border-r border-b px-4 py-2 align-top"><span className="block max-w-[75ch] truncate" title={value ?? undefined}>{value ?? "(blank)"}</span>{match?.id && <a className="mt-1 inline-block font-mono text-xs underline" href={`https://commons.wikimedia.org/entity/${encodeURIComponent(match.id)}`} target="_blank" rel="noreferrer">{match.id}</a>}</td>
                  })}</tr>)}</tbody>
                </table>
              </div>
              <div className="flex h-11 shrink-0 items-center border-t">
                <span className="px-4 font-mono text-xs text-muted-foreground">{preview.start + 1}–{Math.min(preview.start + preview.data.rows.length, preview.data.total)} / {preview.data.total.toLocaleString()}</span>
                <Spacer /><CellButton rule="left" disabled={!!busy || !preview.start} onClick={() => page(Math.max(0, preview.start - PAGE))}>Previous</CellButton><CellButton rule="left" disabled={!!busy || preview.start + preview.data.rows.length >= preview.data.total} onClick={() => page(preview.start + PAGE)}>Next</CellButton>
              </div>
            </>
          )}
          <p className="shrink-0 border-t px-4 py-2 text-xs text-muted-foreground">Changing source options requires a new browse. The prepared snapshot is kept for up to 30 minutes; creation uses that snapshot without fetching Commons again.</p>
        </main>
      </div>
    </FullView>
  )
}
