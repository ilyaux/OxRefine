import { useEffect, useRef } from "react"
import { EditorState } from "@codemirror/state"
import { EditorView, keymap, placeholder as cmPlaceholder } from "@codemirror/view"
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands"
import {
  autocompletion,
  closeBrackets,
  closeBracketsKeymap,
  completionKeymap,
  snippetCompletion,
  type Completion,
  type CompletionContext,
} from "@codemirror/autocomplete"
import { bracketMatching, HighlightStyle, StreamLanguage, syntaxHighlighting } from "@codemirror/language"
import { tags } from "@lezer/highlight"
import docs from "@/grel-docs.json"

const VARIABLES: [string, string][] = [
  ["value", "The cell's value"],
  ["cell", "The cell: cell.value, cell.recon"],
  ["cells", "All cells of the row: cells[\"Column\"].value"],
  ["row", "The row: row.index, row.starred, row.flagged, row.cells, row.record"],
  ["rowIndex", "The row number, starting at 0"],
]

/** GREL tokens: strings, /regex/ where an operand is expected, numbers, functions, variables. */
const grel = StreamLanguage.define<{ operand: boolean }>({
  name: "grel",
  startState: () => ({ operand: true }),
  token(stream, state) {
    if (stream.eatSpace()) return null
    const ch = stream.peek()
    if (ch === '"' || ch === "'") {
      stream.next()
      let escaped = false
      while (!stream.eol()) {
        const c = stream.next()
        if (escaped) escaped = false
        else if (c === "\\") escaped = true
        else if (c === ch) break
      }
      state.operand = false
      return "string"
    }
    if (ch === "/" && state.operand) {
      stream.next()
      while (!stream.eol()) {
        const c = stream.next()
        if (c === "\\") stream.next()
        else if (c === "/") break
      }
      stream.match(/^[a-z]*/)
      state.operand = false
      return "regexp"
    }
    if (stream.match(/^\d+(\.\d+)?/)) {
      state.operand = false
      return "number"
    }
    if (stream.match(/^[A-Za-z_]\w*/)) {
      const word = stream.current()
      state.operand = false
      if (stream.peek() === "(") return "function"
      if (word === "true" || word === "false" || word === "null") return "atom"
      if (VARIABLES.some(([v]) => v === word)) return "keyword"
      return "variableName"
    }
    const c = stream.next()
    state.operand = c !== ")" && c !== "]"
    return "+-*/%<>=!".includes(c ?? "") ? "operator" : "punctuation"
  },
  tokenTable: { function: tags.function(tags.variableName), regexp: tags.regexp },
})

const highlight = HighlightStyle.define([
  { tag: tags.string, color: "var(--syntax-string)" },
  { tag: tags.regexp, color: "var(--syntax-regex)" },
  { tag: tags.number, color: "var(--syntax-number)" },
  { tag: tags.atom, color: "var(--syntax-number)" },
  { tag: tags.function(tags.variableName), color: "var(--syntax-function)" },
  { tag: tags.keyword, color: "var(--syntax-variable)", fontWeight: "600" },
  { tag: tags.operator, color: "var(--muted-foreground)" },
])

const theme = EditorView.theme({
  "&": { backgroundColor: "transparent", fontSize: "0.875rem" },
  "&.cm-focused": { outline: "none" },
  ".cm-content": { fontFamily: "var(--font-mono, ui-monospace, monospace)", caretColor: "var(--foreground)", padding: "8px 0" },
  ".cm-line": { padding: "0 10px" },
  ".cm-placeholder": { color: "var(--muted-foreground)" },
  ".cm-matchingBracket": { backgroundColor: "var(--accent)" },
  ".cm-tooltip": { backgroundColor: "var(--popover)", color: "var(--popover-foreground)", border: "1px solid var(--border)", borderRadius: "8px" },
  ".cm-tooltip-autocomplete > ul > li": { padding: "2px 8px" },
  ".cm-tooltip-autocomplete > ul > li[aria-selected]": { backgroundColor: "var(--accent)", color: "var(--accent-foreground)" },
  ".cm-completionDetail": { color: "var(--muted-foreground)", marginLeft: "8px", fontStyle: "normal" },
  ".cm-completionInfo": { maxWidth: "22rem", padding: "6px 10px", lineHeight: "1.4" },
})

