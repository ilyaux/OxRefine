import { useEffect, useState } from "react"
import { CheckIcon, CopyCheckIcon, ExternalLinkIcon, EyeIcon, PlusIcon, SearchIcon, XIcon } from "lucide-react"
import { call, type Engine, type Op } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent } from "@/components/ui/popover"
import { Separator } from "@/components/ui/separator"
import { Spinner } from "@/components/ui/spinner"
import { InputGroup, InputGroupAddon, InputGroupInput } from "@/components/ui/input-group"

type Candidate = { id: string; name: string; types: string[]; score: number }
type Recon = { j: "none" | "matched" | "new" | "error"; m?: Candidate; c: Candidate[]; e?: string; service: string; identifierSpace: string | null; schemaSpace: string | null }

type Manifest = { view?: { url?: string }; preview?: { url?: string; width?: number; height?: number }; suggest?: Record<string, unknown> }

/** A service's manifest (where it shows and previews entities, what it can suggest), asked once per service. */
const manifests = new Map<string, Promise<Manifest | null>>()
export function manifestOf(service: string): Promise<Manifest | null> {
  if (!manifests.has(service))
    manifests.set(
      service,
      call<Manifest>("recon_service", { url: service }, { quiet: true }).then(
        ({ data }) => data,
        () => null,
      ),
    )
  return manifests.get(service)!
}

export type Suggestion = { id: string; name: string; description?: string }

/** Search the service's suggest API (`kind`: entity, type or property) as you type. */
export function SuggestBox({ service, kind, placeholder, onPick }: { service: string; kind: "entity" | "type" | "property"; placeholder: string; onPick: (s: Suggestion) => void }) {
  const [text, setText] = useState("")
  const [found, setFound] = useState<Suggestion[]>([])
  useEffect(() => {
    if (text.trim().length < 2) return setFound([])
    let live = true
    const t = setTimeout(() => {
      call<Suggestion[]>("recon_suggest", { url: service, method: kind, text: text.trim() }, { quiet: true }).then(
        ({ data }) => live && setFound(data.slice(0, 8)),
        () => live && setFound([]),
      )
    }, 250)
    return () => {
      live = false
      clearTimeout(t)
    }
  }, [text, service, kind])
  return (
    <div className="flex flex-col gap-1">
      <InputGroup>
        <InputGroupAddon>
          <SearchIcon />
        </InputGroupAddon>
        <InputGroupInput aria-label={placeholder} placeholder={placeholder} value={text} onChange={(e) => setText(e.target.value)} />
      </InputGroup>
      {found.map((s) => (
        <Button key={s.id} variant="ghost" size="sm" className="h-auto justify-start py-1 text-left" onClick={() => (onPick(s), setText(""), setFound([]))}>
          <span className="min-w-0 truncate">
            {s.name} <span className="text-muted-foreground">({s.id}{s.description ? `, ${s.description}` : ""})</span>
          </span>
        </Button>
      ))}
    </div>
  )
}

export type Judging = { row: number; column: string; value: string; anchor: HTMLElement }

/** OpenRefine's matching UI for one reconciled cell: its candidates, match one, create a new item,
 * search for another, for this cell or every cell with the same text. */
