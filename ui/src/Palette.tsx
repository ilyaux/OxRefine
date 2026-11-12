import { useState, type ReactNode } from "react"
import { useCommandState } from "cmdk"
import {
  Command,
  CommandDialog,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
  CommandSeparator,
  CommandShortcut,
} from "@/components/ui/command"
import { COMMON, PRESETS, type ColumnAction } from "@/Grid"

type Kind = Exclude<ColumnAction["kind"], "common" | "preset-facet">

/** The one-key tools, shown in the key column. */
const KEYS: Partial<Record<Kind, string>> = { facet: "F", transform: "T", cluster: "C", reconcile: "R" }

/** The label with the typed text in bold where it matches. */
function Mark({ text, q }: { text: string; q: string }) {
  const i = q ? text.toLowerCase().indexOf(q.toLowerCase()) : -1
  // one span, so the item's flex gap doesn't split the word around the match
  if (i < 0) return <span>{text}</span>
  return (
    <span>
      {text.slice(0, i)}
      <b className="font-semibold">{text.slice(i, i + q.length)}</b>
      {text.slice(i + q.length)}
    </span>
  )
}

/** How many commands match the typed text. */
function Count() {
  const n = useCommandState((state) => state.filtered.count)
  return <span className="shrink-0 font-mono text-[11.5px] text-muted-foreground">{n} results</span>
}

const Hint = ({ children }: { children: ReactNode }) => <span className="flex items-center border-r px-3 font-mono text-[11px] text-muted-foreground">{children}</span>

/** Every column-menu command, in the menu's order and wording. */
const COLUMN_COMMANDS: [Kind, string][] = [
  ["facet", "Text facet"],
  ["numeric-facet", "Numeric facet"],
  ["timeline-facet", "Timeline facet"],
  ["text-filter", "Text filter"],
  ["custom-list-facet", "Custom text facet…"],
  ["custom-range-facet", "Custom numeric facet…"],
  ["transform", "Transform…"],
  ["fill-down", "Fill down"],
  ["blank-down", "Blank down"],
  ["mv-split", "Split multi-valued cells…"],
  ["mv-join", "Join multi-valued cells…"],
  ["cluster", "Cluster and edit…"],
  ["split", "Split into several columns…"],
  ["add-column", "Add column based on this…"],
  ["fetch", "Add column by fetching URLs…"],
  ["move-first", "Move column to the beginning"],
  ["move-left", "Move column left"],
  ["move-right", "Move column right"],
  ["move-last", "Move column to the end"],
  ["rename", "Rename column…"],
  ["transpose-columns", "Transpose cells across columns into rows…"],
  ["transpose-rows", "Transpose cells in rows into columns…"],
  ["columnize", "Columnize by key/value columns…"],
  ["sort", "Sort rows by this column…"],
  ["dedupe", "Remove duplicate rows by this column"],
  ["remove", "Remove column"],
]

export type ProjectCommand = {
  label: string
  shortcut?: string
  run: () => void
}

/** Ctrl+K: any column command on the column last worked on (or another one picked here), plus the
 * project's own commands. */
export function Palette({
  open,
  onOpenChange,
  column,
  columns,
  onColumn,
  onAction,
  commands,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  column: string | null
  columns: string[]
  onColumn: (c: string) => void
  onAction: (a: ColumnAction) => void
  commands: ProjectCommand[]
}) {
  const [search, setSearch] = useState("")
  const run = (f: () => void) => {
    onOpenChange(false)
    setSearch("")
    f()
  }
  return (
    <CommandDialog
      open={open}
      onOpenChange={onOpenChange}
      title="Command palette"
      description="Run a command on the column or the project"
    >
      <Command>
        <CommandInput
          placeholder={
            column
              ? `Run a command on “${column}”, or type a column name…`
              : "Run a command…"
          }
          value={search}
          onValueChange={setSearch}
        >
          <Count />
        </CommandInput>
        <CommandList>
          <CommandEmpty>No command matches.</CommandEmpty>
          {column && (
            <>
              <CommandGroup heading={`On column “${column}”`}>
                {COLUMN_COMMANDS.map(([kind, label]) => (
                  <CommandItem
                    key={kind}
                    value={`${label} ${column}`}
                    onSelect={() => run(() => onAction({ kind, column }))}
                  >
                    <Mark text={label} q={search} />
                    {KEYS[kind] && <CommandShortcut>{KEYS[kind]}</CommandShortcut>}
                  </CommandItem>
                ))}
                {COMMON.map(([label, expression]) => (
                  <CommandItem
                    key={label}
                    value={`${label} ${column}`}
                    onSelect={() =>
                      run(() =>
                        onAction({ kind: "common", column, expression })
                      )
                    }
                  >
                    <Mark text={label} q={search} />
                  </CommandItem>
                ))}
                {PRESETS.map(([label, facet, expression]) => (
                  <CommandItem
                    key={label}
                    value={`${label} ${column}`}
                    onSelect={() =>
                      run(() =>
                        onAction({
                          kind: "preset-facet",
                          column,
                          facet,
                          expression: expression.replace(
                            "$COL",
                            JSON.stringify(column)
                          ),
                        })
                      )
                    }
                  >
                    <Mark text={label} q={search} />
                  </CommandItem>
                ))}
              </CommandGroup>
              <CommandSeparator />
            </>
          )}
          {columns.length > 0 && (
            <CommandGroup heading="Work on another column">
              {columns
                .filter((c) => c !== column)
                .map((c) => (
                  <CommandItem
                    key={c}
                    value={`column ${c}`}
                    onSelect={() => (onColumn(c), setSearch(""))}
                  >
                    <Mark text={`Column “${c}”`} q={search} />
                  </CommandItem>
                ))}
            </CommandGroup>
          )}
          <CommandSeparator />
          <CommandGroup heading="Project">
            {commands.map((c) => (
              <CommandItem
                key={c.label}
                value={c.label}
                onSelect={() => run(c.run)}
              >
                <Mark text={c.label} q={search} />
                {c.shortcut && <CommandShortcut>{c.shortcut}</CommandShortcut>}
              </CommandItem>
            ))}
          </CommandGroup>
        </CommandList>
        <div className="flex h-8 shrink-0 items-stretch border-t">
          <Hint>↑↓ move</Hint>
          <Hint>↵ run</Hint>
          <span className="flex-1" />
          <span className="flex items-center border-l px-3 font-mono text-[11px] text-muted-foreground">Esc close</span>
        </div>
      </Command>
    </CommandDialog>
  )
}
