import { useEffect, useState } from "react"
import { call, type Engine } from "@/api"
import { useDebounced } from "@/Dialogs"
import { FullView, Metric, Spacer, StripAction, SummaryStrip } from "@/components/instrument"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { cn } from "@/lib/utils"

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

const HINTS: Partial<Record<ExportKind, string>> = {
  csv: "Every column, comma-separated, with a header row.",
  tsv: "Every column, tab-separated, with a header row.",
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
  const spec = FORMATS.find((f) => f.id === format)!

  const ordered = columns.filter((c) => chosen.includes(c))
  const params =
    format === "template"
      ? tpl
      : format === "sql"
        ? { options: { ...sql, includeIfExistWithDropStatement: sql.includeDropStatement, columns: ordered.map((n) => ({ name: n, type: "VARCHAR", size: 255, allowNull: true })) } }
        : format === "*sv"
          ? { options: { ...sv, columns: ordered.map((n) => ({ name: n })) } }
          : {}

  const key = useDebounced(JSON.stringify([format, params]), 250)
  const [preview, setPreview] = useState<{ key?: string; text?: string; error?: string }>({})
  useEffect(() => {
    const [f, p] = JSON.parse(key) as [ExportKind, object]
    if (!FORMATS.find((x) => x.id === f)?.text) return
    let live = true
    call<string>("export", { format: f, params: p, engine, limit: 10 }).then(
      ({ data }) => live && setPreview({ key, text: data }),
      (e) => live && setPreview({ key, error: String((e as Error).message ?? e) }),
    )
    return () => {
      live = false
    }
  }, [key, engine])
  const shown = preview.key === key ? preview : {}

  const blocked = (format === "*sv" || format === "sql") && !ordered.length
  const file = `${name.replace(/\.[^.]+$/, "")}.${format === "project" ? "openrefine.tar.gz" : spec.ext}`
  const save = () => !blocked && onExport(format, spec.ext, params)
  const check = (label: string, on: boolean, set: (v: boolean) => void) => (
    <label key={label} className="flex cursor-pointer items-center gap-2.5 py-1">
      <input type="checkbox" checked={on} onChange={(e) => set(e.target.checked)} className="accent-foreground" />
      {label}
    </label>
  )
  const label = (text: string) => <span className="text-[11.5px] text-muted-foreground">{text}</span>

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
                  {check("Keep blank rows", sv.outputBlankRows, (v) => setSv({ ...sv, outputBlankRows: v }))}
                </div>
              </>
            )}
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
            {(format === "*sv" || format === "sql") && (
              <fieldset className="flex flex-col border-t pt-3">
                <legend className="flex w-full justify-between text-[11.5px] text-muted-foreground">
                  <span>Columns</span>
                  <span className="font-mono">
                    {ordered.length} of {columns.length}
                  </span>
                </legend>
                {columns.map((c) => check(c, chosen.includes(c), (on) => setChosen(on ? [...chosen, c] : chosen.filter((x) => x !== c))))}
              </fieldset>
            )}
          </div>
          <div className="flex min-w-0 flex-1 flex-col bg-muted">
            <div className="flex h-10 shrink-0 items-center gap-3 border-b px-5">
              <span className="font-semibold">Preview</span>
              <span className="font-mono text-xs text-muted-foreground">{spec.text ? "first 10 matching rows · as written" : "no text preview"}</span>
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
