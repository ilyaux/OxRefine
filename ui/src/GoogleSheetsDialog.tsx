import { useEffect, useState } from "react"
import { openUrl } from "@tauri-apps/plugin-opener"
import { call, isTauri } from "@/api"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Spinner } from "@/components/ui/spinner"
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table"

type Status = { configured: boolean; signedIn: boolean; email: string | null; pending: boolean; error: string | null }
type Sheet = { id: string; name: string; modifiedTime?: string; owners?: { displayName: string }[] }

const open = (url: string) => (isTauri ? openUrl(url).catch(() => window.open(url, "_blank")) : window.open(url, "_blank", "noopener"))

/** Import a Google Sheet (OpenRefine's gdata extension): by link without signing in, or from Drive. */
export function GoogleSheetsDialog({ onImport, onError, onClose }: { onImport: (link: string) => Promise<void>; onError: (e: unknown) => void; onClose: () => void }) {
  const [status, setStatus] = useState<Status | null>(null)
  const [link, setLink] = useState("")
  const [query, setQuery] = useState("")
  const [sheets, setSheets] = useState<Sheet[] | null>(null)
  const [signInUrl, setSignInUrl] = useState("")
  const [busy, setBusy] = useState(false)

  const refresh = () => call<Status>("google_status", {}, { quiet: true }).then(({ data }) => (setStatus(data), data))
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => void refresh().catch(onError), [])
  // while signing in, ask again every second until Google's answer arrives
  useEffect(() => {
    if (!status?.pending) return
    const t = setInterval(() => void refresh().catch(() => {}), 1000)
    return () => clearInterval(t)
  }, [status?.pending])
  useEffect(() => {
    if (!status?.signedIn) return setSheets(null)
    let live = true
    const t = setTimeout(() => call<Sheet[]>("google_sheets", { text: query }).then(({ data }) => live && setSheets(data), (e) => live && onError(e)), 250)
    return () => {
      live = false
      clearTimeout(t)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [status?.signedIn, query])

  const run = (f: () => Promise<unknown>) => {
    setBusy(true)
    f().catch(onError).finally(() => setBusy(false))
  }
  const signIn = () =>
    run(async () => {
      const { data } = await call<{ url: string }>("google_sign_in")
      setSignInUrl(data.url)
      open(data.url)
      await refresh()
    })
  const valid = /^https:\/\/docs\.google\.com\/spreadsheets\/d\/[\w-]+/.test(link.trim())

  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-x-hidden overflow-y-auto sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>Google Sheets</DialogTitle>
          <DialogDescription>Every sheet of the spreadsheet comes in through the Excel import preview; pick sheets and options there.</DialogDescription>
        </DialogHeader>
        <FieldGroup>
          <Field>
            <FieldLabel htmlFor="gsheet-link">Sheet link</FieldLabel>
            <div className="flex gap-2">
              <Input id="gsheet-link" placeholder="https://docs.google.com/spreadsheets/d/…" value={link} onChange={(e) => setLink(e.target.value)} onKeyDown={(e) => e.key === "Enter" && valid && run(() => onImport(link.trim()))} />
              <Button disabled={!valid || busy} onClick={() => run(() => onImport(link.trim()))}>
                Import
              </Button>
            </div>
            <FieldDescription>Works without signing in for sheets shared with anyone who has the link.</FieldDescription>
          </Field>
          <Field>
            <FieldLabel>Your Google Drive</FieldLabel>
            {!status ? (
              <Spinner />
            ) : status.signedIn ? (
              <div className="flex items-center justify-between gap-2">
                <span className="truncate text-sm">Signed in{status.email ? ` as ${status.email}` : ""}</span>
                <Button variant="outline" size="sm" disabled={busy} onClick={() => run(() => call<Status>("google_sign_out").then(({ data }) => setStatus(data)))}>
                  Sign out
                </Button>
              </div>
            ) : status.configured ? (
              <div className="flex flex-col gap-2">
                <div>
                  <Button variant="outline" disabled={busy || status.pending} onClick={signIn}>
                    {status.pending && <Spinner data-icon="inline-start" />}
                    {status.pending ? "Waiting for Google…" : "Sign in to Google"}
                  </Button>
                </div>
                {status.pending && signInUrl && (
                  <FieldDescription>
                    Finish signing in in your browser.{" "}
                    <a href={signInUrl} target="_blank" rel="noreferrer" className="underline underline-offset-4">
                      Open the sign-in page again
                    </a>
                  </FieldDescription>
                )}
                {status.error && <FieldDescription className="text-destructive">{status.error}</FieldDescription>}
              </div>
            ) : (
              <FieldDescription>
                To list your sheets, import private ones and export to Google, set <code>google.clientId</code> and <code>google.clientSecret</code> in Preferences → All preferences.
                They come from a "Desktop app" OAuth client in Google Cloud Console; Google issues them to whoever registers the app, so OxRefine can't ship one.
              </FieldDescription>
            )}
          </Field>
          {status?.signedIn && (
            <Field>
              <Input aria-label="Search your sheets" placeholder="Search your sheets" value={query} onChange={(e) => setQuery(e.target.value)} />
              {sheets === null ? (
                <Spinner />
              ) : sheets.length === 0 ? (
                <Empty>
                  <EmptyHeader>
                    <EmptyTitle>No sheets found</EmptyTitle>
                    <EmptyDescription>{query ? "No sheet name contains that text." : "This Drive has no Google Sheets."}</EmptyDescription>
                  </EmptyHeader>
                </Empty>
              ) : (
                <Table className="table-fixed">
                  <TableBody>
                    {sheets.map((s) => (
                      <TableRow key={s.id}>
                        <TableCell className="truncate">{s.name}</TableCell>
                        <TableCell className="w-32 text-xs text-muted-foreground">{s.modifiedTime?.slice(0, 10)}</TableCell>
                        <TableCell className="w-24 text-right">
                          <Button size="sm" variant="outline" disabled={busy} onClick={() => run(() => onImport(`https://docs.google.com/spreadsheets/d/${s.id}`))}>
                            Import
                          </Button>
                        </TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              )}
            </Field>
          )}
        </FieldGroup>
      </DialogContent>
    </Dialog>
  )
}
