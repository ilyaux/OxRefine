import { useEffect, useState } from "react"
import { call, type Engine } from "@/api"
import { useDebounced } from "@/Dialogs"
import { FullView, Metric, Spacer, StripAction, SummaryStrip } from "@/components/instrument"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"
import { ChevronDownIcon, ChevronUpIcon } from "lucide-react"

type ColumnOptions = { name: string; dateSettings: { format: string; custom: string; omitTime: boolean; useLocalTimeZone: boolean }; reconSettings: { output: string; blankUnmatchedCells: boolean; linkToEntityPages: boolean } }
const columnDefaults = (name: string): ColumnOptions => ({ name, dateSettings: { format: "iso-8601", custom: "yyyy-MM-dd", omitTime: false, useLocalTimeZone: false }, reconSettings: { output: "entity-name", blankUnmatchedCells: false, linkToEntityPages: true } })
const PRESETS = "oxrefine.export.presets"
const CUSTOM_FORMATS = ["csv", "tsv", "*sv", "html", "xlsx", "xls", "ods"]
type SvOptions = { separator: string; lineSeparator: string; outputColumnHeaders: boolean; quoteAll: boolean; outputBlankRows: boolean }
type Preset = { name: string; format: ExportKind; columns: ColumnOptions[]; sv: SvOptions; encoding: string; bom: boolean; replace: boolean }
function validPreset(value: unknown): value is Preset {
  if (!value || typeof value !== "object") return false
  const p = value as Preset
  const only = (o: object, fields: string[]) => Object.keys(o).every((k) => fields.includes(k))
  return only(p,["name","format","columns","sv","encoding","bom","replace"]) && typeof p.name === "string" && p.name.length > 0 && p.name.length <= 80 && CUSTOM_FORMATS.includes(p.format) && ENCODINGS.includes(p.encoding) && typeof p.bom === "boolean" && typeof p.replace === "boolean" &&
    !!p.sv && only(p.sv,["separator","lineSeparator","outputColumnHeaders","outputBlankRows","quoteAll"]) && typeof p.sv.separator === "string" && p.sv.separator.length <= 32 && typeof p.sv.lineSeparator === "string" && p.sv.lineSeparator.length <= 16 && [p.sv.outputColumnHeaders, p.sv.outputBlankRows, p.sv.quoteAll].every((v) => typeof v === "boolean") &&
    Array.isArray(p.columns) && p.columns.length > 0 && p.columns.length <= 1000 && p.columns.every((c) => typeof c?.name === "string" && c.name.length <= 256 && !!c.dateSettings && !!c.reconSettings &&
      only(c,["name","dateSettings","reconSettings"]) && only(c.dateSettings,["format","custom","omitTime","useLocalTimeZone"]) && only(c.reconSettings,["output","blankUnmatchedCells","linkToEntityPages"]) && ["iso-8601","locale-short","locale-medium","locale-long","locale-full","custom"].includes(c.dateSettings.format) && typeof c.dateSettings.custom === "string" && c.dateSettings.custom.length <= 256 && [c.dateSettings.omitTime, c.dateSettings.useLocalTimeZone, c.reconSettings.blankUnmatchedCells, c.reconSettings.linkToEntityPages].every((v) => typeof v === "boolean") && ["entity-name", "entity-id", "cell-content"].includes(c.reconSettings.output)) && new Set(p.columns.map((c) => c.name)).size === p.columns.length
}

/** Formats with options of their own, as the engine's export takes them. */
export type ExportFormat = "*sv" | "template" | "sql"
export const EXTENSION: Record<ExportFormat, string> = { "*sv": "csv", template: "txt", sql: "sql" }

/** Everything the Export view offers: the engine's row formats, then the whole project and its recipe. */
export type ExportKind = "csv" | "tsv" | ExportFormat | "xlsx" | "xls" | "ods" | "html" | "project" | "recipe"
const FORMATS: { id: ExportKind; label: string; ext: string; text: boolean }[] = [
  { id: "csv", label: "CSV", ext: "csv", text: true },
  { id: "tsv", label: "TSV", ext: "tsv", text: true },
  { id: "*sv", label: "Custom tabular", ext: "csv", text: true },
  { id: "xlsx", label: "Excel", ext: "xlsx", text: false },
  { id: "xls", label: "Excel 97", ext: "xls", text: false },
  { id: "ods", label: "OpenDocument", ext: "ods", text: false },
  { id: "html", label: "HTML table", ext: "html", text: true },
  { id: "template", label: "Templating", ext: "txt", text: true },
  { id: "sql", label: "SQL", ext: "sql", text: true },
  { id: "project", label: "OpenRefine project", ext: "tar.gz", text: false },
  { id: "recipe", label: "Recipe JSON", ext: "json", text: false },
]
const ENCODINGS = ["UTF-8", "UTF-16LE", "UTF-16BE", "windows-1251", "windows-1252", "ISO-8859-1", "US-ASCII"]

