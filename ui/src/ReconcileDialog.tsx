import { useEffect, useState } from "react"
import { PlusIcon } from "lucide-react"
import { call, type Engine, type Op } from "@/api"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Spinner } from "@/components/ui/spinner"
import { SuggestBox } from "@/ReconPanel"
import { FullView, Metric, RailLabel, Spacer, StripAction, SummaryStrip, Tag } from "@/components/instrument"
import { cn } from "@/lib/utils"

type Service = { url: string; name: string }
type Manifest = { name?: string; identifierSpace: string; schemaSpace: string; defaultTypes?: { id: string; name: string }[] }
type Guess = { id: string; name: string; score: number; count: number }

/** OpenRefine's preset service, then any the user added (kept in this browser profile). */
const WIKIDATA: Service = { url: "https://wikidata.reconci.link/en/api", name: "Wikidata (en)" }
const KEY = "ox-recon-services"
export function loadServices(): Service[] {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "[]") as Service[]
    return [WIKIDATA, ...saved.filter((s) => s.url !== WIKIDATA.url)]
  } catch {
    return [WIKIDATA]
  }
}
export function saveServices(list: Service[]) {
  try {
    localStorage.setItem(KEY, JSON.stringify(list.filter((s) => s.url !== WIKIDATA.url)))
  } catch {
    // private mode: the list lasts for this session only
  }
}

const NONE = "\u0000none"
const CUSTOM = "\u0000custom"

