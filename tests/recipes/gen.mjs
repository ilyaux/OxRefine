// Regenerate references from OpenRefine 3.10.1: node tests/recipes/gen.mjs
import assert from "node:assert/strict"
import { writeFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { startOpenRefine } from "../../tools/openrefine.mjs"
import { CASES } from "./cases.mjs"

const or = await startOpenRefine({ port: 3339, workspace: fileURLToPath(new URL("../../target/recipes-fixture-workspace/", import.meta.url)),
  jvm: ["-Duser.language=en", "-Duser.country=US", "-Duser.timezone=UTC", "-Dfile.encoding=UTF-8"] })
try {
  const fixtures = []
  for (const c of CASES) {
    const batchProject = await or.createProject(c.csv, { name: `${c.name} batch`, guessTypes: c.guessTypes ?? false })
    const batchResult = await or.runOps(batchProject, c.ops)
    const batch = { result: { code: batchResult.code, ...(batchResult.message ? { message: batchResult.message } : {}) },
      table: await or.table(batchProject), history: (await or.get(`/command/core/get-history?project=${batchProject}`)).past.map((h) => h.description) }
    await or.command("delete-project", { project: batchProject })
    const project = await or.createProject(c.csv, { name: c.name, guessTypes: c.guessTypes ?? false })
    if (c.guessTypes) {
      const archive = await fetch(`${or.url}/command/core/export-project?project=${project}`, { method: "POST" })
      assert.equal(archive.status, 200)
      writeFileSync(new URL("typed-input.openrefine.tar.gz", import.meta.url), Buffer.from(await archive.arrayBuffer()))
    }
    const steps = []
    for (const op of c.ops) {
      const result = await or.runOps(project, [op])
      steps.push({ result: { code: result.code, ...(result.message ? { message: result.message } : {}) }, table: await or.table(project), history: (await or.get(`/command/core/get-history?project=${project}`)).past.map((h) => h.description) })
      if (result.code !== "ok") break
    }
    if (c.failureStep) assert.notEqual(steps[c.failureStep - 1].result.code, "ok", c.name)
    else assert.equal(steps.at(-1).result.code, "ok", `${c.name}: ${JSON.stringify(steps.at(-1).result)}`)
    fixtures.push({ ...c, batch, steps })
    await or.command("delete-project", { project })
    console.log(`${c.name}: ${steps.length} steps, ${steps.at(-1).result.code}`)
  }
  writeFileSync(new URL("fixtures.json", import.meta.url), JSON.stringify({ openrefine: "3.10.1", sources: "sources.json", cases: fixtures }, null, 2) + "\n")
} finally {
  or.stop()
}
