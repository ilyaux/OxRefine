export type JsonPaths = {
  paths: { path: string[]; count: number; type: string; fields: string[] }[]
  suggestedPath: string[]
  sampled: boolean
  file: string
  limits: { bytes: number; nodes: number; depth: number; paths: number; records: number; keyCharacters: number }
}

/** A native list keeps a sampled JSON outline usable with the keyboard and long field names. */
export function JsonPathPicker({ data, value, onChange }: { data: JsonPaths; value: string[]; onChange: (path: string[]) => void }) {
  const key = JSON.stringify(value)
  const selected = data.paths.find((p) => JSON.stringify(p.path) === key)
  return (
    <div className="flex flex-col gap-1.5 px-4 pt-3 text-xs">
      <label htmlFor="json-record-paths" className="font-medium">Record paths</label>
      <span className="truncate text-muted-foreground" title={data.file}>First source: {data.file}</span>
      <select
        id="json-record-paths"
        size={Math.max(2, Math.min(8, data.paths.length))}
        value={key}
        onChange={(e) => onChange(JSON.parse(e.target.value))}
        className="w-full min-w-0 rounded-[3px] border border-input bg-background font-mono text-xs outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        {!selected && <option value={key}>Manual: {key}</option>}
        {data.paths.map((p) => (
          <option key={JSON.stringify(p.path)} value={JSON.stringify(p.path)} title={p.fields.join(", ")}>
            {JSON.stringify(p.path)} · {p.count.toLocaleString()} {p.type}{data.sampled ? " in sample" : ""}
          </option>
        ))}
      </select>
      {selected?.fields.length ? <span className="break-words text-muted-foreground">Example fields (up to 8): {selected.fields.join(", ")}</span> : null}
      <span className="text-muted-foreground">
        {data.sampled ? `Sample counts only: up to ${(data.limits.bytes / 1024).toLocaleString()} KiB decoded text, ${data.limits.nodes.toLocaleString()} nodes, ${data.limits.records} JSONL roots.` : "Counts cover the first source file."}
        {" "}Outline: depth {data.limits.depth}, {data.limits.paths} paths; keys up to {data.limits.keyCharacters} characters.
      </span>
    </div>
  )
}