/** OpenRefine's "Start reconciling": a service, a type, other columns as properties. */
export function ReconcileDialog({
  column,
  columns,
  engine,
  onApply,
  onClose,
}: {
  column: string
  columns: string[]
  engine: Engine
  onApply: (op: Op) => Promise<void>
  onClose: () => void
}) {
  const [services, setServices] = useState(loadServices)
  const [url, setUrl] = useState(services[0].url)
  const [adding, setAdding] = useState("")
  const [manifest, setManifest] = useState<Manifest | null>(null)
  const [guesses, setGuesses] = useState<Guess[] | null>(null)
  const [type, setType] = useState("")
  const [custom, setCustom] = useState("")
  const [autoMatch, setAutoMatch] = useState(true)
  const [limit, setLimit] = useState("")
  const [props, setProps] = useState<Record<string, string>>({})
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)

  // the service's manifest, then the types its answers suggest for this column
  useEffect(() => {
    let live = true
    setManifest(null)
    setGuesses(null)
    setError("")
    call<Manifest>("recon_service", { url }, { quiet: true })
      .then(({ data }) => {
        if (!live) return
        setManifest(data)
        return call<Guess[]>("recon_guess_types", { column, url }, { quiet: true }).then(({ data: g }) => {
          if (!live) return
          setGuesses(g)
          setType((t) => t || g[0]?.id || data.defaultTypes?.[0]?.id || NONE)
        })
      })
      .catch((e) => live && setError(String((e as Error).message ?? e)))
    return () => {
      live = false
    }
  }, [url, column])

  const add = async () => {
    const u = adding.trim()
    if (!u) return
    setError("")
    try {
      const { data } = await call<Manifest>("recon_service", { url: u })
      const list = [...services.filter((s) => s.url !== u), { url: u, name: data.name || u }]
      setServices(list)
      saveServices(list)
      setUrl(u)
      setAdding("")
    } catch (e) {
      setError(String((e as Error).message ?? e))
    }
  }

  // the type choices: guessed first (as OpenRefine lists them), then the service's defaults
  const choices = [...(guesses ?? []).slice(0, 6).map((g) => ({ id: g.id, name: g.name, hint: `${g.count} of the sample` }))]
  for (const t of manifest?.defaultTypes ?? []) if (!choices.some((c) => c.id === t.id)) choices.push({ id: t.id, name: t.name, hint: "suggested by the service" })
  const chosen = type === CUSTOM ? (custom.trim() ? { id: custom.trim(), name: custom.trim() } : null) : type === NONE ? null : (choices.find((c) => c.id === type) ?? null)
  const blocked = !manifest || busy || (type === CUSTOM && !custom.trim())

  const apply = async () => {
    if (!manifest) return
    setBusy(true)
    const op: Op = {
      op: "core/recon",
      engineConfig: engine,
      columnName: column,
      config: {
        mode: "standard-service",
        service: url,
        identifierSpace: manifest.identifierSpace,
        schemaSpace: manifest.schemaSpace,
        type: chosen && { id: chosen.id, name: chosen.name },
        autoMatch,
        columnDetails: Object.entries(props)
          .filter(([, p]) => p.trim())
          .map(([c, p]) => ({ column: c, propertyName: p.trim(), propertyID: p.trim() })),
        limit: Math.max(0, Number(limit) || 0),
      },
    }
    await onApply(op).then(onClose, (e) => setError(String((e as Error).message ?? e)))
    setBusy(false)
  }

  const others = columns.filter((c) => c !== column)
  const service = services.find((x) => x.url === url)
  const row = "flex cursor-pointer items-center gap-2.5 border-b border-row-rule px-5 py-2.5 hover:bg-accent"
  return (
    <FullView crumbs={["Reconcile", <Tag key="c">{column}</Tag>]} onClose={onClose}>
      <div
        className="flex min-h-0 flex-1 flex-col"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !blocked) {
            e.preventDefault()
            apply()
          }
        }}
      >
        <SummaryStrip>
          <Metric label="service" className="[&>span:last-child]:font-sans [&>span:last-child]:text-[15px]">
            {service?.name ?? url}
          </Metric>
          <Metric label="type" className="[&>span:last-child]:font-sans [&>span:last-child]:text-[15px]">
            {chosen ? (
              <>
                {chosen.name} <span className="font-mono text-[12px] text-muted-foreground">{chosen.id}</span>
              </>
            ) : type === NONE ? (
              <span className="text-muted-foreground">any</span>
            ) : (
              "…"
            )}
          </Metric>
          <Spacer />
          <StripAction ink keys="Ctrl ↵" disabled={blocked} onClick={apply}>
            {busy && <Spinner />}
            Start reconciling
          </StripAction>
        </SummaryStrip>
        <div className="flex min-h-0 flex-1">
          <aside aria-label="Service and options" className="w-75 shrink-0 overflow-y-auto border-r pb-4">
            <RailLabel>Service</RailLabel>
            <div role="radiogroup" aria-label="Service">
              {services.map((x) => (
                <label key={x.url} className={cn("flex cursor-pointer items-center gap-2.5 px-3.5 py-1.5 hover:bg-accent", x.url === url && "bg-secondary")}>
                  <input type="radio" name="rc-service" checked={x.url === url} onChange={() => setUrl(x.url)} className="accent-foreground" />
                  <span className="min-w-0 flex-1 truncate" title={x.url}>
                    {x.name}
                  </span>
                </label>
              ))}
            </div>
            <form
              className="flex gap-1.5 px-3.5 pt-2"
              onSubmit={(e) => {
                e.preventDefault()
                add()
              }}
            >
              <Input aria-label="Add a service by its URL" placeholder="Add a service: https://…" value={adding} onChange={(e) => setAdding(e.target.value)} className="h-7.5" />
              <Button type="submit" variant="outline" size="sm" disabled={!adding.trim()}>
                <PlusIcon data-icon="inline-start" />
                Add
              </Button>
            </form>
            <div className="mt-3 border-t" />
            <RailLabel>Options</RailLabel>
            <label className="flex cursor-pointer items-start gap-2.5 px-3.5 py-1.5">
              <input type="checkbox" checked={autoMatch} onChange={(e) => setAutoMatch(e.target.checked)} className="mt-0.5 accent-foreground" />
              Match automatically when the service is confident
            </label>
            <label className="flex items-center gap-2 px-3.5 py-1.5">
              <span className="min-w-0 flex-1">Candidates per cell</span>
              <input
                type="number"
                min={0}
                placeholder="all"
                value={limit}
                onChange={(e) => setLimit(e.target.value)}
                className="h-7.5 w-20 rounded-[3px] border border-input bg-background px-2 text-right font-mono text-[12.5px] outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
              />
            </label>
            <p className="px-3.5 pt-3 text-xs text-muted-foreground">Cells with the same text are asked once. Review results by value or judge individual cells in the grid.</p>
          </aside>
          <div className="min-w-0 flex-1 overflow-y-auto">
            {error && (
              <p role="alert" className="border-b px-5 py-3 text-destructive">
                {error}
              </p>
            )}
            <div className="flex h-10 items-center border-b border-b-foreground px-5">
              <span className="font-semibold">Reconcile against type</span>
            </div>
            {!manifest || !guesses ? (
              !error && (
                <p className="flex items-center gap-2 px-5 py-4 text-muted-foreground">
                  <Spinner />
                  Asking the service about a sample of this column…
                </p>
              )
            ) : (
              <div role="radiogroup" aria-label="Type">
                {choices.map((c) => (
                  <label key={c.id} className={cn(row, type === c.id && "bg-secondary")}>
                    <input type="radio" name="rc-type" checked={type === c.id} onChange={() => setType(c.id)} className="accent-foreground" />
                    <span className="flex-1">
                      {c.name} <span className="font-mono text-[11.5px] text-muted-foreground">{c.id}</span>
                    </span>
                    <span className="text-xs text-muted-foreground">{c.hint}</span>
                  </label>
                ))}
                <label className={cn(row, type === CUSTOM && "bg-secondary")}>
                  <input type="radio" name="rc-type" checked={type === CUSTOM} onChange={() => setType(CUSTOM)} className="accent-foreground" />
                  <span>Another type</span>
                  <Input aria-label="Type id" placeholder="Type id, like Q5" value={custom} onChange={(e) => (setCustom(e.target.value), setType(CUSTOM))} className="h-7.5 max-w-48 font-mono" />
                </label>
                {"suggest" in (manifest ?? {}) && (
                  <div className="border-b border-row-rule px-5 py-2.5">
                    <SuggestBox service={url} kind="type" placeholder="Search for a type" onPick={(x) => (setCustom(x.id), setType(CUSTOM))} />
                  </div>
                )}
                <label className={cn(row, type === NONE && "bg-secondary")}>
                  <input type="radio" name="rc-type" checked={type === NONE} onChange={() => setType(NONE)} className="accent-foreground" />
                  No particular type
                </label>
              </div>
            )}
            {others.length > 0 && (
              <>
                <div className="mt-6 flex h-10 items-center gap-3 border-b border-b-foreground px-5">
                  <span className="font-semibold">Also use other columns</span>
                  <span className="text-xs text-muted-foreground">a property id (like P17 for country) sends that column's cell with each query</span>
                </div>
                <table className="w-full border-collapse">
                  <thead>
                    <tr className="text-left text-[11.5px] text-muted-foreground">
                      <th scope="col" className="border-b px-5 py-2 font-normal">
                        column
                      </th>
                      <th scope="col" className="border-b px-5 py-2 font-normal">
                        as property
                      </th>
                    </tr>
                  </thead>
                  <tbody>
                    {others.map((c) => (
                      <tr key={c}>
                        <td className="border-b border-row-rule px-5 py-1.5">{c}</td>
                        <td className="border-b border-row-rule px-5 py-1.5">
                          <Input aria-label={`Property for ${c}`} placeholder="—" value={props[c] ?? ""} onChange={(e) => setProps((p) => ({ ...p, [c]: e.target.value }))} className="h-7.5 max-w-48 font-mono" />
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </>
            )}
          </div>
        </div>
      </div>
    </FullView>
  )
}

type Proposal = { properties: { id: string; name: string }[]; manifest: Manifest; service: string }

/** OpenRefine's "Add columns from reconciled values": properties of the matched items as new columns. */
export function ExtendDialog({
  column,
  columns,
  engine,
  onApply,
  onClose,
}: {
  column: string
  columns: string[]
  engine: Engine
  onApply: (op: Op) => Promise<void>
  onClose: () => void
}) {
  const [proposal, setProposal] = useState<Proposal | null>(null)
  const [chosen, setChosen] = useState<string[]>([])
  const [more, setMore] = useState("")
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    call<Proposal>("recon_propose", { column }, { quiet: true }).then(({ data }) => setProposal(data), (e) => setError(String((e as Error).message ?? e)))
  }, [column])

  const extra = more.split(",").map((s) => s.trim()).filter(Boolean)
  const ids = [...chosen, ...extra.filter((x) => !chosen.includes(x))]
  const nameOf = (id: string) => proposal?.properties.find((p) => p.id === id)?.name ?? id
  const apply = async () => {
    if (!proposal) return
    setBusy(true)
    await onApply({
      op: "core/extend-reconciled-data",
      engineConfig: engine,
      baseColumnName: column,
      endpoint: proposal.service,
      identifierSpace: proposal.manifest.identifierSpace,
      schemaSpace: proposal.manifest.schemaSpace,
      extension: { properties: ids.map((id) => ({ id, name: nameOf(id) })) },
      columnInsertIndex: columns.indexOf(column) + 1,
    }).then(onClose, (e) => setError(String((e as Error).message ?? e)))
    setBusy(false)
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Add columns from the items matched in “{column}”</DialogTitle>
          <DialogDescription>Each property becomes a column after this one; items with several values get extra rows in their record.</DialogDescription>
        </DialogHeader>
        <ScrollArea className="max-h-[60vh]">
          <FieldGroup className="pr-3">
            <FieldSet>
              <FieldLegend variant="label">Properties the service suggests</FieldLegend>
              {!proposal && !error && (
                <FieldDescription className="flex items-center gap-2">
                  <Spinner />
                  Asking the service…
                </FieldDescription>
              )}
              {proposal?.properties.length === 0 && <FieldDescription>The service suggests none; type their ids below.</FieldDescription>}
              {proposal?.properties.map((p) => (
                <Field key={p.id} orientation="horizontal">
                  <Checkbox id={`ext-${p.id}`} checked={chosen.includes(p.id)} onCheckedChange={(v) => setChosen((c) => (v ? [...c, p.id] : c.filter((x) => x !== p.id)))} />
                  <FieldLabel htmlFor={`ext-${p.id}`} className="font-normal">
                    {p.name} <span className="text-muted-foreground">({p.id})</span>
                  </FieldLabel>
                </Field>
              ))}
            </FieldSet>
            <Field>
              <FieldLabel htmlFor="ext-more">Other properties</FieldLabel>
              <Input id="ext-more" placeholder="Property ids, separated by commas (P31, P625)" value={more} onChange={(e) => setMore(e.target.value)} />
            </Field>
            {error && <FieldError>{error}</FieldError>}
          </FieldGroup>
        </ScrollArea>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={apply} disabled={!proposal || ids.length === 0 || busy}>
            {busy && <Spinner data-icon="inline-start" />}
            Add {ids.length || ""} column{ids.length === 1 ? "" : "s"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