const HINTS: Partial<Record<ExportKind, string>> = {
  csv: "Comma-separated text with configurable columns and headers.",
  tsv: "Tab-separated text with configurable columns and headers.",
  "*sv": "Pick columns, separator and quoting.",
  xlsx: "An Excel workbook with one sheet. It's a binary file, so there is no text preview.",
  xls: "An Excel 97 workbook (.xls) with one sheet, for older software: at most 65,536 rows and 256 columns, and 32,767 characters a cell. It's a binary file, so there is no text preview.",
  ods: "An OpenDocument spreadsheet with one sheet. It's a binary file, so there is no text preview.",
  html: "An HTML table.",
  template: "Each row renders the template; {{ … }} holds a GREL expression such as cells[\"name\"].value.",
  sql: "CREATE TABLE and INSERT statements for the matching rows. Blank cells become NULL.",
  project: "The whole project with its undo history, as an OpenRefine archive; OpenRefine opens it with OxRefine's steps.",
  recipe: "The steps so far as OpenRefine-compatible JSON, for Undo/Redo → Apply in OpenRefine or for ox apply. History → Extract picks steps.",
}

/** OpenRefine's default template: one JSON object per row. */
const defaultTemplate = (columns: string[]) => ({
  prefix: '{\n  "rows" : [\n',
  template: `    {\n${columns.map((c) => `      ${JSON.stringify(c)} : {{jsonize(cells[${JSON.stringify(c)}].value)}}`).join(",\n")}\n    }`,
  separator: ",\n",
  suffix: "\n  ]\n}",
})

/** A segmented control of a few choices: the chosen one in ink. */
function Segmented<T extends string>({ label, value, options, onChange }: { label: string; value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div role="radiogroup" aria-label={label} className="flex">
      {options.map(([v, l]) => (
        <button
          key={l}
          type="button"
          role="radio"
          aria-checked={value === v}
          onClick={() => onChange(v)}
          className={cn(
            "h-7.5 flex-1 border border-input px-2 outline-none not-first:-ml-px first:rounded-l-[3px] last:rounded-r-[3px] focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-ring",
            value === v ? "z-1 border-foreground bg-foreground text-background" : "hover:bg-accent",
          )}
        >
          {l}
        </button>
      ))}
    </div>
  )
}

