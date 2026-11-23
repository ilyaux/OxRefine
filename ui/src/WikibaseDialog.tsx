import { useEffect, useState } from "react"
import { call, type Engine } from "@/api"
import { useDebounced } from "@/Dialogs"
import { FullView, Metric, RailLabel, Spacer, StripAction, SummaryStrip } from "@/components/instrument"
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { PlusIcon, Trash2Icon } from "lucide-react"

/** The wikibase extension's schema JSON (OpenRefine 3.10.1): what this editor reads and writes. */
type Expr = { type: string; [k: string]: unknown }
type Snak = { prop: Expr; value: Expr }
type Statement = { value: Expr | null; qualifiers: Snak[]; references: { snaks: Snak[] }[]; mergingStrategy: unknown; mode: string }
type Group = { property: Expr; statements: Statement[] }
type Term = { name_type: string; value: Expr }
type Entity = { type: "wbitemeditexpr" | "wbmediainfoeditexpr"; subject: Expr; nameDescs: Term[]; statementGroups: Group[]; filePath?: Expr | null; fileName?: Expr | null; wikitext?: Expr | null; overrideWikitext?: boolean }
export type Schema = { entityEdits: Entity[]; siteIri?: string; entityTypeSiteIRI?: Record<string, string>; mediaWikiApiEndpoint?: string }
type Warning = { type: string; aggregationId: string; severity: "CRITICAL" | "WARNING" | "INFO"; count: number; example: string }
type Preview = { edits: number; newEntities: number; quickstatements: string | null; quickstatementsError: string | null; warnings: Warning[] }

const SITES = {
  wikidata: { label: "Wikidata", siteIri: "http://www.wikidata.org/entity/", mediaWikiApiEndpoint: "https://www.wikidata.org/w/api.php" },
  commons: { label: "Wikimedia Commons", siteIri: "http://www.wikidata.org/entity/", mediaWikiApiEndpoint: "https://commons.wikimedia.org/w/api.php" },
}
const fresh = (): Schema => ({
  entityEdits: [],
  siteIri: SITES.wikidata.siteIri,
  entityTypeSiteIRI: { item: SITES.wikidata.siteIri, property: SITES.wikidata.siteIri, mediainfo: "https://commons.wikimedia.org/entity/" },
  mediaWikiApiEndpoint: SITES.wikidata.mediaWikiApiEndpoint,
})
const STRICT = { type: "qualifiers", valueMatcher: { type: "strict" }, pids: [] }
const TERMS: [string, string][] = [["LABEL_IF_NEW", "Label if new"], ["LABEL", "Label"], ["DESCRIPTION_IF_NEW", "Description if new"], ["DESCRIPTION", "Description"], ["ALIAS", "Alias"]]
const MODES: [string, string][] = [["add_or_merge", "Add or merge"], ["add", "Add"], ["delete", "Delete"]]
const DATATYPES = ["wikibase-item", "string", "external-id", "url", "commonsMedia", "time", "quantity", "globe-coordinate", "monolingualtext", "wikibase-property", "math", "tabular-data", "geo-shape", "musical-notation", "wikibase-lexeme", "wikibase-form", "wikibase-sense"]

/** The kind of value a datatype takes, and the expressions that give one. */
type Kind = "entity" | "string" | "language" | "date" | "location" | "quantity" | "monolingual"
const kindOf = (datatype: string): Kind =>
  datatype.startsWith("wikibase-") ? "entity" : datatype === "time" ? "date" : datatype === "globe-coordinate" ? "location" : datatype === "quantity" ? "quantity" : datatype === "monolingualtext" ? "monolingual" : "string"
