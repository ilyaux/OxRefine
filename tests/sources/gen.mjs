// Regenerate tests/sources/fixtures.json from a real OpenRefine 3.10.1:  node tests/sources/gen.mjs
// Each case goes through OpenRefine's import wizard with its defaults: load the clipboard text or
// download the URL (from a local server), guess the format, take the parser defaults, create the project.
import { createServer } from "node:http"
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, FILES } from "./cases.mjs"

const PORT = 41873
const server = createServer((req, res) => {
  const f = FILES[req.url]
  if (!f) return res.writeHead(404).end("not found")
  res.writeHead(200, { "content-type": f[0] }).end(Buffer.from(f[1], f[2] ?? "utf8"))
}).listen(PORT, "127.0.0.1")

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-sources-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const out = []
  for (const c of CASES) {
    const r = await or.wizard(c.clipboard !== undefined ? { clipboard: c.clipboard } : { download: `http://127.0.0.1:${PORT}${c.url}` }, c.choose)
    out.push({ ...c, ...r })
    console.log(`${c.name.padEnd(32)} ${r.format.padEnd(26)} ${r.columns.length} cols x ${r.rows.length} rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", files: FILES, cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/sources/fixtures.json`)
} finally {
  or.stop()
  server.close()
}
