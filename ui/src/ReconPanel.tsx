import { useEffect, useState } from "react"
import { CheckIcon, CopyCheckIcon, ExternalLinkIcon, PlusIcon, XIcon } from "lucide-react"
import { call, type Engine, type Op } from "@/api"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Popover, PopoverContent } from "@/components/ui/popover"
import { Separator } from "@/components/ui/separator"
import { Spinner } from "@/components/ui/spinner"

type Candidate = { id: string; name: string; types: string[]; score: number }
type Recon = { j: "none" | "matched" | "new" | "error"; m?: Candidate; c: Candidate[]; e?: string; service: string; identifierSpace: string | null; schemaSpace: string | null }

/** Where a service shows an entity (its manifest's view URL), asked once per service. */
const views = new Map<string, Promise<string | null>>()
function viewUrl(service: string): Promise<string | null> {
  if (!views.has(service))
    views.set(
      service,
      call<{ view?: { url?: string } }>("recon_service", { url: service }, { quiet: true }).then(
        ({ data }) => data.view?.url ?? null,
        () => null,
      ),
    )
  return views.get(service)!
}

export type Judging = { row: number; column: string; value: string; anchor: HTMLElement }

/** OpenRefine's matching UI for one reconciled cell: its candidates, match one, create a new item,
 * search for another, for this cell or every cell with the same text. */
export function ReconPanel({ judging, engine, onApply, onClose }: { judging: Judging; engine: Engine; onApply: (op: Op) => Promise<void>; onClose: () => void }) {
  const [recon, setRecon] = useState<Recon | null>(null)
  const [view, setView] = useState<string | null>(null)
  const [other, setOther] = useState({ id: "", name: "" })
  const { row, column, value } = judging

  useEffect(() => {
    call<Recon | null>("recon_cell", { column, index: row }, { quiet: true }).then(({ data }) => {
      setRecon(data)
      if (data?.service && data.service !== "unknown") viewUrl(data.service).then(setView)
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
            <Separator />
            <Field>
              <FieldLabel>Another item</FieldLabel>
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
