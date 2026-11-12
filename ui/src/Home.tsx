import { useEffect, useState } from "react"
import { call } from "@/api"
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog"
import { Button } from "@/components/ui/button"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table"
import { cn } from "@/lib/utils"

/** OpenRefine's project metadata, with its field names. */
export type ProjectInfo = {
  tags: string[]
  title: string
  description: string
  creator: string
  contributors: string
  subject: string
  version: string
  license: string
  homepage: string
  image: string
  customMetadata: Record<string, unknown>
}
export type ProjectMeta = { id: string; name: string; created: string; modified: string; rows: number; columns: number; source?: { path: string }; info?: Partial<ProjectInfo> }

type Text = Exclude<keyof ProjectInfo, "tags" | "customMetadata" | "description">
const FIELDS: [Text, string][] = [
  ["title", "Title"],
  ["creator", "Creator"],
  ["contributors", "Contributors"],
  ["subject", "Subject"],
  ["version", "Version"],
  ["license", "License"],
  ["homepage", "Homepage"],
  ["image", "Image"],
]

const when = (iso: string) => {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
}

/** "just now", "09:42", "yesterday", "Nov 9", "Oct 28, 2024". */
const changed = (iso: string) => {
  const d = new Date(iso)
  if (isNaN(d.getTime())) return iso
  const now = new Date()
  const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime()
  const days = Math.round((day(now) - day(d)) / 864e5)
  if (now.getTime() - d.getTime() < 6e4) return "just now"
  if (days === 0) return d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
  if (days === 1) return "yesterday"
  return d.toLocaleDateString(undefined, d.getFullYear() === now.getFullYear() ? { month: "short", day: "numeric" } : { dateStyle: "medium" })
}

const fileName = (path?: string) => path?.split(/[\\/]/).pop()

/** One way to start a project: a full-width row with a rule above it. */
function Source({ title, hint, keys, onClick }: { title: string; hint: string; keys?: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="grid w-full grid-cols-[1fr_auto] items-baseline gap-x-3 gap-y-0.5 border-t px-5 py-3 text-left outline-none hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
    >
      <span className="font-medium">{title}</span>
      {keys ? <kbd>{keys}</kbd> : <span />}
      <span className="col-span-2 text-xs text-muted-foreground">{hint}</span>
    </button>
  )
}

