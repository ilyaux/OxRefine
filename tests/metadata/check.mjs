// Open OxRefine's archive in a real OpenRefine 3.10.1 and compare its metadata with the fixture:
//   cargo test -p ox-core --test orproject_conformance   (writes target/metadata-export/)
//   node tests/metadata/check.mjs
import { readFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"

const fx = JSON.parse(readFileSync(new URL("fixtures.json", import.meta.url)))
const archive = new URL("../../target/metadata-export/project.openrefine.tar.gz", import.meta.url)
const or = await startOpenRefine({ workspace: join(tmpdir(), "oxrefine-metadata-check"), jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC"] })
let bad = 0
try {
  const file = new File([readFileSync(archive)], "project.openrefine.tar.gz")
  const r = await or.command("import-project", { "project-file": file }, { json: false })
  const project = new URL(r.headers.get("location") ?? "", or.url).searchParams.get("project")
  if (!project) throw new Error(`not imported: ${r.status}`)
  const meta = await or.get(`/command/core/get-project-metadata?project=${project}`)
  for (const [k, want] of Object.entries(fx.metadata)) {
    if (JSON.stringify(meta[k]) !== JSON.stringify(want)) (bad++, console.log(`FAIL ${k}: OpenRefine shows ${JSON.stringify(meta[k])}, expected ${JSON.stringify(want)}`))
  }
  console.log(bad ? `${bad} fields differ` : `all ${Object.keys(fx.metadata).length} metadata fields match in OpenRefine`)
} finally {
  or.stop()
}
process.exit(bad ? 1 : 0)
