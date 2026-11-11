import { useState } from "react"
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
        />
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
                    {label}
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
                    {label}
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
                    {label}
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
                    Column “{c}”
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
                {c.label}
                {c.shortcut && <CommandShortcut>{c.shortcut}</CommandShortcut>}
              </CommandItem>
            ))}
          </CommandGroup>
        </CommandList>
      </Command>
    </CommandDialog>
  )
}
