import { useEffect, useState } from "react"
import { PlusIcon, Trash2Icon } from "lucide-react"
import { call } from "@/api"
import { useTheme } from "@/components/theme-provider"
import { Button } from "@/components/ui/button"
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog"
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty"
import { Field, FieldDescription, FieldGroup, FieldLabel, FieldSet, FieldLegend } from "@/components/ui/field"
import { Input } from "@/components/ui/input"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table"
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs"
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group"
import { forgetExpressions, recentExpressions } from "@/ExpressionEditor"
import { loadServices, saveServices } from "@/ReconcileDialog"

type Prefs = Record<string, unknown>
const FACET_LIMIT = "ui.browsing.listFacet.limit"

/** Preferences, as on OpenRefine's preferences page: the common ones by name, then every one as key and
 * JSON value. Engine preferences live in the data folder (shared by every project); the theme, the
 * reconciliation services and recent expressions belong to this browser profile. */
export function Preferences({ onClose, onError, onChanged }: { onClose: () => void; onError: (e: unknown) => void; onChanged: () => void }) {
  const [prefs, setPrefs] = useState<Prefs | null>(null)
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => void call<Prefs>("preferences").then(({ data }) => setPrefs(data), onError), [])
  const set = async (name: string, value: unknown) => {
    const { data } = await call<Prefs>("set_preference", { name, value }).catch((e) => (onError(e), { data: prefs }))
    setPrefs(data)
    onChanged()
  }
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()}>
      <DialogContent className="max-h-[90svh] overflow-y-auto sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Preferences</DialogTitle>
          <DialogDescription>Changes apply right away.</DialogDescription>
        </DialogHeader>
        <Tabs defaultValue="general">
          <TabsList>
            <TabsTrigger value="general">General</TabsTrigger>
            <TabsTrigger value="recon">Reconciliation</TabsTrigger>
            <TabsTrigger value="expressions">Expressions</TabsTrigger>
            <TabsTrigger value="all">All preferences</TabsTrigger>
          </TabsList>
          <TabsContent value="general">
            {/* keyed by the saved limit, so the box starts from it again once preferences load or change */}
            <General key={String(prefs?.[FACET_LIMIT] ?? "")} prefs={prefs} set={set} />
          </TabsContent>
          <TabsContent value="recon">
            <Services />
          </TabsContent>
          <TabsContent value="expressions">
            <Expressions />
          </TabsContent>
          <TabsContent value="all">
            <AllPrefs prefs={prefs} set={set} />
          </TabsContent>
        </Tabs>
      </DialogContent>
    </Dialog>
  )
}

function General({ prefs, set }: { prefs: Prefs | null; set: (name: string, value: unknown) => Promise<void> }) {
  const { theme, setTheme } = useTheme()
  const current = prefs?.[FACET_LIMIT]
  const [limit, setLimit] = useState(String(current ?? 2000))
  const n = Number(limit)
  const valid = Number.isInteger(n) && n > 0
  return (
    <FieldGroup>
      <Field>
        <FieldLabel>Theme</FieldLabel>
        <ToggleGroup variant="outline" value={[theme]} onValueChange={(v) => v[0] && setTheme(v[0] as typeof theme)} aria-label="Theme">
          <ToggleGroupItem value="light">Light</ToggleGroupItem>
          <ToggleGroupItem value="dark">Dark</ToggleGroupItem>
          <ToggleGroupItem value="system">System</ToggleGroupItem>
        </ToggleGroup>
      </Field>
      <Field data-invalid={!valid || undefined}>
        <FieldLabel htmlFor="pref-facet-limit">Facet choice limit</FieldLabel>
        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault()
            if (valid) void set(FACET_LIMIT, n === 2000 ? null : n)
          }}
        >
          <Input id="pref-facet-limit" type="number" min={1} value={limit} onChange={(e) => setLimit(e.target.value)} aria-invalid={!valid || undefined} className="max-w-40" />
          <Button type="submit" variant="outline" disabled={!valid || n === Number(current ?? 2000)}>
            Save
          </Button>
        </form>
        <FieldDescription>
          A list facet with more choices than this lists none, as in OpenRefine (<code>{FACET_LIMIT}</code>, 2000 by default).
        </FieldDescription>
      </Field>
    </FieldGroup>
  )
}

