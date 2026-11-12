import { useEffect, useState } from "react"
import { ClipboardPasteIcon, FolderOpenIcon, GlobeIcon, HistoryIcon, TableIcon, TagIcon } from "lucide-react"
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
import { Badge } from "@/components/ui/badge"
import { Button } from "@/components/ui/button"
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldLegend, FieldSet } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Textarea } from "@/components/ui/textarea"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

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
  const tags = [...new Set((projects ?? []).flatMap((p) => p.info?.tags ?? []))].sort((a, b) => a.localeCompare(b))
  const shown = (projects ?? []).filter((p) => !tag || p.info?.tags?.includes(tag))
  const load = () => call<ProjectMeta[]>("projects").then(({ data }) => setProjects(data), onError)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => void load(), [])

  const remove = async () => {
    if (!deleting) return
    await call("delete_project", { id: deleting.id }).catch(onError)
    setDeleting(null)
    load()
  }

  return (
    <div className="mx-auto flex w-full max-w-5xl min-h-0 flex-1 flex-col gap-6 overflow-auto p-6">
      <Card>
        <CardHeader>
          <CardTitle>New project</CardTitle>
          <CardDescription>
            Import CSV, TSV, fixed-width or plain text, Excel (.xlsx, .xls), OpenDocument (.ods), JSON or XML from a file, a zip, tar or gzip archive, the clipboard or a web address.
            Options and a preview come next; the original is never changed. OpenRefine projects (.openrefine.tar.gz) open with
            their undo history.
          </CardDescription>
          <CardAction className="flex gap-2">
            <Button variant="outline" onClick={onOpenRefine}>
              <HistoryIcon data-icon="inline-start" />
              OpenRefine project
            </Button>
            <Button variant="outline" onClick={onPaste}>
              <ClipboardPasteIcon data-icon="inline-start" />
              Paste data
            </Button>
            <Button variant="outline" onClick={onUrl}>
              <GlobeIcon data-icon="inline-start" />
              Web address
            </Button>
            <Button onClick={onImport}>
              <FolderOpenIcon data-icon="inline-start" />
              Choose a file
            </Button>
          </CardAction>
        </CardHeader>
      </Card>

      <Card className="min-h-0">
        <CardHeader>
          <CardTitle>Your projects</CardTitle>
          <CardDescription>Saved automatically after every step, with the full undo history.</CardDescription>
          {tags.length > 0 && (
            <CardAction>
              <ToggleGroup variant="outline" size="sm" value={tag ? [tag] : []} onValueChange={(v: string[]) => setTag(v[0] ?? null)} aria-label="Show projects tagged">
                {tags.map((t) => (
                  <ToggleGroupItem key={t} value={t}>
                    <TagIcon data-icon="inline-start" />
                    {t}
                  </ToggleGroupItem>
                ))}
              </ToggleGroup>
            </CardAction>
          )}
        </CardHeader>
        <CardContent>
          {!projects ? (
            <div className="flex justify-center py-8">
              <Spinner />
            </div>
          ) : projects.length === 0 ? (
            <Empty>
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <TableIcon />
                </EmptyMedia>
                <EmptyTitle>No projects yet</EmptyTitle>
                <EmptyDescription>Choose a file above to create your first one.</EmptyDescription>
              </EmptyHeader>
            </Empty>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Name</TableHead>
                  <TableHead className="text-right">Rows</TableHead>
                  <TableHead className="text-right">Columns</TableHead>
                  <TableHead>Last changed</TableHead>
                  <TableHead className="text-right">
                    <span className="sr-only">Actions</span>
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {shown.map((p) => (
                  <TableRow key={p.id}>
                    <TableCell className="max-w-80">
                      <div className="flex flex-col items-start gap-1">
                        <Button variant="link" className="h-auto max-w-full justify-start p-0" onClick={() => onOpen(p.id)}>
                          <span className="truncate">{p.name}</span>
                        </Button>
                        {p.info?.description && <span className="max-w-full truncate text-xs text-muted-foreground">{p.info.description}</span>}
                        {!!p.info?.tags?.length && (
                          <div className="flex flex-wrap gap-1">
                            {p.info.tags.map((t, i) => (
                              <Badge key={i} variant="secondary">
                                {t}
                              </Badge>
                            ))}
                          </div>
                        )}
                      </div>
                    </TableCell>
                    <TableCell className="text-right tabular-nums">{p.rows.toLocaleString()}</TableCell>
                    <TableCell className="text-right tabular-nums">{p.columns}</TableCell>
                    <TableCell className="text-muted-foreground">{when(p.modified)}</TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
                        <Button variant="ghost" size="sm" onClick={() => setAbout(p)}>
                          About
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => onRename(p, load)}>
                          Rename
                        </Button>
                        <Button variant="ghost" size="sm" onClick={() => setDeleting(p)}>
                          Delete
                        </Button>
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

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
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-lg">
        <form onSubmit={submit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>About “{project.name}”</DialogTitle>
            <DialogDescription>
              Created {when(project.created)}, last changed {when(project.modified)}
              {project.source?.path ? `, imported from ${project.source.path}` : ""}. OpenRefine project archives carry these fields both ways.
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
