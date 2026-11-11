// Regenerate tests/corpus/fixtures.json from a real OpenRefine 3.10.1:  node tests/corpus/gen.mjs
// (run make.mjs first if the files changed). Each file is uploaded to OpenRefine's import wizard.
import { readFileSync, writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-corpus-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const out = []
  for (const c of CASES) {
    const bytes = readFileSync(new URL(`files/${c.file}`, import.meta.url))
    try {
      const r = await or.wizard({ upload: new File([bytes], c.file) }, c.choose)
      out.push({ ...c, ...r })
      console.log(`${c.name.padEnd(34)} ${r.format.padEnd(22)} ${String(r.options.encoding ?? "").padEnd(12)} ${r.columns.length} cols x ${r.rows.length} rows`)
    } catch (e) {
      out.push({ ...c, error: String(e.message ?? e).slice(0, 300) })
      console.log(`${c.name.padEnd(34)} ERROR ${String(e.message ?? e).slice(0, 120)}`)
    }
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/corpus/fixtures.json`)
} finally {
  or.stop()
}