function Services() {
  const [list, setList] = useState(loadServices)
  const remove = (url: string) => {
    const next = list.filter((s) => s.url !== url)
    saveServices(next)
    setList(next)
  }
  return (
    <FieldSet>
      <FieldLegend variant="label">Reconciliation services</FieldLegend>
      <FieldDescription>Add a service from Reconcile → Start reconciling… on a column. Wikidata is always there.</FieldDescription>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Service</TableHead>
            <TableHead>Address</TableHead>
            <TableHead className="text-right">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {list.map((s, i) => (
            <TableRow key={s.url}>
              <TableCell className="font-medium">{s.name}</TableCell>
              <TableCell className="max-w-72 truncate text-muted-foreground">{s.url}</TableCell>
              <TableCell className="text-right">
                {i > 0 && (
                  <Button variant="ghost" size="icon-sm" aria-label={`Remove ${s.name}`} onClick={() => remove(s.url)}>
                    <Trash2Icon />
                  </Button>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </FieldSet>
  )
}

function Expressions() {
  const [recent, setRecent] = useState(recentExpressions)
  return (
    <FieldSet>
      <FieldLegend variant="label">Recent expressions</FieldLegend>
      <FieldDescription>The last 20 expressions you ran, offered again in the expression editor.</FieldDescription>
      {recent.length ? (
        <>
          <Table>
            <TableBody>
              {recent.map((e) => (
                <TableRow key={e}>
                  <TableCell className="max-w-xl truncate font-mono text-xs">{e}</TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
          <div>
            <Button variant="outline" onClick={() => (forgetExpressions(), setRecent([]))}>
              Clear the list
            </Button>
          </div>
        </>
      ) : (
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No recent expressions</EmptyTitle>
            <EmptyDescription>Expressions you run in Transform and Add column appear here.</EmptyDescription>
          </EmptyHeader>
        </Empty>
      )}
    </FieldSet>
  )
}

/** Every engine preference as name and JSON value, like OpenRefine's preferences page. */
function AllPrefs({ prefs, set }: { prefs: Prefs | null; set: (name: string, value: unknown) => Promise<void> }) {
  const [name, setName] = useState("")
  const [value, setValue] = useState("")
  const parsed = (() => {
    try {
      return { ok: true, v: JSON.parse(value) as unknown }
    } catch {
      return { ok: false, v: null }
    }
  })()
  const entries = Object.entries(prefs ?? {})
  return (
    <FieldGroup>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Value</TableHead>
            <TableHead className="text-right">
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {entries.length === 0 && (
            <TableRow>
              <TableCell colSpan={3} className="text-muted-foreground">
                Everything is at its default.
              </TableCell>
            </TableRow>
          )}
          {entries.map(([k, v]) => (
            <TableRow key={k}>
              <TableCell className="font-mono text-xs">{k}</TableCell>
              <TableCell className="max-w-72 truncate font-mono text-xs">{JSON.stringify(v)}</TableCell>
              <TableCell className="text-right">
                <Button variant="ghost" size="sm" onClick={() => (setName(k), setValue(JSON.stringify(v)))}>
                  Edit
                </Button>
                <Button variant="ghost" size="icon-sm" aria-label={`Remove ${k}`} onClick={() => void set(k, null)}>
                  <Trash2Icon />
                </Button>
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault()
          if (name.trim() && parsed.ok) void set(name.trim(), parsed.v).then(() => (setName(""), setValue("")))
        }}
      >
        <Field orientation="horizontal">
          <Input aria-label="Preference name" placeholder="Name" value={name} onChange={(e) => setName(e.target.value)} className="font-mono" />
          <Input
            aria-label="Value as JSON"
            placeholder="Value as JSON: 10, true, &quot;text&quot;"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            aria-invalid={(!!value && !parsed.ok) || undefined}
            className="font-mono"
          />
          <Button type="submit" variant="outline" disabled={!name.trim() || !parsed.ok}>
            <PlusIcon data-icon="inline-start" />
            Set
          </Button>
        </Field>
      </form>
    </FieldGroup>
  )
}