export function Home({
  onImport,
  onPaste,
  onUrl,
  onOpenRefine,
  onOpen,
  onRename,
  onError,
}: {
  onImport: () => void
  onPaste: () => void
  onUrl: () => void
  onOpenRefine: () => void
  onOpen: (id: string) => void
  onRename: (p: ProjectMeta, done: () => void) => void
  onError: (e: unknown) => void
}) {
  const [projects, setProjects] = useState<ProjectMeta[] | null>(null)
  const [deleting, setDeleting] = useState<ProjectMeta | null>(null)
  const [about, setAbout] = useState<ProjectMeta | null>(null)
  const [tag, setTag] = useState<string | null>(null)
  const [filter, setFilter] = useState("")
  const tags = [...new Set((projects ?? []).flatMap((p) => p.info?.tags ?? []))].sort((a, b) => a.localeCompare(b))
  const q = filter.trim().toLowerCase()
  const shown = (projects ?? []).filter(
    (p) => (!tag || p.info?.tags?.includes(tag)) && (!q || [p.name, p.source?.path, p.info?.description, ...(p.info?.tags ?? [])].some((x) => x?.toLowerCase().includes(q))),
  )
  const load = () => call<ProjectMeta[]>("projects").then(({ data }) => setProjects(data), onError)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => void load(), [])

  const remove = async () => {
    if (!deleting) return
    await call("delete_project", { id: deleting.id }).catch(onError)
    setDeleting(null)
    load()
  }

  const th = "border-b px-3 py-2.5 text-left text-[11.5px] font-normal text-muted-foreground"
  const num = "border-b px-3 text-right font-mono text-[12.5px]"
  return (
    <div className="flex min-h-0 flex-1 flex-wrap overflow-auto">
      <aside aria-labelledby="new-h" className="flex max-w-[340px] flex-[1_1_300px] flex-col border-r">
        <h2 id="new-h" className="px-5 pt-5 pb-3 font-semibold">
          New project
        </h2>
        <Source title="From a file or folder" hint="Preview with OpenRefine's import options" keys="Ctrl O" onClick={onImport} />
        <Source title="Paste from clipboard" hint="Tables copied from a spreadsheet or a page" onClick={onPaste} />
        <Source title="From a web address" hint="CSV, JSON or a file behind a link" onClick={onUrl} />
        <Source title="Open an OpenRefine project" hint=".tar.gz export, undo history included" onClick={onOpenRefine} />
        <div className="border-t" />
        <p className="px-5 py-4 font-mono text-[11px] leading-relaxed text-muted-foreground">
          csv tsv xlsx xls ods json xml txt
          <br />
          zip tar gz bz2 · openrefine.tar.gz
        </p>
      </aside>

      <main className="flex min-w-0 flex-[999_1_560px] flex-col">
        <div className="flex flex-wrap items-center gap-3 border-b border-foreground px-5 py-3.5">
          <h1 className="font-semibold">
            Your projects{" "}
            <span className="font-mono font-normal text-muted-foreground">{projects ? shown.length : ""}</span>
          </h1>
          {tags.length > 0 && (
            <div role="group" aria-label="Show projects tagged" className="flex flex-wrap items-center gap-1">
              {tags.map((t) => (
                <button
                  key={t}
                  type="button"
                  aria-pressed={tag === t}
                  onClick={() => setTag(tag === t ? null : t)}
                  className={cn(
                    "rounded-[3px] border px-2 font-mono text-[11.5px] leading-6 outline-none focus-visible:ring-2 focus-visible:ring-ring",
                    tag === t ? "border-foreground bg-foreground text-background" : "border-input text-ink-2 hover:bg-accent",
                  )}
                >
                  {t}
                </button>
              ))}
            </div>
          )}
          <span className="flex-1" />
          <label className="block flex-[0_1_260px]">
            <span className="sr-only">Filter projects</span>
            <Input type="search" placeholder="Filter…" value={filter} onChange={(e) => setFilter(e.target.value)} className="h-7.5" />
          </label>
        </div>
        {!projects ? (
          <div className="flex justify-center py-8">
            <Spinner />
          </div>
        ) : projects.length === 0 ? (
          <div className="flex flex-col gap-1 px-5 py-8">
            <span className="font-medium">No projects yet</span>
            <span className="text-muted-foreground">Start one from the list on the left; it is saved after every step, with the full undo history.</span>
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[760px] border-collapse">
              <thead>
                <tr>
                  <th scope="col" className={cn(th, "pl-5")}>
                    Name
                  </th>
                  <th scope="col" className={cn(th, "text-right")}>
                    Rows
                  </th>
                  <th scope="col" className={cn(th, "text-right")}>
                    Cols
                  </th>
                  <th scope="col" className={cn(th, "text-right")}>
                    Changed
                  </th>
                  <th scope="col" className={cn(th, "pr-5")}>
                    <span className="sr-only">Actions</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {shown.map((p) => (
                  <tr key={p.id} className="group/row hover:bg-muted">
                    <td className="h-13 max-w-0 border-b py-2 pr-3 pl-5">
                      <button type="button" onClick={() => onOpen(p.id)} className="flex max-w-full flex-col items-start gap-0.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring">
                        <span className="max-w-full truncate font-medium group-hover/row:underline">{p.name}</span>
                        {fileName(p.source?.path) && <span className="max-w-full truncate font-mono text-[11px] text-muted-foreground">{fileName(p.source?.path)}</span>}
                      </button>
                      {(p.info?.description || !!p.info?.tags?.length) && (
                        <div className="mt-1 flex max-w-full flex-wrap items-center gap-1.5 text-xs text-muted-foreground">
                          {p.info?.tags?.map((t, i) => (
                            <span key={i} className="rounded-[3px] border border-input px-1 font-mono text-[11px] text-ink-2">
                              {t}
                            </span>
                          ))}
                          {p.info?.description && <span className="truncate">{p.info.description}</span>}
                        </div>
                      )}
                    </td>
                    <td className={num}>{p.rows.toLocaleString()}</td>
                    <td className={cn(num, "text-muted-foreground")}>{p.columns}</td>
                    <td className={cn(num, "text-xs text-muted-foreground")} title={when(p.modified)}>
                      {changed(p.modified)}
                    </td>
                    <td className="border-b pr-3 pl-3 text-right whitespace-nowrap">
                      <span className="inline-flex opacity-60 group-focus-within/row:opacity-100 group-hover/row:opacity-100">
                        <Button variant="ghost" size="sm" onClick={() => setAbout(p)}>
                          About
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => onRename(p, load)}>
                          Rename
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleting(p)}>
                          Delete
                        </Button>
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            {shown.length === 0 && <p className="px-5 py-6 text-muted-foreground">No project matches.</p>}
          </div>
        )}
      </main>

      {about && (
        <AboutDialog
          project={about}
          onClose={() => setAbout(null)}
          onSave={async (info) => {
            await call("set_project_info", { id: about.id, info }).catch(onError)
            setAbout(null)
            load()
          }}
        />
      )}

      <AlertDialog open={!!deleting} onOpenChange={(o) => !o && setDeleting(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete “{deleting?.name}”?</AlertDialogTitle>
            <AlertDialogDescription>
              The project and its history are removed from OxRefine. The file it was imported from stays where it is.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction variant="destructive" onClick={remove}>
              Delete project
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  )
}

/** A project's metadata, as OpenRefine's "About" dialog shows it: tags, description and the rest. */
function AboutDialog({ project, onSave, onClose }: { project: ProjectMeta; onSave: (info: ProjectInfo) => Promise<void>; onClose: () => void }) {
  const i = project.info ?? {}
  const [tags, setTags] = useState((i.tags ?? []).join(", "))
  const [description, setDescription] = useState(i.description ?? "")
  const [fields, setFields] = useState(() => Object.fromEntries(FIELDS.map(([k]) => [k, i[k] ?? ""])) as Record<Text, string>)
  const custom = Object.entries(i.customMetadata ?? {})
  const submit = async (e: React.FormEvent) => {
    e.preventDefault()
    // the engine splits tags at commas and spaces, as OpenRefine does
    await onSave({ ...fields, tags: [tags], description, customMetadata: i.customMetadata ?? {} })
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-x-hidden overflow-y-auto sm:max-w-lg">
        <form onSubmit={submit} className="flex min-w-0 flex-col gap-4">
          <DialogHeader>
            <DialogTitle>About “{project.name}”</DialogTitle>
            <DialogDescription>
              Created {when(project.created)}, last changed {when(project.modified)}
              {project.source?.path && (
                <>
                  , imported from <span className="font-mono text-[11.5px] break-all">{project.source.path}</span>
                </>
              )}
              . OpenRefine project archives carry these fields both ways.
            </DialogDescription>
          </DialogHeader>
          <FieldGroup>
            <Field>
              <FieldLabel htmlFor="about-tags">Tags</FieldLabel>
              <Input id="about-tags" value={tags} onChange={(e) => setTags(e.target.value)} autoFocus />
              <FieldDescription>Separated by commas or spaces, as in OpenRefine.</FieldDescription>
            </Field>
            <Field>
              <FieldLabel htmlFor="about-description">Description</FieldLabel>
              <Textarea id="about-description" value={description} onChange={(e) => setDescription(e.target.value)} />
            </Field>
            {FIELDS.map(([k, label]) => (
              <Field key={k}>
                <FieldLabel htmlFor={`about-${k}`}>{label}</FieldLabel>
                <Input id={`about-${k}`} value={fields[k]} onChange={(e) => setFields({ ...fields, [k]: e.target.value })} />
              </Field>
            ))}
            {custom.length > 0 && (
              <FieldSet>
                <FieldLegend variant="label">Custom metadata</FieldLegend>
                <FieldDescription>From the OpenRefine project; kept as it is.</FieldDescription>
                <Table>
                  <TableBody>
                    {custom.map(([k, v]) => (
                      <TableRow key={k}>
                        <TableCell className="font-medium">{k}</TableCell>
                        <TableCell className="text-muted-foreground">{typeof v === "string" ? v : JSON.stringify(v)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </FieldSet>
            )}
          </FieldGroup>
          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit">Save</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
