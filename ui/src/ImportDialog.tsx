import { useEffect, useState } from "react"
import { call } from "@/api"
import { Button } from "@/components/ui/button"
import { Checkbox } from "@/components/ui/checkbox"
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Field, FieldDescription, FieldError, FieldGroup, FieldLabel, FieldSet, FieldLegend } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { ScrollArea } from "@/components/ui/scroll-area"
import { Select, SelectContent, SelectGroup, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"

type Format = "csv" | "lines" | "fixed-width" | "excel" | "ods" | "json" | "jsonlines" | "xml"
/** The guessed format and OpenRefine's wizard defaults for it; for an archive or folder, its files;
 * for XML, the element paths a record can be. */
type Info = {
  format: Format
  sheets?: string[]
  separator?: string
  options: Options
  files?: { name: string; size: number; format: Format | null; selected: boolean }[]
  archive?: string | null
  elements?: { path: string[]; count: number }[]
}
type Options = Record<string, unknown>
type Preview = { columns: string[]; rows: [number, (string | null)[], boolean, boolean][] }

const ENCODINGS = ["UTF-8", "windows-1252", "ISO-8859-1", "UTF-16LE", "UTF-16BE", "windows-1251", "KOI8-R", "Shift_JIS", "GBK"]
const SEPARATORS: [string, string][] = [[",", "Comma"], [";", "Semicolon"], ["\t", "Tab"], ["|", "Pipe"]]

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p
const TEXT: [Format, string][] = [["csv", "Delimited"], ["lines", "Plain lines"], ["fixed-width", "Fixed width"]]
const KIND: Partial<Record<Format, string>> = { csv: "delimited text", lines: "plain lines (one row per line)", "fixed-width": "fixed-width text" }

export function ImportDialog({
  path,
  name: initialName,
  onCreate,
  onClose,
}: {
  path: string
  /** project name to suggest; defaults to the file name */
  name?: string
  onCreate: (args: { path: string; name: string; format: string; options: Options }) => Promise<void>
  onClose: () => void
}) {
  const [info, setInfo] = useState<Info | null>(null)
  const [format, setFormat] = useState<Format>("csv")
  const [options, setOptions] = useState<Options>({})
  const label = initialName ?? baseName(path)
  const [name, setName] = useState(label)
  const [preview, setPreview] = useState<Preview | null>(null)
  const [error, setError] = useState("")
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    call<Info>("inspect", { path })
      .then(({ data }) => {
        setInfo(data)
        setFormat(data.format)
        setOptions({ ...data.options, ...(data.files ? { files: data.files.filter((f) => f.selected).map((f) => f.name) } : {}) })
      })
      .catch((e) => setError(e.message))
  }, [path])

  useEffect(() => {
    if (!info) return
    const t = setTimeout(() => {
      call<Preview>("preview_import", { path, format, options, limit: 50 })
        .then(({ data }) => (setPreview(data), setError("")))
        .catch((e) => (setPreview(null), setError(e.message)))
    }, 300)
    return () => clearTimeout(t)
  }, [path, info, format, options])

  const set = (patch: Options) => setOptions((o) => ({ ...o, ...patch }))
  const num = (key: string, label: string, help?: string) => (
    <Field>
      <FieldLabel htmlFor={`imp-${key}`}>{label}</FieldLabel>
      <Input
        id={`imp-${key}`}
        type="number"
        min={key === "limit" ? undefined : 0}
        value={key === "limit" ? (Number(options[key]) < 0 ? "" : String(options[key])) : String(Math.max(0, Number(options[key] ?? 0)))}
        placeholder={key === "limit" ? "All rows" : undefined}
        onChange={(e) => set({ [key]: e.target.value === "" ? (key === "limit" ? -1 : 0) : Number(e.target.value) })}
      />
      {help && <FieldDescription>{help}</FieldDescription>}
    </Field>
  )
  const check = (key: string, label: string) => (
    <Field orientation="horizontal">
      <Checkbox id={`imp-${key}`} checked={Boolean(options[key])} onCheckedChange={(c) => set({ [key]: !!c })} />
      <FieldLabel htmlFor={`imp-${key}`}>{label}</FieldLabel>
    </Field>
  )
  const sep = String(options.separator ?? ",")
  // JSON and XML: records from a tree, not lines
  const tree = format === "json" || format === "jsonlines" || format === "xml"
  const sheets = (options.sheets as { fileNameAndSheetIndex: string; selected: boolean; name: string }[]) ?? []

  const create = async () => {
    setBusy(true)
    try {
      await onCreate({ path, name: name.trim() || label, format, options })
    } catch (e) {
      setError((e as Error).message)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="sm:max-w-6xl">
        <DialogHeader>
          <DialogTitle>Import {label}</DialogTitle>
          <DialogDescription>
            {info
              ? `${info.files ? `${info.files.length} files inside. ` : ""}Detected ${KIND[info.format] ?? info.format.toUpperCase()}. The preview updates as you change the options.`
              : "Reading the file…"}
          </DialogDescription>
        </DialogHeader>
        <div className="grid min-h-0 gap-6 md:grid-cols-[18rem_minmax(0,1fr)]">
          <ScrollArea className="max-h-[60vh] pr-3">
            <FieldGroup>
              <Field>
                <FieldLabel htmlFor="imp-name">Project name</FieldLabel>
                <Input id="imp-name" value={name} onChange={(e) => setName(e.target.value)} />
              </Field>
              {info?.files && (
                <FieldSet>
                  <FieldLegend variant="label">Files to import</FieldLegend>
                  {info.files.map((f, i) => {
                    const picked = (options.files as string[]) ?? []
                    return (
                      <Field key={f.name} orientation="horizontal">
                        <Checkbox
                          id={`imp-file-${i}`}
                          checked={picked.includes(f.name)}
                          onCheckedChange={(c) => set({ files: c ? [...picked, f.name] : picked.filter((x) => x !== f.name) })}
                        />
                        <FieldLabel htmlFor={`imp-file-${i}`} className="min-w-0">
                          <span className="truncate">{f.name}</span>
                        </FieldLabel>
                      </Field>
                    )
                  })}
                  {check("includeFileSources", "Add a File column with each row's file")}
                  {info.archive && check("includeArchiveFileName", "Add an Archive column with the archive's name")}
                </FieldSet>
              )}
              {TEXT.some(([f]) => f === info?.format) && (
                <Field>
                  <FieldLabel>Read the text as</FieldLabel>
                  <ToggleGroup variant="outline" size="sm" value={[format]} onValueChange={(v) => v[0] && setFormat(v[0] as Format)}>
                    {TEXT.map(([v, l]) => (
                      <ToggleGroupItem key={v} value={v}>
                        {l}
                      </ToggleGroupItem>
                    ))}
                  </ToggleGroup>
                </Field>
              )}
              {format === "fixed-width" && (
                <Field>
                  <FieldLabel htmlFor="imp-widths">Column widths</FieldLabel>
                  <Input
                    id="imp-widths"
                    className="font-mono"
                    placeholder="5, 10, 9"
                    defaultValue={((options.columnWidths as number[]) ?? []).join(", ")}
                    onChange={(e) => set({ columnWidths: e.target.value.split(/[\s,]+/).map(Number).filter((n) => n > 0) })}
                  />
                  <FieldDescription>Characters in each column, left to right. Whatever is past the last one becomes one more column.</FieldDescription>
                </Field>
              )}
              {format === "xml" && info?.elements && (
                <Field>
                  <FieldLabel>Each record is an element</FieldLabel>
                  <Select value={JSON.stringify(options.recordPath ?? [])} onValueChange={(v) => v && set({ recordPath: JSON.parse(String(v)) })}>
                    <SelectTrigger aria-label="Record element">
                      <SelectValue>{(v: string) => (JSON.parse(v ?? "[]") as string[]).join(" › ")}</SelectValue>
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {info.elements.map((e) => (
                          <SelectItem key={e.path.join("/")} value={JSON.stringify(e.path)}>
                            {e.path.join(" › ")} ({e.count.toLocaleString()})
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                  <FieldDescription>Its attributes and the elements inside it become columns.</FieldDescription>
                </Field>
              )}
              {(format === "csv" || format === "lines" || format === "fixed-width") && (
                <Field>
                  <FieldLabel>Character encoding</FieldLabel>
                  <Select value={String(options.encoding ?? "UTF-8")} onValueChange={(v) => set({ encoding: v })}>
                    <SelectTrigger>
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectGroup>
                        {ENCODINGS.map((e) => (
                          <SelectItem key={e} value={e}>
                            {e}
                          </SelectItem>
                        ))}
                      </SelectGroup>
                    </SelectContent>
                  </Select>
                </Field>
              )}
              {format === "csv" && (
                <>
                  <Field>
                    <FieldLabel>Columns separated by</FieldLabel>
                    <ToggleGroup variant="outline" size="sm" value={[sep]} onValueChange={(v) => v[0] !== undefined && set({ separator: v[0] })}>
                      {SEPARATORS.map(([v, l]) => (
                        <ToggleGroupItem key={l} value={v}>
                          {l}
                        </ToggleGroupItem>
                      ))}
                    </ToggleGroup>
                  </Field>
                </>
              )}
              {(format === "excel" || format === "ods") && info && (
                <FieldSet>
                  <FieldLegend variant="label">Sheets to import</FieldLegend>
                  {info.sheets?.map((s, i) => {
                    const on = sheets.some((x) => x.fileNameAndSheetIndex === `file#${i}` && x.selected)
                    return (
                      <Field key={s} orientation="horizontal">
                        <Checkbox
                          id={`imp-sheet-${i}`}
                          checked={on}
                          onCheckedChange={(c) =>
                            set({
                              sheets: c
                                ? [...sheets, { name: s, fileNameAndSheetIndex: `file#${i}`, selected: true }]
                                : sheets.filter((x) => x.fileNameAndSheetIndex !== `file#${i}`),
                            })
                          }
                        />
                        <FieldLabel htmlFor={`imp-sheet-${i}`}>{s}</FieldLabel>
                      </Field>
                    )
                  })}
                </FieldSet>
              )}
              {(format === "json" || format === "jsonlines") && (
                <Field>
                  <FieldLabel htmlFor="imp-path">Path to the records</FieldLabel>
                  <Input
                    id="imp-path"
                    className="font-mono"
                    value={(options.recordPath as string[]).join(", ")}
                    onChange={(e) => set({ recordPath: e.target.value.split(",").map((s) => s.trim()).filter(Boolean) })}
                  />
                  <FieldDescription>Field names from the top, with _ for the items of an array. A list of objects is “_, _”.</FieldDescription>
                </Field>
              )}
              {format === "lines" && num("linesPerRow", "Lines per row", "Each group of this many lines becomes one row.")}
              {info && !tree && (
                <>
                  {num("ignoreLines", "Lines to ignore at the top")}
                  {num("headerLines", "Header lines", "0 names the columns Column 1, Column 2…")}
                  {num("skipDataLines", "Data lines to skip")}
                </>
              )}
              {info && num("limit", "Load at most this many rows")}
              {info && (
                <FieldSet>
                  <FieldLegend variant="label">Cells</FieldLegend>
                  {format === "csv" && check("processQuotes", "Quotes group text with separators in it")}
                  {(format === "csv" || tree) && check("trimStrings", "Trim leading and trailing spaces")}
                  {(format === "csv" || format === "fixed-width" || tree) && check("guessCellValueTypes", "Turn text that looks like numbers into numbers")}
                  {!tree && check("storeBlankRows", "Keep blank rows")}
                  {!tree && check("storeBlankCellsAsNulls", "Store blank cells as empty (null)")}
                  {tree && check("storeEmptyStrings", "Keep empty text as an empty string")}
                </FieldSet>
              )}
            </FieldGroup>
          </ScrollArea>
          <div className="flex min-w-0 flex-col gap-2">
            {error && <FieldError>{error}</FieldError>}
            <ScrollArea className="h-[60vh] rounded-lg border">
              {!preview ? (
                <div className="flex h-[60vh] items-center justify-center">{error ? null : <Spinner />}</div>
              ) : (
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead className="w-10 text-right">#</TableHead>
                      {preview.columns.map((c) => (
                        <TableHead key={c}>{c}</TableHead>
                      ))}
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {preview.rows.map(([i, cells]) => (
                      <TableRow key={i}>
                        <TableCell className="text-right text-muted-foreground tabular-nums">{i + 1}</TableCell>
                        {cells.map((v, c) => (
                          <TableCell key={c} className="max-w-48 truncate" title={v ?? undefined}>
                            {v}
                          </TableCell>
                        ))}
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </ScrollArea>
            {preview && <p className="text-sm text-muted-foreground">Showing the first {preview.rows.length} rows of the preview.</p>}
          </div>
        </div>
        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button disabled={!preview || busy} onClick={create}>
            {busy && <Spinner data-icon="inline-start" />}
            Create project
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
