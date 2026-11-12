// Regenerate tests/facets/fixtures.json from a real OpenRefine 3.10.1:  node tests/facets/gen.mjs
import { writeFileSync } from "node:fs"
import { join } from "node:path"
import { tmpdir } from "node:os"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES, CORPUS } from "./cases.mjs"

const or = await startOpenRefine({
  workspace: join(tmpdir(), "oxrefine-facet-fixtures"),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8", "-Dsun.jnu.encoding=UTF-8"],
})
try {
  const out = []
  for (const c of CASES) {
    const project = await or.createProject(CORPUS, { name: "facets" })
    // preferences a case needs (OpenRefine keeps them for the whole workspace, so they're cleared after)
    for (const [name, value] of Object.entries(c.prefs ?? {})) await or.command("set-preference", { name, value: JSON.stringify(value) })
    if (c.ops) {
      const res = await or.command("apply-operations", { project, operations: c.ops })
      if (res.code !== "ok") throw new Error(`${c.name}: ${JSON.stringify(res)}`)
      await or.idle(project)
    }
    const facets = (await or.command("compute-facets", { project, engine: c.engine })).facets
    const engine = encodeURIComponent(JSON.stringify(c.engine))
    const got = await or.get(`/command/core/get-rows?project=${project}&start=0&limit=1000&engine=${engine}`)
    out.push({ ...c, facets, matching: got.rows.map((r) => r.i), filtered: got.filtered })
    await or.command("delete-project", { project })
    for (const name of Object.keys(c.prefs ?? {})) await or.command("set-preference", { name, value: "null" })
    console.log(`${c.name.padEnd(40)} ${got.rows.length} matching rows`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", cases: out }, null, 1) + "\n")
  console.log(`${out.length} cases -> tests/facets/fixtures.json`)
} finally {
  or.stop()
}
