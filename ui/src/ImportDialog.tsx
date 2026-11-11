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

/** The guessed format and OpenRefine's wizard defaults for it. */
type Info = { format: "csv" | "lines" | "excel" | "ods" | "json" | "jsonlines"; sheets?: string[]; separator?: string; options: Options }
type Options = Record<string, unknown>
type Preview = { columns: string[]; rows: [number, (string | null)[], boolean, boolean][] }

const ENCODINGS = ["UTF-8", "windows-1252", "ISO-8859-1", "UTF-16LE", "UTF-16BE", "windows-1251", "KOI8-R", "Shift_JIS", "GBK"]
const SEPARATORS: [string, string][] = [[",", "Comma"], [";", "Semicolon"], ["\t", "Tab"], ["|", "Pipe"]]

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p

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
        setOptions(data.options)
      })
      .catch((e) => setError(e.message))
  }, [path])

  useEffect(() => {
    if (!info) return
    const t = setTimeout(() => {
      call<Preview>("preview_import", { path, format: info.format, options, limit: 50 })
        .then(({ data }) => (setPreview(data), setError("")))
        .catch((e) => (setPreview(null), setError(e.message)))
    }, 300)
    return () => clearTimeout(t)
  }, [path, info, options])

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
  const sheets = (options.sheets as { fileNameAndSheetIndex: string; selected: boolean; name: string }[]) ?? []

  const create = async () => {
    setBusy(true)
    try {
      await onCreate({ path, name: name.trim() || label, format: info!.format, options })
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
              ? `Detected ${{ csv: "delimited text", lines: "plain lines (one row per line)" }[info.format as string] ?? info.format.toUpperCase()}. The preview updates as you change the options.`
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
              {info?.format === "csv" && (
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
                  <Field>
                    <FieldLabel>Character encoding</FieldLabel>
                    <Select value={String(options.encoding)} onValueChange={(v) => set({ encoding: v })}>
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
                </>
              )}
              {(info?.format === "excel" || info?.format === "ods") && (
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
              {(info?.format === "json" || info?.format === "jsonlines") && (
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
              {info?.format === "lines" && num("linesPerRow", "Lines per row", "Each group of this many lines becomes one row.")}
              {info && info.format !== "json" && info.format !== "jsonlines" && (
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
                  {info.format === "csv" && check("processQuotes", "Quotes group text with separators in it")}
                  {(info.format === "csv" || info.format.startsWith("json")) && check("trimStrings", "Trim leading and trailing spaces")}
                  {(info.format === "csv" || info.format.startsWith("json")) && check("guessCellValueTypes", "Turn text that looks like numbers into numbers")}
                  {info.format !== "json" && info.format !== "jsonlines" && check("storeBlankRows", "Keep blank rows")}
                  {info.format !== "json" && info.format !== "jsonlines" && check("storeBlankCellsAsNulls", "Store blank cells as empty (null)")}
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