type Doc = { params: string; returns: string; description: string }
const entry = (name: string, d: Doc, type: string): Completion =>
  snippetCompletion(`${name}(\${})`, { label: name, type, detail: `(${d.params}) → ${d.returns}`, info: d.description, boost: type === "function" ? 0 : 1 })

const FUNCTIONS: Completion[] = [
  ...Object.entries(docs.functions as Record<string, Doc>).map(([n, d]) => entry(n, d, "function")),
  ...Object.entries(docs.controls as Record<string, Doc>).map(([n, d]) => entry(n, d, "keyword")),
]

function source(columns: string[]) {
  const vars: Completion[] = [
    ...VARIABLES.map(([label, info]) => ({ label, type: "variable", info, boost: 2 })),
    ...columns.map((c) => ({ label: `cells[${JSON.stringify(c)}].value`, type: "property", detail: "other column", boost: 1 })),
  ]
  return (ctx: CompletionContext) => {
    const word = ctx.matchBefore(/[\w$]*/)
    if (!word || (word.from === word.to && !ctx.explicit)) return null
    const afterDot = ctx.state.sliceDoc(word.from - 1, word.from) === "."
    return { from: word.from, options: afterDot ? FUNCTIONS : [...vars, ...FUNCTIONS], validFor: /^\w*$/ }
  }
}

export function ExpressionEditor({
  id,
  value,
  onChange,
  onSubmit,
  columns = [],
  invalid,
  autoFocus,
  placeholder,
}: {
  id?: string
  value: string
  onChange: (v: string) => void
  onSubmit?: () => void
  columns?: string[]
  invalid?: boolean
  autoFocus?: boolean
  placeholder?: string
}) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const latest = useRef({ onChange, onSubmit })
  latest.current = { onChange, onSubmit }

  useEffect(() => {
    const v = new EditorView({
      parent: host.current!,
      state: EditorState.create({
        doc: value,
        extensions: [
          history(),
          closeBrackets(),
          bracketMatching(),
          grel,
          syntaxHighlighting(highlight),
          theme,
          autocompletion({ override: [source(columns)], icons: false }),
          cmPlaceholder(placeholder ?? "value"),
          EditorView.lineWrapping,
          EditorView.contentAttributes.of({ "aria-label": "GREL expression", ...(id ? { id } : {}) }),
          keymap.of([
            { key: "Mod-Enter", run: () => (latest.current.onSubmit?.(), true) },
            ...closeBracketsKeymap,
            ...completionKeymap,
            ...historyKeymap,
            ...defaultKeymap,
          ]),
          EditorView.updateListener.of((u) => u.docChanged && latest.current.onChange(u.state.doc.toString())),
        ],
      }),
    })
    view.current = v
    if (autoFocus) v.focus()
    return () => v.destroy()
    // the editor is created once; later prop changes reach it through refs and the effect below
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    const v = view.current
    if (v && v.state.doc.toString() !== value) v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: value } })
  }, [value])

  return (
    <div
      ref={host}
      aria-invalid={invalid}
      className="min-h-20 rounded-lg border border-input bg-transparent shadow-xs focus-within:border-ring focus-within:ring-3 focus-within:ring-ring/50 aria-invalid:border-destructive"
    />
  )
}

/** Recent expressions, kept per browser (a convenience; losing it loses nothing). */
const RECENT_KEY = "oxrefine.recent-expressions"
export function recentExpressions(): string[] {
  try {
    return JSON.parse(localStorage.getItem(RECENT_KEY) ?? "[]")
  } catch {
    return []
  }
}
export function rememberExpression(e: string) {
  try {
    const list = [e, ...recentExpressions().filter((x) => x !== e)].slice(0, 20)
    localStorage.setItem(RECENT_KEY, JSON.stringify(list))
  } catch {
    /* storage unavailable: nothing to remember */
  }
}
