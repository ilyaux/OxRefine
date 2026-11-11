import { useEffect, useState } from "react"
import { call, type Engine } from "@/api"
import { useDebounced } from "@/Dialogs"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

export type ExportFormat = "*sv" | "template" | "sql"
export const EXTENSION: Record<ExportFormat, string> = { "*sv": "csv", template: "txt", sql: "sql" }

const TITLES: Record<ExportFormat, [string, string]> = {
  "*sv": ["Custom tabular export", "Pick columns, separator and quoting. Rows follow the current facets."],
  template: ["Templating export", "Each row renders the template; {{ … }} holds a GREL expression such as cells[\"name\"].value."],
  sql: ["SQL export", "CREATE TABLE and INSERT statements for the matching rows. Blank cells become NULL."],
}

/** OpenRefine's default template: one JSON object per row. */
const defaultTemplate = (columns: string[]) => ({
  prefix: '{\n  "rows" : [\n',
  template: `    {\n${columns.map((c) => `      ${JSON.stringify(c)} : {{jsonize(cells[${JSON.stringify(c)}].value)}}`).join(",\n")}\n    }`,
  separator: ",\n",
  suffix: "\n  ]\n}",
})

export function ExportDialog({
  format,
  columns,
  engine,
  onExport,
  onClose,
}: {
  format: ExportFormat
  columns: string[]
  engine: Engine
  onExport: (params: object) => void
  onClose: () => void
}) {
  const [chosen, setChosen] = useState(columns)
  const [sv, setSv] = useState({ separator: ",", lineSeparator: "\n", outputColumnHeaders: true, quoteAll: false, outputBlankRows: true })
  const [tpl, setTpl] = useState(() => defaultTemplate(columns))
  const [sql, setSql] = useState({ tableName: "data", includeDropStatement: false, includeStructure: true, includeContent: true })

  const ordered = columns.filter((c) => chosen.includes(c))
  const params =
    format === "template"
      ? tpl
      : format === "sql"
        ? { options: { ...sql, includeIfExistWithDropStatement: sql.includeDropStatement, columns: ordered.map((name) => ({ name, type: "VARCHAR", size: 255, allowNull: true })) } }
        : { options: { ...sv, columns: ordered.map((name) => ({ name })) } }

  const key = useDebounced(JSON.stringify(params), 250)
  const [preview, setPreview] = useState<{ text?: string; error?: string }>({})
  useEffect(() => {
    let live = true
    call<string>("export", { format, params: JSON.parse(key), engine, limit: 10 }).then(
      ({ data }) => live && setPreview({ text: data }),
      (e) => live && setPreview({ error: String((e as Error).message ?? e) }),
    )
    return () => {
      live = false
    }
  }, [format, key, engine])

  const check = (id: string, label: string, on: boolean, set: (v: boolean) => void) => (
    <Field orientation="horizontal" key={id}>
      <Checkbox id={`exp-${id}`} checked={on} onCheckedChange={(c) => set(!!c)} />
      <FieldLabel htmlFor={`exp-${id}`} className="font-normal">
        {label}
      </FieldLabel>
    </Field>
  )
  const columnPicker = (
    <FieldSet>
      <FieldLegend variant="label">Columns</FieldLegend>
      <ScrollArea className="max-h-48 rounded-md border p-2">
        <div className="flex flex-col gap-1">
          {columns.map((c, i) => check(`col-${i}`, c, chosen.includes(c), (on) => setChosen(on ? [...chosen, c] : chosen.filter((x) => x !== c))))}
        </div>
      </ScrollArea>
    </FieldSet>
  )
  const [title, description] = TITLES[format]

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-4xl">
        <DialogHeader>
          <DialogTitle>{title}</DialogTitle>
          <DialogDescription>{description}</DialogDescription>
        </DialogHeader>
        <div className="grid min-h-0 gap-4 md:grid-cols-2">
          <FieldGroup>
            {format === "*sv" && (
              <>
                <Field>
                  <FieldLabel htmlFor="exp-sep">Separator</FieldLabel>
                  <ToggleGroup variant="outline" value={[sv.separator]} onValueChange={(v) => v[0] !== undefined && setSv({ ...sv, separator: v[0] })}>
                    <ToggleGroupItem value=",">Comma</ToggleGroupItem>
                    <ToggleGroupItem value={"\t"}>Tab</ToggleGroupItem>
                    <ToggleGroupItem value=";">Semicolon</ToggleGroupItem>
                    <ToggleGroupItem value="|">Pipe</ToggleGroupItem>
                  </ToggleGroup>
                </Field>
                <Field>
                  <FieldLabel>Line endings</FieldLabel>
                  <ToggleGroup variant="outline" value={[sv.lineSeparator]} onValueChange={(v) => v[0] && setSv({ ...sv, lineSeparator: v[0] })}>
                    <ToggleGroupItem value={"\n"}>LF</ToggleGroupItem>
                    <ToggleGroupItem value={"\r\n"}>CRLF</ToggleGroupItem>
                  </ToggleGroup>
                </Field>
                {check("headers", "Column headers", sv.outputColumnHeaders, (v) => setSv({ ...sv, outputColumnHeaders: v }))}
                {check("quote", "Quote every cell", sv.quoteAll, (v) => setSv({ ...sv, quoteAll: v }))}
                {check("blank", "Keep blank rows", sv.outputBlankRows, (v) => setSv({ ...sv, outputBlankRows: v }))}
                {columnPicker}
              </>
            )}
            {format === "sql" && (
              <>
                <Field>
                  <FieldLabel htmlFor="exp-table">Table name</FieldLabel>
                  <Input id="exp-table" value={sql.tableName} onChange={(e) => setSql({ ...sql, tableName: e.target.value })} />
                </Field>
                {check("drop", "DROP TABLE IF EXISTS first", sql.includeDropStatement, (v) => setSql({ ...sql, includeDropStatement: v }))}
                {check("structure", "CREATE TABLE", sql.includeStructure, (v) => setSql({ ...sql, includeStructure: v }))}
                {check("content", "INSERT rows", sql.includeContent, (v) => setSql({ ...sql, includeContent: v }))}
                {columnPicker}
              </>
            )}
            {format === "template" &&
              (["prefix", "template", "separator", "suffix"] as const).map((k) => (
                <Field key={k}>
                  <FieldLabel htmlFor={`exp-${k}`}>{{ prefix: "Prefix", template: "Row template", separator: "Row separator", suffix: "Suffix" }[k]}</FieldLabel>
                  <Textarea
                    id={`exp-${k}`}
                    className={k === "template" ? "h-40 font-mono" : "min-h-0 font-mono"}
                    rows={k === "template" ? undefined : 2}
                    value={tpl[k]}
                    onChange={(e) => setTpl({ ...tpl, [k]: e.target.value })}
                  />
                </Field>
              ))}
          </FieldGroup>
          <Field className="min-h-0">
            <FieldLabel>Preview (first 10 matching rows)</FieldLabel>
            {preview.error ? (
              <p className="text-sm text-destructive">{preview.error}</p>
            ) : (
              <ScrollArea className="h-96 rounded-md border bg-muted">
                <pre className="p-2 font-mono text-xs whitespace-pre">{preview.text?.replaceAll("\t", "→\t")}</pre>
              </ScrollArea>
            )}
          </Field>
        </div>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button disabled={format !== "template" && !ordered.length} onClick={() => onExport(params)}>
            Export…
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
