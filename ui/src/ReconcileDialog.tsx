import { useEffect, useState } from "react"
import { PlusIcon } from "lucide-react"
import { call, type Engine, type Op } from "@/api"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { InputGroup, InputGroupAddon, InputGroupButton, InputGroupInput } from "@/components/ui/input-group"
import { Input } from "@/components/ui/input"
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

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
function saveServices(list: Service[]) {
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
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Reconcile “{column}”</DialogTitle>
          <DialogDescription>Match each cell to an entity of a reconciliation service. Cells with the same text are asked once.</DialogDescription>
        </DialogHeader>
        <ScrollArea className="max-h-[65vh]">
          <FieldGroup className="pr-3">
            <Field>
              <FieldLabel>Service</FieldLabel>
              <Select value={url} onValueChange={(v) => v && setUrl(String(v))}>
                <SelectTrigger aria-label="Service">
                  <SelectValue>{(v: string) => services.find((s) => s.url === v)?.name ?? v}</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  <SelectGroup>
                    {services.map((s) => (
                      <SelectItem key={s.url} value={s.url}>
                        {s.name}
                      </SelectItem>
                    ))}
                  </SelectGroup>
                </SelectContent>
              </Select>
              <InputGroup>
                <InputGroupInput aria-label="Add a service by its URL" placeholder="Add a service: https://…" value={adding} onChange={(e) => setAdding(e.target.value)} onKeyDown={(e) => e.key === "Enter" && add()} />
                <InputGroupAddon align="inline-end">
                  <InputGroupButton onClick={add} disabled={!adding.trim()}>
                    <PlusIcon data-icon="inline-start" />
                    Add
                  </InputGroupButton>
                </InputGroupAddon>
              </InputGroup>
            </Field>
            <FieldSet>
              <FieldLegend variant="label">Reconcile against type</FieldLegend>
              {!manifest || !guesses ? (
                !error && (
                  <FieldDescription className="flex items-center gap-2">
                    <Spinner />
                    Asking the service about a sample of this column…
                  </FieldDescription>
                )
              ) : (
                <RadioGroup value={type} onValueChange={(v) => setType(String(v))}>
                  {choices.map((c) => (
                    <Field key={c.id} orientation="horizontal">
                      <RadioGroupItem value={c.id} id={`rt-${c.id}`} />
                      <FieldLabel htmlFor={`rt-${c.id}`} className="font-normal">
                        {c.name} <span className="text-muted-foreground">({c.id}, {c.hint})</span>
                      </FieldLabel>
                    </Field>
                  ))}
                  <Field orientation="horizontal">
                    <RadioGroupItem value={CUSTOM} id="rt-custom" />
                    <FieldLabel htmlFor="rt-custom" className="font-normal">
                      Another type
                    </FieldLabel>
                    <Input aria-label="Type id" placeholder="Type id, like Q5" value={custom} onChange={(e) => (setCustom(e.target.value), setType(CUSTOM))} className="max-w-48" />
                  </Field>
                  <Field orientation="horizontal">
                    <RadioGroupItem value={NONE} id="rt-none" />
                    <FieldLabel htmlFor="rt-none" className="font-normal">
                      No particular type
                    </FieldLabel>
                  </Field>
                </RadioGroup>
              )}
            </FieldSet>
            {others.length > 0 && (
              <Field>
                <FieldLabel>Also use other columns</FieldLabel>
                <FieldDescription>A property id (like P17 for country) sends that column's cell along with each query; matched cells send their entity.</FieldDescription>
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Column</TableHead>
                      <TableHead>As property</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {others.map((c) => (
                      <TableRow key={c}>
                        <TableCell>{c}</TableCell>
                        <TableCell>
                          <Input aria-label={`Property for ${c}`} placeholder="—" value={props[c] ?? ""} onChange={(e) => setProps((p) => ({ ...p, [c]: e.target.value }))} />
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </Field>
            )}
            <Field orientation="horizontal">
              <Checkbox id="rc-auto" checked={autoMatch} onCheckedChange={(v) => setAutoMatch(!!v)} />
              <FieldLabel htmlFor="rc-auto" className="font-normal">
                Match automatically when the service is confident
              </FieldLabel>
            </Field>
            <Field>
              <FieldLabel htmlFor="rc-limit">Candidates per cell</FieldLabel>
              <Input id="rc-limit" type="number" min={0} placeholder="As many as the service sends" value={limit} onChange={(e) => setLimit(e.target.value)} className="max-w-64" />
            </Field>
            {error && <FieldError>{error}</FieldError>}
          </FieldGroup>
        </ScrollArea>
        <DialogFooter>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={apply} disabled={blocked}>
            {busy && <Spinner data-icon="inline-start" />}
            Start reconciling
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