export function ReconPanel({ judging, engine, onApply, onClose }: { judging: Judging; engine: Engine; onApply: (op: Op) => Promise<void>; onClose: () => void }) {
  const [recon, setRecon] = useState<Recon | null>(null)
  const [manifest, setManifest] = useState<Manifest | null>(null)
  const [preview, setPreview] = useState<string | null>(null)
  const view = manifest?.view?.url ?? null
  const [other, setOther] = useState({ id: "", name: "" })
  const { row, column, value } = judging

  useEffect(() => {
    call<Recon | null>("recon_cell", { column, index: row }, { quiet: true }).then(({ data }) => {
      setRecon(data)
      if (data?.service && data.service !== "unknown") manifestOf(data.service).then(setManifest)
    })
  }, [row, column])

  const done = (op: Op) => onApply(op).then(onClose, () => {})
  const one = (judgment: string, match?: Candidate) => done({ op: "ox/recon-judge-one-cell", row, columnName: column, judgment, ...(match ? { match } : {}) })
  const similar = (judgment: string, match?: Candidate, shareNewTopics = false) =>
    done({ op: "core/recon-judge-similar-cells", engineConfig: engine, columnName: column, similarValue: value, judgment, shareNewTopics, ...(match ? { match } : {}) })
  const link = (id: string) => (view ? view.replace("{{id}}", encodeURIComponent(id)) : null)
  const manual: Candidate | null = other.id.trim() ? { id: other.id.trim(), name: other.name.trim() || other.id.trim(), types: [], score: 100 } : null

  return (
    <Popover open onOpenChange={(o) => !o && onClose()}>
      <PopoverContent anchor={judging.anchor} align="start" className="w-96">
        {!recon ? (
          <Spinner />
        ) : (
          <FieldGroup>
            <Field>
              <FieldLabel>
                “{value}” <Badge variant="secondary">{recon.j === "error" ? "error" : recon.j}</Badge>
              </FieldLabel>
              {recon.j === "matched" && recon.m && (
                <FieldDescription>
                  Matched to {recon.m.name} ({recon.m.id})
                  {link(recon.m.id) && (
                    <a href={link(recon.m.id)!} target="_blank" rel="noreferrer" className="ml-1 inline-flex items-center underline">
                      open <ExternalLinkIcon className="ml-0.5 size-3" />
                    </a>
                  )}
                </FieldDescription>
              )}
              {recon.e && <FieldDescription>{recon.e}</FieldDescription>}
            </Field>
            {recon.c.length > 0 && (
              <div className="flex flex-col gap-1">
                {recon.c.map((c) => (
                  <div key={c.id} className="flex items-center gap-2">
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm">
                        {link(c.id) ? (
                          <a href={link(c.id)!} target="_blank" rel="noreferrer" className="underline">
                            {c.name}
                          </a>
                        ) : (
                          c.name
                        )}
                      </div>
                      <div className="truncate text-xs text-muted-foreground">
                        {c.id} · score {Math.round(c.score * 10) / 10}
                        {c.types.length > 0 && ` · ${c.types.join(", ")}`}
                      </div>
                    </div>
                    {manifest?.preview?.url && (
                      <Button variant="ghost" size="icon-sm" aria-label={`Preview ${c.name}`} title="Preview" aria-pressed={preview === c.id} onClick={() => setPreview((p) => (p === c.id ? null : c.id))}>
                        <EyeIcon />
                      </Button>
                    )}
                    <Button variant="outline" size="icon-sm" aria-label={`Match ${c.name} to this cell`} title="Match to this cell" onClick={() => one("matched", c)}>
                      <CheckIcon />
                    </Button>
                    <Button variant="outline" size="icon-sm" aria-label={`Match ${c.name} to all cells with this text`} title="Match to all identical cells" onClick={() => similar("matched", c)}>
                      <CopyCheckIcon />
                    </Button>
                  </div>
                ))}
              </div>
            )}
            {preview && manifest?.preview?.url && (
              <iframe
                title={`Preview of ${preview}`}
                src={manifest.preview.url.replace("{{id}}", encodeURIComponent(preview))}
                width={manifest.preview.width ?? 400}
                height={manifest.preview.height ?? 200}
                className="w-full rounded-md border"
              />
            )}
            <Separator />
            <Field>
              <FieldLabel>Another item</FieldLabel>
              {manifest?.suggest && recon.service && <SuggestBox service={recon.service} kind="entity" placeholder="Search the service" onPick={(s) => setOther({ id: s.id, name: s.name })} />}
              <div className="flex gap-2">
                <Input aria-label="Item id" placeholder="id" value={other.id} onChange={(e) => setOther((o) => ({ ...o, id: e.target.value }))} />
                <Input aria-label="Item name" placeholder="name" value={other.name} onChange={(e) => setOther((o) => ({ ...o, name: e.target.value }))} />
              </div>
              <div className="flex gap-2">
                <Button variant="outline" size="sm" disabled={!manual} onClick={() => manual && one("matched", manual)}>
                  Match this cell
                </Button>
                <Button variant="outline" size="sm" disabled={!manual} onClick={() => manual && similar("matched", manual)}>
                  Match identical cells
                </Button>
              </div>
            </Field>
            <Separator />
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size="sm" onClick={() => one("new")}>
                <PlusIcon data-icon="inline-start" />
                New item
              </Button>
              <Button variant="outline" size="sm" onClick={() => similar("new", undefined, true)}>
                <PlusIcon data-icon="inline-start" />
                One new item for identical cells
              </Button>
              {recon.j !== "none" && recon.j !== "error" && (
                <Button variant="ghost" size="sm" onClick={() => one("none")}>
                  Discard judgment
                </Button>
              )}
              <Button variant="ghost" size="sm" onClick={() => done({ op: "ox/recon-clear-one-cell", row, columnName: column })}>
                <XIcon data-icon="inline-start" />
                Clear
              </Button>
            </div>
          </FieldGroup>
        )}
      </PopoverContent>
    </Popover>
  )
}
