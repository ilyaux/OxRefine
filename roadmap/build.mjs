// node roadmap/build.mjs  ->  ROADMAP.md and docs/readme/roadmap.svg from roadmap/tasks.json (the single source of the plan)
import { readFileSync, writeFileSync } from "node:fs"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

const planned = (t) => t.status === "todo" || t.status === "backlog"
const MILESTONES = ["v0.1", "v0.2", "v0.5", "v1.0", "later"]

export function renderRoadmap({ phases, tasks, currentRelease }) {
  const count = (f) => tasks.filter(f).length
  const done = count((t) => t.status === "done")
  const item = (t) => {
    const mark = t.status === "done" ? "[x]" : "[ ]"
    const state = t.status === "review" ? " (review)" : t.status === "doing" ? " *(in progress)*" : ""
    return `- ${mark} **${t.title}**${state} — ${t.detail} <sub>${t.milestone} · ${t.priority} · ${t.size} · OpenRefine: ${t.ref}</sub>`
  }
  const out = [
    "# OxRefine roadmap",
    "",
    "Full OpenRefine parity, in Rust.",
    "",
    ...(currentRelease ? [`Current release: [**${currentRelease.tag}**](${currentRelease.url}), published ${currentRelease.published}.`, ""] : []),
    '<img src="docs/readme/roadmap.svg" width="100%" alt="Progress by phase: tasks done, in review, in progress and planned.">',
    "",
    `**${done} of ${tasks.length} tasks done.** Generated from \`roadmap/tasks.json\` by \`node roadmap/build.mjs\`; statuses follow the Kanzo board. **Done** is closed by the owner, **review** waits for the owner and doesn't count as done, **planned** is todo and backlog together. Shipped features may still be in review.`,
    "",
    "| Milestone | Tasks | Done | Review | In progress | Planned |",
    "|---|---|---|---|---|---|",
    ...MILESTONES.map(
      (m) => `| ${m} | ${count((t) => t.milestone === m)} | ${count((t) => t.milestone === m && t.status === "done")} | ${count((t) => t.milestone === m && t.status === "review")} | ${count((t) => t.milestone === m && t.status === "doing")} | ${count((t) => t.milestone === m && planned(t))} |`
    ),
    `| **Total** | **${tasks.length}** | **${done}** | **${count((t) => t.status === "review")}** | **${count((t) => t.status === "doing")}** | **${count(planned)}** |`,
    "",
    "Milestones are planning groups, not release checklists: **v0.2** the core cleaning workflow, **v0.5** formats, clustering, exporters and distribution, **v1.0** reconciliation and UI parity, **later** specialised workflows. Features can ship before their group; the release notes say what each version has.",
    "",
  ]
  for (const p of phases) {
    const ts = tasks.filter((t) => t.phase === p.id)
    const d = ts.filter((t) => t.status === "done").length
    out.push(`## ${p.name}`, "", `${p.goal} **${d} of ${ts.length} done.**`, "")
    // a phase that is all done folds away; open work stays in sight
    const open = ts.filter((t) => t.status !== "done"), closed = ts.filter((t) => t.status === "done")
    for (const t of open) out.push(item(t))
    if (closed.length) {
      if (open.length) out.push("")
      out.push(`<details><summary>${closed.length} done</summary>`, "", ...closed.map(item), "", "</details>")
    }
    out.push("")
  }
  return out.join("\n")
}

/** Progress by phase as a stacked bar per phase, in the README charts' style (tools/readme-art.mjs). */
export function renderRoadmapSvg({ phases, tasks }) {
  const W = 960, X = 230, L = 600, top = 92, G = 30
  const f = (v) => +v.toFixed(1)
  const parts = [["done", "ox"], ["review", "ox2"], ["doing", "warn"], ["planned", "or2"]]
  let body = ""
  phases.forEach((p, i) => {
    const ts = tasks.filter((t) => t.phase === p.id), y = top + i * G
    const by = { done: 0, review: 0, doing: 0, planned: 0 }
    for (const t of ts) by[planned(t) ? "planned" : t.status]++
    let x = X
    body += `<text x="24" y="${y + 13}" font-size="13">${p.name.replaceAll("&", "&amp;").replaceAll("<", "&lt;")}</text>`
    for (const [k, cls] of parts) {
      const w = (by[k] / ts.length) * L
      if (w) body += `<rect class="${cls}" x="${f(x)}" y="${y + 2}" width="${f(w)}" height="14"><animate attributeName="opacity" values="0;0;1" keyTimes="0;0.5;1" dur="${(0.4 + i * 0.05).toFixed(2)}s" fill="freeze"/></rect>`
      x += w
    }
    body += `<text class="m" x="${W - 24}" y="${y + 13}" font-size="12" text-anchor="end">${by.done} <tspan class="mute">/ ${ts.length}</tspan></text>`
  })
  const y = top + phases.length * G + 14
  const done = tasks.filter((t) => t.status === "done").length
  const legend = parts.map(([k, cls], i) => `<rect class="${cls}" x="${W - 380 + i * 92}" y="28" width="12" height="12"/><text x="${W - 362 + i * 92}" y="39" font-size="12">${k}</text>`).join("")
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${y + 36}" viewBox="0 0 ${W} ${y + 36}" role="img" aria-label="Roadmap progress: ${done} of ${tasks.length} tasks done">
<title>Roadmap progress: ${done} of ${tasks.length} tasks done</title>
<style>
svg{--bg:#FFFFFF;--ink:#111113;--mute:#5F5F66;--rule:#E4E4E0;--ox:#1D4ED8;--ox2:#5B7FE6;--or2:#D4D4D8;--warn:#B45309}
@media (prefers-color-scheme:dark){svg{--bg:#0D1117;--ink:#E6EDF3;--mute:#9198A1;--rule:#30363D;--ox:#4C8DFF;--ox2:#2B5CB8;--or2:#30363D;--warn:#E3A008}}
text{font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;fill:var(--ink)}
.m{font-family:ui-monospace,SFMono-Regular,"SF Mono",Menlo,Consolas,monospace}.mute{fill:var(--mute)}.b{font-weight:600}
.ox{fill:var(--ox)}.ox2{fill:var(--ox2)}.or2{fill:var(--or2)}.warn{fill:var(--warn)}.rule{stroke:var(--rule);fill:none}
</style>
<rect fill="var(--bg)" width="${W}" height="${y + 36}"/>
<rect class="rule" x="0.5" y="0.5" width="${W - 1}" height="${y + 35}"/>
<text x="24" y="38" font-size="17" class="b">Roadmap</text>
<text x="24" y="60" font-size="12" class="mute">tasks by phase, from roadmap/tasks.json</text>
${legend}
${body}
<line class="rule" x1="24" x2="${W - 24}" y1="${y - 8}" y2="${y - 8}"/>
<text x="24" y="${y + 14}" font-size="13" class="b">All phases</text>
<text class="m b" x="${X}" y="${y + 14}" font-size="13">${Math.round((done / tasks.length) * 100)}% done</text>
<text class="m b" x="${W - 24}" y="${y + 14}" font-size="13" text-anchor="end">${done} <tspan class="mute">/ ${tasks.length}</tspan></text>
</svg>
`
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const plan = JSON.parse(readFileSync(new URL("tasks.json", import.meta.url), "utf8"))
  writeFileSync(new URL("../ROADMAP.md", import.meta.url), renderRoadmap(plan))
  writeFileSync(new URL("../docs/readme/roadmap.svg", import.meta.url), renderRoadmapSvg(plan))
  console.log(`ROADMAP.md: ${plan.tasks.length} tasks in ${plan.phases.length} phases`)
}
