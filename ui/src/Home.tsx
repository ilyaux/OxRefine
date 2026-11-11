import { useEffect, useState } from "react"
import { ClipboardPasteIcon, FolderOpenIcon, GlobeIcon, TableIcon } from "lucide-react"
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
import { Card, CardAction, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card"
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"

export type ProjectMeta = { id: string; name: string; created: string; modified: string; rows: number; columns: number; source?: { path: string } }

const when = (iso: string) => {
  const d = new Date(iso)
  return isNaN(d.getTime()) ? iso : d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" })
}

export function Home({
  onImport,
  onPaste,
  onUrl,
  onOpen,
  onRename,
  onError,
}: {
  onImport: () => void
  onPaste: () => void
  onUrl: () => void
  onOpen: (id: string) => void
  onRename: (p: ProjectMeta, done: () => void) => void
  onError: (e: unknown) => void
}) {
  const [projects, setProjects] = useState<ProjectMeta[] | null>(null)
  const [deleting, setDeleting] = useState<ProjectMeta | null>(null)
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
            Import CSV, TSV, Excel (.xlsx, .xls), OpenDocument (.ods), JSON or plain text from a file, the clipboard or a web address.
            Options and a preview come next; the original is never changed.
          </CardDescription>
          <CardAction className="flex gap-2">
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
                {projects.map((p) => (
                  <TableRow key={p.id}>
                    <TableCell className="max-w-72">
                      <Button variant="link" className="h-auto max-w-full justify-start p-0" onClick={() => onOpen(p.id)}>
                        <span className="truncate">{p.name}</span>
                      </Button>
                    </TableCell>
                    <TableCell className="text-right tabular-nums">{p.rows.toLocaleString()}</TableCell>
                    <TableCell className="text-right tabular-nums">{p.columns}</TableCell>
                    <TableCell className="text-muted-foreground">{when(p.modified)}</TableCell>
                    <TableCell className="text-right">
                      <div className="flex justify-end gap-1">
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