export function ExportDialog({
  format: initial,
  name,
  columns,
  engine,
  rows,
  allRows,
  onExport,
  onClose,
}: {
  format: ExportKind
  /** the project's name, for the file name */
  name: string
  columns: string[]
  engine: Engine
  /** rows matching the facets, and all rows */
  rows: number
  allRows: number
  /** save in this format: the engine's export with these parameters, or the project or its recipe */
  onExport: (format: ExportKind, ext: string, params: object) => void
  onClose: () => void
}) {
  const [format, setFormat] = useState<ExportKind>(initial)
  const [chosen, setChosen] = useState(columns)
  const [sv, setSv] = useState({ separator: ",", lineSeparator: "\n", outputColumnHeaders: true, quoteAll: false, outputBlankRows: true })
  const [tpl, setTpl] = useState(() => defaultTemplate(columns))
  const [sql, setSql] = useState({ tableName: "data", includeDropStatement: false, includeStructure: true, includeContent: true })
  const [encoding, setEncoding] = useState("UTF-8")
  const [bom, setBom] = useState(false)
  const [replace, setReplace] = useState(false)
  const [blankRows, setBlankRows] = useState<Partial<Record<ExportKind, boolean>>>({})
  const outputBlankRows = blankRows[format] ?? (format !== "csv" && format !== "tsv")
  const [columnOptions, setColumnOptions] = useState<Record<string, ColumnOptions>>(() => Object.fromEntries(columns.map((name) => [name, columnDefaults(name)])))
  const [editing, setEditing] = useState(columns[0] ?? "")
  const [presets, setPresets] = useState<Preset[] | null>(null)
  const [presetName, setPresetName] = useState("")
  const [presetError, setPresetError] = useState("")
  const [presetNotice, setPresetNotice] = useState("")
  const [presetBusy, setPresetBusy] = useState(false)
  useEffect(() => {
    let live = true
    call<Record<string, unknown>>("preferences", {}, { quiet: true }).then(({ data }) => {
      if (!live) return
      const saved = data[PRESETS]
      if (saved === undefined || saved === null) setPresets([])
      else if (Array.isArray(saved) && saved.length <= 20 && saved.every(validPreset) && new Set(saved.map((p) => p.name)).size === saved.length) setPresets(saved)
      else setPresetError("Saved export presets are invalid. Review this preference before replacing them.")
    }, (error) => live && setPresetError(String(error.message ?? error)))
    return () => { live = false }
  }, [])
  const spec = FORMATS.find((f) => f.id === format)!
  const tabular = format === "csv" || format === "tsv" || format === "*sv"
  const custom = CUSTOM_FORMATS.includes(format)
  const workbook = ["xlsx", "xls", "ods"].includes(format)
  const unicode = encoding.startsWith("UTF-")
  const charset = { encoding, bom: unicode && bom, encodingErrors: !unicode && replace ? "replace" : "error" }

  const ordered = chosen.filter((c) => columns.includes(c))
  const selectedOptions = ordered.map((name) => columnOptions[name] ?? columnDefaults(name))
  const active = ordered.includes(editing) ? editing : ordered[0]
  const activeOptions = active ? columnOptions[active] ?? columnDefaults(active) : undefined
  const setActive = (next: ColumnOptions) => setColumnOptions((old) => ({ ...old, [next.name]: next }))
  const params =
    format === "template"
      ? tpl
      : format === "sql"
        ? { options: { ...sql, includeIfExistWithDropStatement: sql.includeDropStatement, columns: ordered.map((n) => ({ name: n, type: "VARCHAR", size: 255, allowNull: true })) } }
        : custom
          ? { ...(tabular ? charset : {}), options: { ...(format === "*sv" ? { ...sv, outputBlankRows } : { outputColumnHeaders: sv.outputColumnHeaders, outputBlankRows }), columns: selectedOptions } }
          : {}

  const requested = JSON.stringify([format, params])
  const key = useDebounced(requested, 250)
  const [preview, setPreview] = useState<{ key?: string; engine?: Engine; text?: string; error?: string }>({})
  useEffect(() => {
    const [f, p] = JSON.parse(key) as [ExportKind, object]
    if (!FORMATS.find((x) => x.id === f)?.text) return
    let live = true
    call<string>("export", { format: f, params: p, engine, limit: 10 }).then(
      ({ data }) => live && setPreview({ key, engine, text: data }),
      (e) => live && setPreview({ key, engine, error: String((e as Error).message ?? e) }),
    )
    return () => {
      live = false
    }
  }, [key, engine])
  const shown = preview.key === requested && preview.engine === engine ? preview : {}

  const blocked = ((custom || format === "sql") && !ordered.length) || ((tabular || format === "html") && (shown.text === undefined || !!shown.error))
  const file = `${name.replace(/\.[^.]+$/, "")}.${format === "project" ? "openrefine.tar.gz" : spec.ext}`
  const save = () => !blocked && onExport(format, spec.ext, params)
  const check = (label: string, on: boolean, set: (v: boolean) => void) => (
    <label key={label} className="flex cursor-pointer items-center gap-2.5 py-1">
      <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} className="accent-foreground" />
      {label}
    </label>
  )
  const label = (text: string) => <span className="text-[11.5px] text-muted-foreground">{text}</span>
  const storePresets = async (next: Preset[]) => {
    setPresetBusy(true); setPresetError("")
    try { await call("set_preference", { name: PRESETS, value: next }, { quiet: true }); setPresets(next) }
    catch (error) { setPresetError(String((error as Error).message ?? error)) }
    finally { setPresetBusy(false) }
  }
  const applyPreset = (preset: Preset) => {
    const present = preset.columns.filter((c) => columns.includes(c.name))
    setFormat(preset.format); setChosen(present.map((c) => c.name)); setColumnOptions(Object.fromEntries(present.map((c) => [c.name, c])))
    setSv(preset.sv); setEncoding(preset.encoding); setBom(preset.bom); setReplace(preset.replace); setPresetName(preset.name)
    setBlankRows((old) => ({ ...old, [preset.format]: preset.sv.outputBlankRows }))
    setPresetNotice(present.length === preset.columns.length ? "Preset applied." : `${preset.columns.length - present.length} preset columns are missing from this project; review the selection.`)
  }
  const move = (name: string, by: number) => { const next = [...chosen]; const index = next.indexOf(name); [next[index], next[index + by]] = [next[index + by], next[index]]; setChosen(next) }

  return (
    <FullView crumbs={["Export"]} onClose={onClose}>
      <div
        className="flex min-h-0 flex-1 flex-col"
        onKeyDown={(e) => {
          if (e.key.toLowerCase() === "s" && (e.ctrlKey || e.metaKey)) {
            e.preventDefault()
            save()
          }
        }}
      >
        <SummaryStrip className="min-h-14">
          <Metric label="matching rows" className="min-w-0 bg-secondary [&>span:last-child]:text-[16px]">
            {rows.toLocaleString()}
          </Metric>
          <Metric label="all rows" className="min-w-0 [&>span:last-child]:text-[16px] [&>span:last-child]:text-muted-foreground">
            {allRows.toLocaleString()}
          </Metric>
          <Spacer />
          <StripAction ink keys="Ctrl S" disabled={blocked} onClick={save}>
            Save {file}
          </StripAction>
        </SummaryStrip>
        <div className="flex min-h-0 flex-1">
          <nav aria-label="Formats" className="w-60 shrink-0 overflow-y-auto border-r py-2">
            <ul role="listbox" aria-label="Format">
              {FORMATS.map((f) => (
                <li key={f.id} role="option" aria-selected={format === f.id}>
                  <button
                    type="button"
                    onClick={() => setFormat(f.id)}
                    className={cn(
                      "flex w-full items-baseline px-4 py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                      format === f.id ? "bg-foreground text-background" : "hover:bg-accent",
                    )}
                  >
                    <span className="flex-1">{f.label}</span>
                    <span className={cn("font-mono text-[11px]", format === f.id ? "text-background/70" : "text-muted-foreground")}>.{f.ext}</span>
                  </button>
                </li>
              ))}
            </ul>
          </nav>
          <div className="flex w-70 shrink-0 flex-col gap-4 overflow-y-auto border-r p-4">
            <p className="text-xs text-muted-foreground">{HINTS[format]}</p>
            {custom && <fieldset className="flex flex-col gap-2 border-b pb-3">
              <legend className="mb-2 text-[11.5px] text-muted-foreground">Export preset</legend>
              <select aria-label="Load export preset" className="h-8 border border-input bg-background px-2 text-xs" value="" disabled={!presets?.length || presetBusy} onChange={(e) => { const p = presets?.find((p) => p.name === e.target.value); if (p) applyPreset(p) }}>
                <option value="">Load saved settings…</option>{presets?.map((p) => <option key={p.name} value={p.name}>{p.name}</option>)}
              </select>
              <Input aria-label="Export preset name" placeholder="Preset name" maxLength={80} value={presetName} onChange={(e) => setPresetName(e.target.value)} className="h-8" />
              <div className="flex gap-2">
                <button className="border border-input px-2 py-1 text-xs disabled:opacity-50" disabled={!presets || presetBusy || !presetName.trim() || !ordered.length || (!presets.some((p) => p.name === presetName.trim()) && presets.length >= 20)} onClick={() => {
                  const name = presetName.trim(); if (!presets || !name || !ordered.length) return
                  const preset: Preset = { name, format, columns: selectedOptions, sv: { ...sv, outputBlankRows }, encoding, bom, replace }
                  if (!validPreset(preset)) { setPresetError("These settings exceed the supported preset limits."); return }
                  void storePresets([...presets.filter((p) => p.name !== name), preset])
                }}>{presets?.some((p) => p.name === presetName.trim()) ? "Update preset" : "Save preset"}</button>
                <button className="border border-input px-2 py-1 text-xs disabled:opacity-50" disabled={presetBusy || !presets?.some((p) => p.name === presetName.trim())} onClick={() => presets && void storePresets(presets.filter((p) => p.name !== presetName.trim()))}>Remove preset</button>
              </div>
              <p className="text-[11.5px] text-muted-foreground">Settings and column names only; no rows or filters are saved.</p>
              {presetNotice && <p className="text-[11.5px] text-muted-foreground">{presetNotice}</p>}
              {presetError && <p role="alert" className="text-xs text-destructive">{presetError}</p>}
            </fieldset>}
            {tabular && (
              <div className="flex flex-col gap-3">
                <label className="flex flex-col gap-1.5">
                  {label("Character encoding")}
                  <select
                    aria-label="Character encoding"
                    className="h-8 w-full rounded-[3px] border border-input bg-background px-2 text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    value={encoding}
                    onChange={(e) => { setEncoding(e.target.value); if (!e.target.value.startsWith("UTF-")) setBom(false) }}
                  >
                    {ENCODINGS.map((value) => <option key={value} value={value}>{value}</option>)}
                  </select>
                </label>
                {unicode ? check("Include byte-order mark (BOM)", bom, setBom) : (
                  <div>
                    {check("Replace unsupported characters with ?", replace, setReplace)}
                    <p className="mt-1 text-[11.5px] text-muted-foreground">
                      {replace ? "Characters outside this encoding will be replaced with ?." : "Export stops if a character cannot be represented. Choose UTF-8 to preserve all characters."}
                    </p>
                  </div>
                )}
              </div>
            )}
            {format === "*sv" && (
              <>
                <div className="flex flex-col gap-1.5">
                  {label("Separator")}
                  <Segmented label="Separator" value={sv.separator} options={[[",", "Comma"], ["\t", "Tab"], [";", "Semi"], ["|", "Pipe"]]} onChange={(v) => setSv({ ...sv, separator: v })} />
                </div>
                <div className="flex flex-col gap-1.5">
                  {label("Line endings")}
                  <Segmented label="Line endings" value={sv.lineSeparator} options={[["\n", "LF"], ["\r\n", "CRLF"]]} onChange={(v) => setSv({ ...sv, lineSeparator: v })} />
                </div>
                <div>
                  {check("Column headers", sv.outputColumnHeaders, (v) => setSv({ ...sv, outputColumnHeaders: v }))}
                  {check("Quote every cell", sv.quoteAll, (v) => setSv({ ...sv, quoteAll: v }))}
                  {check("Keep blank rows", outputBlankRows, (v) => setBlankRows((old) => ({ ...old, [format]: v })))}
                </div>
              </>
            )}
            {custom && format !== "*sv" && <div>{check("Column headers", sv.outputColumnHeaders, (v) => setSv({ ...sv, outputColumnHeaders: v }))}{check("Keep blank rows", outputBlankRows, (v) => setBlankRows((old) => ({ ...old, [format]: v })))}</div>}
            {format === "sql" && (
              <>
                <label className="flex flex-col gap-1.5">
                  {label("Table name")}
                  <Input value={sql.tableName} onChange={(e) => setSql({ ...sql, tableName: e.target.value })} className="h-8 font-mono" />
                </label>
                <div>
                  {check("DROP TABLE IF EXISTS first", sql.includeDropStatement, (v) => setSql({ ...sql, includeDropStatement: v }))}
                  {check("CREATE TABLE", sql.includeStructure, (v) => setSql({ ...sql, includeStructure: v }))}
                  {check("INSERT rows", sql.includeContent, (v) => setSql({ ...sql, includeContent: v }))}
                </div>
              </>
            )}
            {format === "template" &&
              (["prefix", "template", "separator", "suffix"] as const).map((k) => (
                <label key={k} className="flex flex-col gap-1.5">
                  {label({ prefix: "Prefix", template: "Row template", separator: "Row separator", suffix: "Suffix" }[k])}
                  <Textarea
                    className={k === "template" ? "h-40 font-mono text-xs" : "min-h-0 font-mono text-xs"}
                    rows={k === "template" ? undefined : 2}
                    value={tpl[k]}
                    onChange={(e) => setTpl({ ...tpl, [k]: e.target.value })}
                  />
                </label>
              ))}
            {(custom || format === "sql") && (
              <fieldset className="flex flex-col border-t pt-3">
                <legend className="flex w-full justify-between text-[11.5px] text-muted-foreground">
                  <span>Columns</span>
                  <span className="font-mono">
                    {ordered.length} of {columns.length}
                  </span>
                </legend>
                {[...ordered, ...columns.filter((c) => !ordered.includes(c))].map((c) => <div key={c} className="flex items-center gap-1">
                  <div className="min-w-0 flex-1">{check(c, chosen.includes(c), (on) => setChosen(on ? [...chosen, c] : chosen.filter((x) => x !== c)))}</div>
                  {ordered.includes(c) && <><button aria-label={`Move ${c} up`} disabled={ordered.indexOf(c) === 0} onClick={() => move(c, -1)} className="p-1 disabled:opacity-30"><ChevronUpIcon className="size-3.5" /></button><button aria-label={`Move ${c} down`} disabled={ordered.indexOf(c) === ordered.length - 1} onClick={() => move(c, 1)} className="p-1 disabled:opacity-30"><ChevronDownIcon className="size-3.5" /></button></>}
                </div>)}
              </fieldset>
            )}
            {custom && activeOptions && <fieldset className="flex flex-col gap-3 border-t pt-3">
              <legend className="text-[11.5px] text-muted-foreground">Column options</legend>
              <select aria-label="Column options" value={active} onChange={(e) => setEditing(e.target.value)} className="h-8 border border-input bg-background px-2 text-xs">{ordered.map((c) => <option key={c} value={c}>{c}</option>)}</select>
              <label className="flex flex-col gap-1.5">{label("Reconciled cells")}<select aria-label="Reconciled cell output" value={activeOptions.reconSettings.output} onChange={(e) => setActive({ ...activeOptions, reconSettings: { ...activeOptions.reconSettings, output: e.target.value } })} className="h-8 border border-input bg-background px-2 text-xs">
                <option value="entity-name">Matched entity name</option><option value="entity-id">Matched entity ID</option><option value="cell-content">Original cell content</option>
              </select></label>
              {check("Blank unmatched cells", activeOptions.reconSettings.blankUnmatchedCells, (v) => setActive({ ...activeOptions, reconSettings: { ...activeOptions.reconSettings, blankUnmatchedCells: v } }))}
              {format === "html" && check("Link matched entities in HTML", activeOptions.reconSettings.linkToEntityPages, (v) => setActive({ ...activeOptions, reconSettings: { ...activeOptions.reconSettings, linkToEntityPages: v } }))}
              <label className="flex flex-col gap-1.5">{label("Date format")}<select aria-label="Date format" value={activeOptions.dateSettings.format} onChange={(e) => setActive({ ...activeOptions, dateSettings: { ...activeOptions.dateSettings, format: e.target.value } })} className="h-8 border border-input bg-background px-2 text-xs">
                <option value="iso-8601">ISO 8601</option><option value="locale-short">Short (English)</option><option value="locale-medium">Medium (English)</option><option value="locale-long">Long (English)</option><option value="locale-full">Full (English)</option><option value="custom">Custom pattern</option>
              </select></label>
              {activeOptions.dateSettings.format === "custom" && <Input aria-label="Custom date pattern" maxLength={256} value={activeOptions.dateSettings.custom} onChange={(e) => setActive({ ...activeOptions, dateSettings: { ...activeOptions.dateSettings, custom: e.target.value } })} className="h-8 font-mono" />}
              {activeOptions.dateSettings.format !== "custom" && check("Omit time", activeOptions.dateSettings.omitTime, (v) => setActive({ ...activeOptions, dateSettings: { ...activeOptions.dateSettings, omitTime: v } }))}
              {check("Use server local time", activeOptions.dateSettings.useLocalTimeZone, (v) => setActive({ ...activeOptions, dateSettings: { ...activeOptions.dateSettings, useLocalTimeZone: v } }))}
              <p className="text-[11.5px] text-muted-foreground">Custom patterns support yy/yyyy, M–MMMM, d/dd, E–EEEE, H/HH, h/hh, m/mm, s/ss, SSS, a and simple quoted text.</p>
              {workbook && <p className="text-[11.5px] text-muted-foreground">Workbooks keep original numbers, booleans and default dates, including reconciled cells. Use CSV/HTML for entity text on these cells. Custom formats, omitted time or local time export dates as text. Hyperlinks are available in HTML.</p>}
            </fieldset>}
          </div>
          <div className="flex min-w-0 flex-1 flex-col bg-muted">
            <div className="flex h-10 shrink-0 items-center gap-3 border-b px-5">
              <span className="font-semibold">Preview</span>
              <span className="font-mono text-xs text-muted-foreground">{spec.text ? tabular ? "first 10 matching rows · decoded text, BOM hidden" : "first 10 matching rows · as written" : "no text preview"}</span>
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-5">
              {!spec.text ? (
                <p className="text-muted-foreground">{HINTS[format]}</p>
              ) : shown.error ? (
                <p role="alert" className="text-destructive">
                  {shown.error}
                </p>
              ) : (
                <pre className="font-mono text-[12.5px] leading-relaxed whitespace-pre">{shown.text?.replaceAll("\t", "→\t")}</pre>
              )}
            </div>
          </div>
        </div>
      </div>
    </FullView>
  )
}
