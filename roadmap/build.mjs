// node roadmap/build.mjs  ->  ROADMAP.md from roadmap/tasks.json (the single source of the plan)
import { readFileSync, writeFileSync } from "node:fs"
import { resolve } from "node:path"
import { fileURLToPath } from "node:url"

export function renderRoadmap({ phases, tasks, currentRelease }) {
  const mark = { done: "[x]", doing: "[~]", review: "[~]", todo: "[ ]", backlog: "[ ]" }
  const count = (f) => tasks.filter(f).length
  const planned = (t) => t.status === "todo" || t.status === "backlog"
  const out = [
    "# OxRefine roadmap: full OpenRefine parity in Rust",
    "",
    ...(currentRelease ? [`Current release: [**${currentRelease.tag}**](${currentRelease.url}), published ${currentRelease.published}.`, ""] : []),
    `Generated from \`roadmap/tasks.json\` by \`node roadmap/build.mjs\`. ${count((t) => t.status === "done")} of ${tasks.length} tasks done.`,
    "",
    "Statuses follow Kanzo: **done** is closed by the owner; **review** awaits owner review and is not counted as done; **doing** is in progress; **planned** combines todo and backlog. Shipped features may still be in review.",
    "",
    "Milestone labels are the original planning groups, not release checklists. **v0.2** covers the core cleaning workflow, **v0.5** formats, clustering, exporters and distribution, **v1.0** reconciliation and UI parity, and **later** specialized workflows. Features can ship earlier than their planning group; see the release notes for what each version includes.",
    "",
    "| Milestone | Tasks | Done | Review | In progress | Planned |",
    "|---|---|---|---|---|---|",
    ...["v0.1", "v0.2", "v0.5", "v1.0", "later"].map(
      (m) => `| ${m} | ${count((t) => t.milestone === m)} | ${count((t) => t.milestone === m && t.status === "done")} | ${count((t) => t.milestone === m && t.status === "review")} | ${count((t) => t.milestone === m && t.status === "doing")} | ${count((t) => t.milestone === m && planned(t))} |`
    ),
    `| **Total** | **${tasks.length}** | **${count((t) => t.status === "done")}** | **${count((t) => t.status === "review")}** | **${count((t) => t.status === "doing")}** | **${count(planned)}** |`,
    ""
  ]
  for (const p of phases) {
    const ts = tasks.filter((t) => t.phase === p.id)
    out.push(`## ${p.name}`, "", p.goal, "")
    for (const t of ts)
      out.push(
        `- ${mark[t.status]} **${t.title}**${t.status === "review" ? " (review)" : ""} · ${t.milestone} · ${t.priority} · ${t.size}. ${t.detail} _OpenRefine: ${t.ref}_`
      )
    out.push("")
  }
  return out.join("\n")
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const plan = JSON.parse(readFileSync(new URL("tasks.json", import.meta.url), "utf8"))
  writeFileSync(new URL("../ROADMAP.md", import.meta.url), renderRoadmap(plan))
  console.log(`ROADMAP.md: ${plan.tasks.length} tasks in ${plan.phases.length} phases`)
}
