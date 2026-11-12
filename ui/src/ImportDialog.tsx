import { useEffect, useState } from "react"
import { call } from "@/api"
import { FullView, Metric, RailLabel, Spacer, StripAction, SummaryStrip } from "@/components/instrument"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { cn } from "@/lib/utils"

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
const SEPARATORS: [string, string, string][] = [[",", ",", "Comma"], ["\t", "\\t", "Tab"], [";", ";", "Semicolon"], ["|", "|", "Pipe"]]

const baseName = (p: string) => p.split(/[\\/]/).pop() ?? p
const TEXT: [Format, string, string][] = [["csv", "CSV / TSV", "csv tsv"], ["fixed-width", "Fixed-width", "txt"], ["lines", "Lines", "txt"]]
const NAMES: Record<Format, string> = { csv: "CSV", lines: "Lines", "fixed-width": "Fixed-width", excel: "Excel", ods: "OpenDocument", json: "JSON", jsonlines: "JSON lines", xml: "XML" }
const size = (n: number) => (n >= 1e9 ? `${(n / 1e9).toFixed(1)} GB` : n >= 1e6 ? `${(n / 1e6).toFixed(1)} MB` : n >= 1e3 ? `${(n / 1e3).toFixed(0)} KB` : `${n} B`)

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
  const [ms, setMs] = useState<number | null>(null)
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
        .then(({ data, ms }) => (setPreview(data), setMs(ms), setError("")))
        .catch((e) => (setPreview(null), setError(e.message)))
    }, 300)
    return () => clearTimeout(t)
  }, [path, info, format, options])

  const set = (patch: Options) => setOptions((o) => ({ ...o, ...patch }))
  // a numeric option as a label and a short mono input on one line
  const num = (key: string, label: string, help?: string) => (
    <label className="flex items-center gap-2 px-4 py-1.5" title={help}>
      <span className="min-w-0 flex-1">{label}</span>
      <input
        type="number"
        min={key === "limit" ? undefined : 0}
        value={key === "limit" ? (Number(options[key]) < 0 ? "" : String(options[key])) : String(Math.max(0, Number(options[key] ?? 0)))}
        placeholder={key === "limit" ? "all" : undefined}
        onChange={(e) => set({ [key]: e.target.value === "" ? (key === "limit" ? -1 : 0) : Number(e.target.value) })}
        className="h-7.5 w-24 rounded-[3px] border border-input bg-background px-2 text-right font-mono text-[12.5px] outline-none placeholder:text-muted-foreground focus-visible:ring-2 focus-visible:ring-ring"
      />
    </label>
  )
  const check = (key: string, label: string) => (
    <label key={key} className="flex cursor-pointer items-center gap-2.5 px-4 py-1.5 hover:bg-accent">
      <input type="checkbox" checked={Boolean(options[key])} onChange={(e) => set({ [key]: e.target.checked })} className="accent-foreground" />
      {label}
    </label>
  )
  const radio = (key: string, on: boolean, label: string, hint: string, pick: () => void) => (
    <label key={key} className={cn("flex cursor-pointer items-center gap-2.5 px-4 py-1.5 hover:bg-accent", on && "bg-secondary")}>
      <input type="radio" name="imp-format" checked={on} onChange={pick} className="accent-foreground" />
      <span className="flex-1">{label}</span>
      <span className="font-mono text-[11px] text-muted-foreground">{hint}</span>
    </label>
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

  const file = baseName(path)
  const detected = info
    ? [
        NAMES[format] ?? format,
        ...(format === "csv" || format === "lines" || format === "fixed-width" ? [String(options.encoding ?? "UTF-8")] : []),
        ...(format === "csv" ? [`“${sep === "\t" ? "\\t" : sep}”`] : []),
        ...(!tree && Number(options.headerLines) > 0 ? ["header"] : []),
      ].join(" · ")
    : "…"
  return (
    <FullView root="Projects" crumbs={["New project"]} closeLabel="Cancel" onClose={onClose}>
      <div
        className="flex min-h-0 flex-1 flex-col"
        onKeyDown={(e) => {
          if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && preview && !busy) {
            e.preventDefault()
            create()
          }
        }}
      >
        <SummaryStrip>
          <Metric label="file" className="min-w-0 max-w-90 [&>span:last-child]:truncate [&>span:last-child]:text-[12.5px] [&>span:last-child]:font-normal">
            {file}
            {info?.files && ` · ${info.files.length} files`}
          </Metric>
          <label className="flex w-80 flex-col justify-center gap-1 border-r px-5">
            <span className="text-[11.5px] text-muted-foreground">project name</span>
            <Input value={name} onChange={(e) => setName(e.target.value)} className="h-8" />
          </label>
          <Metric label="detected" className="[&>span:last-child]:text-[12.5px] [&>span:last-child]:font-normal">
            {detected}
          </Metric>
          <Spacer />
          <StripAction ink keys="Ctrl ↵" disabled={!preview || busy} onClick={create}>
            {busy && <Spinner />}
            Create project
          </StripAction>
        </SummaryStrip>
        <div className="flex min-h-0 flex-1">
          <aside aria-label="Import options" className="w-65 shrink-0 overflow-y-auto border-r pb-4">
            {info?.files && (
              <>
                <RailLabel>Files to import</RailLabel>
                {info.files.map((f) => {
                  const picked = (options.files as string[]) ?? []
                  return (
                    <label key={f.name} className="flex cursor-pointer items-center gap-2.5 px-4 py-1.5 hover:bg-accent">
                      <input
                        type="checkbox"
                        checked={picked.includes(f.name)}
                        onChange={(e) => set({ files: e.target.checked ? [...picked, f.name] : picked.filter((x) => x !== f.name) })}
                        className="accent-foreground"
                      />
                      <span className="min-w-0 flex-1 truncate">{f.name}</span>
                      <span className="font-mono text-[11px] text-muted-foreground">{size(f.size)}</span>
                    </label>
                  )
                })}
                {check("includeFileSources", "Add a File column")}
                {info.archive && check("includeArchiveFileName", "Add an Archive column")}
                <div className="mt-2 border-t" />
              </>
            )}
            {TEXT.some(([f]) => f === info?.format) ? (
              <>
                <RailLabel>Read the text as</RailLabel>
                <div role="radiogroup" aria-label="Read the text as">
                  {TEXT.map(([v, l, h]) => radio(v, format === v, l, h, () => setFormat(v)))}
                </div>
              </>
            ) : (
              info && (
                <>
                  <RailLabel>Read as</RailLabel>
                  <div className="px-4 py-1.5">{NAMES[info.format] ?? info.format}</div>
                </>
              )
            )}
            {info && <div className="mt-2 border-t" />}
            {format === "csv" && (
              <>
                <RailLabel>Columns separated by</RailLabel>
                <div role="radiogroup" aria-label="Columns separated by" className="mx-4 flex">
                  {SEPARATORS.map(([v, shown, l]) => (
                    <button
                      key={l}
                      type="button"
                      role="radio"
                      aria-checked={sep === v}
                      aria-label={l}
                      onClick={() => set({ separator: v })}
                      className={cn(
                        "h-7.5 flex-1 border border-input font-mono text-[12.5px] outline-none not-first:-ml-px first:rounded-l-[3px] last:rounded-r-[3px] focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-ring",
                        sep === v ? "z-1 border-foreground bg-foreground text-background" : "hover:bg-accent",
                      )}
                    >
                      {shown}
                    </button>
                  ))}
                </div>
              </>
            )}
            {format === "fixed-width" && (
              <label className="flex flex-col gap-1 px-4 pt-2" title="Characters in each column, left to right. Whatever is past the last one becomes one more column.">
                <span>Column widths</span>
                <Input
                  className="h-7.5 font-mono"
                  placeholder="5, 10, 9"
                  defaultValue={((options.columnWidths as number[]) ?? []).join(", ")}
                  onChange={(e) => set({ columnWidths: e.target.value.split(/[\s,]+/).map(Number).filter((n) => n > 0) })}
                />
              </label>
            )}
            {format === "xml" && info?.elements && (
              <label className="flex flex-col gap-1 px-4 pt-2" title="Its attributes and the elements inside it become columns.">
                <span>Each record is an element</span>
                <select
                  value={JSON.stringify(options.recordPath ?? [])}
                  onChange={(e) => set({ recordPath: JSON.parse(e.target.value) })}
                  className="h-7.5 rounded-[3px] border border-input bg-background px-2 font-mono text-[12px]"
                >
                  {info.elements.map((el) => (
                    <option key={el.path.join("/")} value={JSON.stringify(el.path)}>
                      {el.path.join(" › ")} ({el.count.toLocaleString()})
                    </option>
                  ))}
                </select>
              </label>
            )}
            {(format === "json" || format === "jsonlines") && (
              <label className="flex flex-col gap-1 px-4 pt-2" title="Field names from the top, with _ for the items of an array. A list of objects is “_, _”.">
                <span>Path to the records</span>
                <Input
                  className="h-7.5 font-mono"
                  value={(options.recordPath as string[]).join(", ")}
                  onChange={(e) => set({ recordPath: e.target.value.split(",").map((x) => x.trim()).filter(Boolean) })}
                />
              </label>
            )}
            {(format === "excel" || format === "ods") && info && (
              <>
                <RailLabel>Sheets to import</RailLabel>
                {info.sheets?.map((sh, i) => {
                  const on = sheets.some((x) => x.fileNameAndSheetIndex === `file#${i}` && x.selected)
                  return (
                    <label key={sh} className="flex cursor-pointer items-center gap-2.5 px-4 py-1.5 hover:bg-accent">
                      <input
                        type="checkbox"
                        checked={on}
                        onChange={(e) =>
                          set({
                            sheets: e.target.checked ? [...sheets, { name: sh, fileNameAndSheetIndex: `file#${i}`, selected: true }] : sheets.filter((x) => x.fileNameAndSheetIndex !== `file#${i}`),
                          })
                        }
                        className="accent-foreground"
                      />
                      {sh}
                    </label>
                  )
                })}
              </>
            )}
            <div className="pt-2" />
            {(format === "csv" || format === "lines" || format === "fixed-width") && (
              <label className="flex items-center gap-2 px-4 py-1.5">
                <span className="min-w-0 flex-1">Character encoding</span>
                <select
                  value={String(options.encoding ?? "UTF-8")}
                  onChange={(e) => set({ encoding: e.target.value })}
                  className="h-7.5 w-24 rounded-[3px] border border-input bg-background px-1 text-right font-mono text-[12px]"
                >
                  {ENCODINGS.map((e) => (
                    <option key={e} value={e}>
                      {e}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {format === "lines" && num("linesPerRow", "Lines per row", "Each group of this many lines becomes one row.")}
            {info && !tree && (
              <>
                {num("headerLines", "Header lines", "0 names the columns Column 1, Column 2…")}
                {num("ignoreLines", "Skip first lines")}
                {num("skipDataLines", "Skip data lines")}
              </>
            )}
            {info && num("limit", "Load at most rows")}
            {info && (
              <>
                <div className="mt-2 border-t" />
                <div className="pt-2" />
                {format === "csv" && check("processQuotes", "Quotes enclose cells")}
                {(format === "csv" || tree) && check("trimStrings", "Trim whitespace")}
                {(format === "csv" || format === "fixed-width" || tree) && check("guessCellValueTypes", "Parse numbers and dates")}
                {!tree && check("storeBlankRows", "Store blank rows")}
                {!tree && check("storeBlankCellsAsNulls", "Blank cells as empty (null)")}
                {tree && check("storeEmptyStrings", "Keep empty text")}
              </>
            )}
          </aside>
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="flex h-10 shrink-0 items-center gap-3 border-b px-5">
              <span className="font-semibold">Preview</span>
              {preview && (
                <span className="font-mono text-xs text-muted-foreground">
                  first {preview.rows.length.toLocaleString()} rows{ms !== null && ` · parsed in ${ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms`}
                </span>
              )}
              <Spacer />
              {error && (
                <span role="alert" className="flex min-w-0 items-center gap-1.5 text-xs text-destructive">
                  <span aria-hidden="true" className="size-1.5 shrink-0 bg-destructive" />
                  <span className="truncate">{error}</span>
                </span>
              )}
            </div>
            <div className="min-h-0 flex-1 overflow-auto">
              {!preview ? (
                <div className="flex justify-center p-10">{error ? null : <Spinner />}</div>
              ) : (
                <table className="border-separate border-spacing-0">
                  <thead className="sticky top-0 z-10 bg-muted">
                    <tr>
                      <th scope="col" className="w-14 border-r border-b border-b-foreground px-3 py-2.5 text-right font-mono text-[11.5px] font-normal text-muted-foreground">
                        line
                      </th>
                      {preview.columns.map((c) => (
                        <th key={c} scope="col" className="max-w-60 truncate border-r border-b border-b-foreground px-3 py-2.5 text-left font-semibold whitespace-nowrap">
                          {c}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {preview.rows.map(([i, cells]) => (
                      <tr key={i} className="h-7.5">
                        <td className="border-r border-b border-row-rule px-3 text-right font-mono text-[11.5px] text-muted-foreground">{i + 1}</td>
                        {cells.map((v, c) => (
                          <td key={c} className="max-w-60 truncate border-r border-b border-row-rule px-3 whitespace-nowrap" title={v ?? undefined}>
                            {v}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        </div>
      </div>
    </FullView>
  )
}