const VARIABLE: Record<string, string> = { entity: "wbentityvariable", string: "wbstringvariable", language: "wblanguagevariable", date: "wbdatevariable", location: "wblocationvariable" }
const CONSTANT: Record<string, string> = { entity: "wbentityidvalueconstant", string: "wbstringconstant", language: "wblanguageconstant", date: "wbdateconstant", location: "wblocationconstant" }
const PLACEHOLDER: Record<string, string> = { entity: "Q42", string: "text", language: "en", date: "2024-01-31", location: "51.5,-0.12" }
const blankValue = (kind: Kind, columns: string[]): Expr =>
  kind === "monolingual" ? { type: "wbmonolingualexpr", language: { type: "wblanguageconstant", id: "en", label: "en" }, value: { type: "wbstringvariable", columnName: columns[0] ?? "" } }
  : kind === "quantity" ? { type: "wbquantityexpr", amount: { type: "wbstringvariable", columnName: columns[0] ?? "" }, unit: null }
  : { type: VARIABLE[kind], columnName: columns[0] ?? "" }
const prop = (pid = "", datatype = "wikibase-item"): Expr => ({ type: "wbpropconstant", pid, label: pid, datatype })
const select = "h-8 border border-input bg-background px-2 text-xs"

/** A value from a column or a constant, for one kind. Monolingual text and quantities combine two. */
function ValueInput({ kind, value, columns, onChange, label }: { kind: Kind; value: Expr | null; columns: string[]; onChange: (v: Expr | null) => void; label: string }) {
  if (kind === "monolingual") {
    const v = value ?? blankValue(kind, columns)
    return (
      <span className="flex flex-wrap items-center gap-1.5">
        <ValueInput kind="language" label={`${label} language`} value={v.language as Expr} columns={columns} onChange={(language) => onChange({ ...v, language })} />
        <ValueInput kind="string" label={`${label} text`} value={v.value as Expr} columns={columns} onChange={(text) => onChange({ ...v, value: text })} />
      </span>
    )
  }
  if (kind === "quantity") {
    const v = value ?? blankValue(kind, columns)
    const unit = v.unit as Expr | null
    return (
      <span className="flex flex-wrap items-center gap-1.5">
        <ValueInput kind="string" label={`${label} amount`} value={v.amount as Expr} columns={columns} onChange={(amount) => onChange({ ...v, amount })} />
        <Input aria-label={`${label} unit`} placeholder="unit, e.g. Q11573" className="h-8 w-36 text-xs" value={String(unit?.id ?? "")} onChange={(e) => onChange({ ...v, unit: e.target.value.trim() ? { type: "wbentityidvalueconstant", id: e.target.value.trim(), label: e.target.value.trim() } : null })} />
      </span>
    )
  }
  const v = value ?? blankValue(kind, columns)
  const fromColumn = "columnName" in v
  const constantKey = kind === "entity" || kind === "language" ? "id" : "value"
  return (
    <span className="flex items-center gap-1.5">
      <select aria-label={`${label} source`} className={select} value={fromColumn ? "column" : "constant"} onChange={(e) => onChange(e.target.value === "column" ? { type: VARIABLE[kind], columnName: columns[0] ?? "" } : { type: CONSTANT[kind], [constantKey]: "", ...(constantKey === "id" ? { label: "" } : {}) })}>
        <option value="column">Column</option>
        <option value="constant">Value</option>
      </select>
      {fromColumn ? (
        <select aria-label={label} className={select} value={String(v.columnName)} onChange={(e) => onChange({ ...v, columnName: e.target.value })}>
          {columns.map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
      ) : (
        <Input aria-label={label} className="h-8 w-40 text-xs" placeholder={PLACEHOLDER[kind]} value={String(v[constantKey] ?? "")} onChange={(e) => onChange({ ...v, [constantKey]: e.target.value, ...(constantKey === "id" ? { label: e.target.value } : {}) })} />
      )}
    </span>
  )
}

/** A property by id, found by name on the schema's Wikibase, with its datatype. */
function PropertyInput({ value, api, onChange }: { value: Expr; api: string; onChange: (v: Expr) => void }) {
  const [query, setQuery] = useState("")
  const [found, setFound] = useState<{ id: string; label: string; description?: string; datatype?: string }[]>([])
  const q = useDebounced(query, 300)
  useEffect(() => {
    if (q.trim().length < 2) return setFound([])
    let live = true
    call<typeof found>("wikibase_search", { url: api, method: "property", text: q }, { quiet: true }).then(({ data }) => live && setFound(data), () => live && setFound([]))
    return () => { live = false }
  }, [q, api])
  return (
    <span className="relative flex flex-wrap items-center gap-1.5">
      <Input aria-label="Property id" className="h-8 w-24 font-mono text-xs" placeholder="P31" value={String(value.pid ?? "")} onChange={(e) => onChange({ ...value, pid: e.target.value.trim(), label: e.target.value.trim() })} />
      <Input aria-label="Find a property" className="h-8 w-44 text-xs" placeholder="find by name" value={query} onChange={(e) => setQuery(e.target.value)} />
      <select aria-label="Datatype" className={select} value={String(value.datatype ?? "wikibase-item")} onChange={(e) => onChange({ ...value, datatype: e.target.value })}>
        {DATATYPES.map((d) => <option key={d} value={d}>{d}</option>)}
      </select>
      {value.label && value.label !== value.pid && <span className="text-xs text-muted-foreground">{String(value.label)}</span>}
      {found.length > 0 && (
        <span role="listbox" aria-label="Properties found" className="absolute top-9 left-24 z-10 flex w-96 flex-col border bg-popover shadow-md">
          {found.map((f) => (
            <button key={f.id} type="button" role="option" aria-selected={false} className="flex gap-2 px-3 py-1.5 text-left text-xs hover:bg-accent" onClick={() => { onChange({ type: "wbpropconstant", pid: f.id, label: f.label, datatype: f.datatype ?? "string" }); setQuery(""); setFound([]) }}>
              <span className="font-mono">{f.id}</span><span>{f.label}</span><span className="truncate text-muted-foreground">{f.description}</span>
            </button>
          ))}
        </span>
      )}
    </span>
  )
}

function Snaks({ snaks, columns, api, label, onChange }: { snaks: Snak[]; columns: string[]; api: string; label: string; onChange: (s: Snak[]) => void }) {
  return (
    <div className="flex flex-col gap-1.5 pl-6">
      {snaks.map((s, i) => (
        <div key={i} className="flex flex-wrap items-center gap-1.5">
          <PropertyInput value={s.prop} api={api} onChange={(p) => onChange(snaks.map((x, j) => (j === i ? { prop: p, value: kindOf(String(p.datatype)) === kindOf(String(s.prop.datatype)) ? s.value : blankValue(kindOf(String(p.datatype)), columns) } : x)))} />
          <ValueInput kind={kindOf(String(s.prop.datatype))} label={`${label} value`} value={s.value} columns={columns} onChange={(v) => onChange(snaks.map((x, j) => (j === i ? { ...x, value: v ?? x.value } : x)))} />
          <Button variant="ghost" size="icon-sm" aria-label={`Remove ${label}`} onClick={() => onChange(snaks.filter((_, j) => j !== i))}><Trash2Icon /></Button>
        </div>
      ))}
      <Button variant="ghost" size="sm" className="self-start" onClick={() => onChange([...snaks, { prop: prop("", "string"), value: blankValue("string", columns) }])}><PlusIcon data-icon="inline-start" />{label}</Button>
    </div>
  )
}

function EntityEditor({ entity, columns, api, onChange, onRemove }: { entity: Entity; columns: string[]; api: string; onChange: (e: Entity) => void; onRemove: () => void }) {
  const media = entity.type === "wbmediainfoeditexpr"
  const setGroup = (i: number, g: Group) => onChange({ ...entity, statementGroups: entity.statementGroups.map((x, j) => (j === i ? g : x)) })
  return (
    <section aria-label={media ? "Media info" : "Item"} className="flex flex-col gap-3 border-b px-5 py-4">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-medium">{media ? "Media info" : "Item"}</span>
        <ValueInput kind="entity" label="Subject" value={entity.subject} columns={columns} onChange={(subject) => subject && onChange({ ...entity, subject })} />
        <span className="text-xs text-muted-foreground">a reconciled column, or an id</span>
        <Spacer />
        <Button variant="ghost" size="sm" onClick={onRemove}><Trash2Icon data-icon="inline-start" />Remove</Button>
      </div>
      {media && (
        <div className="flex flex-wrap items-center gap-2 pl-4">
          <span className="w-20 text-xs text-muted-foreground">File name</span>
          <ValueInput kind="string" label="File name" value={entity.fileName ?? null} columns={columns} onChange={(fileName) => onChange({ ...entity, fileName })} />
          <span className="w-20 text-xs text-muted-foreground">File path</span>
          <ValueInput kind="string" label="File path" value={entity.filePath ?? null} columns={columns} onChange={(filePath) => onChange({ ...entity, filePath })} />
        </div>
      )}
      <div className="flex flex-col gap-1.5 pl-4">
        <span className="text-xs text-muted-foreground">Terms</span>
        {entity.nameDescs.map((t, i) => (
          <div key={i} className="flex flex-wrap items-center gap-1.5">
            <select aria-label="Term" className={select} value={t.name_type} onChange={(e) => onChange({ ...entity, nameDescs: entity.nameDescs.map((x, j) => (j === i ? { ...x, name_type: e.target.value } : x)) })}>
              {TERMS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
            <ValueInput kind="monolingual" label="Term" value={t.value} columns={columns} onChange={(v) => v && onChange({ ...entity, nameDescs: entity.nameDescs.map((x, j) => (j === i ? { ...x, value: v } : x)) })} />
            <Button variant="ghost" size="icon-sm" aria-label="Remove term" onClick={() => onChange({ ...entity, nameDescs: entity.nameDescs.filter((_, j) => j !== i) })}><Trash2Icon /></Button>
          </div>
        ))}
        <Button variant="ghost" size="sm" className="self-start" onClick={() => onChange({ ...entity, nameDescs: [...entity.nameDescs, { name_type: "LABEL_IF_NEW", value: blankValue("monolingual", columns) }] })}><PlusIcon data-icon="inline-start" />Term</Button>
      </div>
      <div className="flex flex-col gap-2 pl-4">
        <span className="text-xs text-muted-foreground">Statements</span>
        {entity.statementGroups.map((g, i) => {
          const kind = kindOf(String(g.property.datatype ?? "wikibase-item"))
          return (
            <div key={i} className="flex flex-col gap-1.5 border-l-2 pl-3">
              <div className="flex flex-wrap items-center gap-1.5">
                <PropertyInput value={g.property} api={api} onChange={(p) => setGroup(i, { property: p, statements: kindOf(String(p.datatype)) === kind ? g.statements : g.statements.map((s) => ({ ...s, value: blankValue(kindOf(String(p.datatype)), columns) })) })} />
                <Button variant="ghost" size="icon-sm" aria-label="Remove property" onClick={() => onChange({ ...entity, statementGroups: entity.statementGroups.filter((_, j) => j !== i) })}><Trash2Icon /></Button>
              </div>
              {g.statements.map((s, k) => {
                const setStatement = (next: Statement) => setGroup(i, { ...g, statements: g.statements.map((x, j) => (j === k ? next : x)) })
                return (
                  <div key={k} className="flex flex-col gap-1 pl-4">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <ValueInput kind={kind} label="Value" value={s.value} columns={columns} onChange={(v) => setStatement({ ...s, value: v })} />
                      <select aria-label="Mode" className={select} value={s.mode} onChange={(e) => setStatement({ ...s, mode: e.target.value })}>
                        {MODES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                      </select>
                      <Button variant="ghost" size="icon-sm" aria-label="Remove value" onClick={() => setGroup(i, { ...g, statements: g.statements.filter((_, j) => j !== k) })}><Trash2Icon /></Button>
                    </div>
                    <Snaks label="Qualifier" snaks={s.qualifiers} columns={columns} api={api} onChange={(qualifiers) => setStatement({ ...s, qualifiers })} />
                    {s.references.map((r, ri) => (
                      <Snaks key={ri} label={`Reference ${ri + 1}`} snaks={r.snaks} columns={columns} api={api} onChange={(snaks) => setStatement({ ...s, references: snaks.length ? s.references.map((x, j) => (j === ri ? { snaks } : x)) : s.references.filter((_, j) => j !== ri) })} />
                    ))}
                    <Button variant="ghost" size="sm" className="self-start pl-6" onClick={() => setStatement({ ...s, references: [...s.references, { snaks: [{ prop: prop("P854", "url"), value: blankValue("string", columns) }] }] })}><PlusIcon data-icon="inline-start" />Reference</Button>
                  </div>
                )
              })}
              <Button variant="ghost" size="sm" className="self-start" onClick={() => setGroup(i, { ...g, statements: [...g.statements, { value: blankValue(kind, columns), qualifiers: [], references: [], mergingStrategy: STRICT, mode: "add_or_merge" }] })}><PlusIcon data-icon="inline-start" />Value</Button>
            </div>
          )
        })}
        <Button variant="ghost" size="sm" className="self-start" onClick={() => onChange({ ...entity, statementGroups: [...entity.statementGroups, { property: prop(), statements: [{ value: blankValue("entity", columns), qualifiers: [], references: [], mergingStrategy: STRICT, mode: "add_or_merge" }] }] })}><PlusIcon data-icon="inline-start" />Statement</Button>
      </div>
    </section>
  )
}

/** OpenRefine's Wikibase → Edit Wikibase schema: entities, terms and statements from columns, the edits
 * they make on the matching rows (as QuickStatements) and their issues. Saving records a history step. */
export function WikibaseDialog({ columns, engine, onSave, onClose }: { columns: string[]; engine: Engine; onSave: (schema: Schema) => Promise<unknown>; onClose: () => void }) {
  const [schema, setSchema] = useState<Schema | null>(null)
  const [preview, setPreview] = useState<Preview | null>(null)
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    call<Schema | null>("wikibase_schema").then(({ data }) => setSchema(data ?? fresh()), (e) => { setError((e as Error).message); setSchema(fresh()) })
  }, [])
  const debounced = useDebounced(schema, 400)
  useEffect(() => {
    if (!debounced) return
    let live = true
    call<Preview>("wikibase_preview", { params: { schema: debounced }, engine }, { quiet: true })
      .then(({ data }) => { if (live) { setPreview(data); setError("") } }, (e) => live && setError((e as Error).message))
    return () => { live = false }
  }, [debounced, engine])
  if (!schema) return null
  const api = schema.mediaWikiApiEndpoint ?? SITES.wikidata.mediaWikiApiEndpoint
  const site = Object.entries(SITES).find(([, s]) => s.mediaWikiApiEndpoint === api)?.[0] ?? "custom"
  const issues = preview?.warnings.reduce((n, w) => n + (w.severity === "INFO" ? 0 : w.count), 0) ?? 0
  const save = async () => {
    setBusy(true)
    try { await onSave(schema); onClose() } catch (e) { setError((e as Error).message) } finally { setBusy(false) }
  }
  const add = (type: Entity["type"]) => setSchema({ ...schema, entityEdits: [...schema.entityEdits, { type, subject: blankValue("entity", columns), nameDescs: [], statementGroups: [], ...(type === "wbmediainfoeditexpr" ? { fileName: null, filePath: null, wikitext: null, overrideWikitext: false } : {}) }] })
  return (
    <FullView crumbs={["Wikibase schema"]} onClose={onClose}>
      <SummaryStrip>
        <Metric label="entities edited">{preview ? preview.edits.toLocaleString() : "…"}</Metric>
        <Metric label="new">{preview ? preview.newEntities.toLocaleString() : "…"}</Metric>
        <Metric label="issues">{preview ? issues.toLocaleString() : "…"}</Metric>
        <Spacer />
        <StripAction ink disabled={busy || !!error} onClick={save}>{busy && <Spinner />}Save schema</StripAction>
      </SummaryStrip>
      {error && <p role="alert" className="border-b px-5 py-2.5 text-sm text-destructive">{error}</p>}
      <div className="flex min-h-0 flex-1">
        <aside aria-label="Wikibase" className="w-64 shrink-0 overflow-y-auto border-r pb-4">
          <RailLabel>Wikibase</RailLabel>
          <div className="flex flex-col gap-2 px-3.5">
            <select aria-label="Wikibase" className={select} value={site} onChange={(e) => {
              const s = SITES[e.target.value as keyof typeof SITES]
              if (s) setSchema({ ...schema, siteIri: s.siteIri, mediaWikiApiEndpoint: s.mediaWikiApiEndpoint, entityTypeSiteIRI: { ...schema.entityTypeSiteIRI, item: s.siteIri, property: s.siteIri } })
            }}>
              {Object.entries(SITES).map(([k, s]) => <option key={k} value={k}>{s.label}</option>)}
              <option value="custom">Another Wikibase</option>
            </select>
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">MediaWiki API
              <Input className="h-8 text-xs" value={api} onChange={(e) => setSchema({ ...schema, mediaWikiApiEndpoint: e.target.value })} />
            </label>
            <label className="flex flex-col gap-1 text-xs text-muted-foreground">Entity IRI (reconciled cells must use it)
              <Input className="h-8 text-xs" value={schema.siteIri ?? ""} onChange={(e) => setSchema({ ...schema, siteIri: e.target.value, entityTypeSiteIRI: { ...schema.entityTypeSiteIRI, item: e.target.value, property: e.target.value } })} />
            </label>
          </div>
          <RailLabel>Add</RailLabel>
          <div className="flex flex-col gap-1 px-2">
            <Button variant="ghost" size="sm" className="justify-start" onClick={() => add("wbitemeditexpr")}><PlusIcon data-icon="inline-start" />Item</Button>
            <Button variant="ghost" size="sm" className="justify-start" onClick={() => add("wbmediainfoeditexpr")}><PlusIcon data-icon="inline-start" />Media info</Button>
          </div>
        </aside>
        <Tabs defaultValue="schema" className="flex min-w-0 flex-1 flex-col">
          <TabsList className="mx-5 mt-3">
            <TabsTrigger value="schema">Schema</TabsTrigger>
            <TabsTrigger value="preview">Preview</TabsTrigger>
            <TabsTrigger value="issues">Issues{preview && issues > 0 ? ` (${issues})` : ""}</TabsTrigger>
          </TabsList>
          <TabsContent value="schema" className="min-h-0 overflow-y-auto">
            {schema.entityEdits.length === 0 && <p className="px-5 py-4 text-sm text-muted-foreground">Add an item or media info on the left, then fill its terms and statements from your columns.</p>}
            {schema.entityEdits.map((e, i) => (
              <EntityEditor key={i} entity={e} columns={columns} api={api}
                onChange={(next) => setSchema({ ...schema, entityEdits: schema.entityEdits.map((x, j) => (j === i ? next : x)) })}
                onRemove={() => setSchema({ ...schema, entityEdits: schema.entityEdits.filter((_, j) => j !== i) })} />
            ))}
          </TabsContent>
          <TabsContent value="preview" className="min-h-0 overflow-auto px-5 py-3">
            {preview?.quickstatementsError ? <p className="text-sm text-muted-foreground">{preview.quickstatementsError}</p>
              : <pre aria-label="Edits as QuickStatements" className="font-mono text-xs whitespace-pre">{preview?.quickstatements || "No edits on the matching rows."}</pre>}
          </TabsContent>
          <TabsContent value="issues" className="min-h-0 overflow-y-auto">
            {preview?.warnings.map((w) => (
              <div key={w.aggregationId} className="flex items-center gap-3 border-b px-5 py-2 text-sm">
                <Badge variant={w.severity === "CRITICAL" ? "destructive" : w.severity === "WARNING" ? "secondary" : "outline"}>{w.severity.toLowerCase()}</Badge>
                <span className="flex-1">{w.type.replaceAll("-", " ")}</span>
                {w.example && <span className="truncate font-mono text-xs text-muted-foreground">{w.example}</span>}
                <span className="font-mono text-xs">{w.count.toLocaleString()}</span>
              </div>
            ))}
            {preview && preview.warnings.length === 0 && <p className="px-5 py-4 text-sm text-muted-foreground">No issues found.</p>}
          </TabsContent>
        </Tabs>
      </div>
    </FullView>
  )
}
