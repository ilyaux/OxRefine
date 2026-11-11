// Write benchmarks/report/index.html from benchmarks/results.json (node benchmarks/compare.mjs first).
// node benchmarks/report.mjs [baseline.json]   — a baseline (an earlier results.json) adds a "then → now" column.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs"

const here = new URL("./", import.meta.url)
const now = JSON.parse(readFileSync(new URL("results.json", here)))
const base = process.argv[2] ? JSON.parse(readFileSync(process.argv[2])) : null
const STEPS = [["import", "Импорт"], ["facet", "Фасет"], ["cluster", "Кластеризация"], ["recipe", "Рецепт (5 шагов)"], ["export", "Экспорт CSV"]]
const total = (x) => (x && x.import != null ? STEPS.reduce((a, [k]) => a + x[k], 0) : null)
const n = (v) => (v == null ? "—" : v.toLocaleString("ru-RU"))
const t = (ms) => (ms == null ? "—" : ms >= 1000 ? `${(ms / 1000).toFixed(1).replace(".", ",")} с` : `${Math.round(ms)} мс`)
const x = (a, b) => (a && b ? `${(b / a).toFixed(b / a >= 10 ? 0 : 1).replace(".", ",")}×` : "—")
const esc = (s) => String(s).replace(/&/g, "&amp;").replace(/</g, "&lt;")

const rows = now.results.map((r) => ({ ...r, oxT: total(r.ox), orT: r.or ? total(r.or) : null, then: base?.results.find((b) => b.rows === r.rows) }))
const big = rows.filter((r) => r.orT).at(-1)
const thenCol = !!base

const html = `<!doctype html>
<html lang="ru"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Бенчмарк OxRefine</title>
<style>
:root { --bg: #fbfaf8; --fg: #1f1e1c; --muted: #6b6862; --line: #e4e1db; --panel: #ffffff; --accent: #2f6f5e; --bad: #a33a2b; }
@media (prefers-color-scheme: dark) { :root { --bg: #171614; --fg: #ece9e3; --muted: #a29e96; --line: #33312d; --panel: #1f1e1b; --accent: #7cc4ad; --bad: #e08a7c; } }
body { background: var(--bg); color: var(--fg); font: 15px/1.5 system-ui, -apple-system, "Segoe UI", sans-serif; margin: 0; }
main { max-width: 980px; margin: 0 auto; padding: 32px 16px 48px; display: flex; flex-direction: column; gap: 28px; }
h1 { font-size: 1.7rem; margin: 0; font-weight: 600; } h2 { font-size: 1.15rem; margin: 0 0 8px; }
p { margin: 0; max-width: 72ch; } .muted { color: var(--muted); font-size: .9rem; }
.tiles { display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 12px; }
.tile { background: var(--panel); border: 1px solid var(--line); border-radius: 10px; padding: 16px; }
.big { font-size: 2rem; font-weight: 600; font-variant-numeric: tabular-nums; color: var(--accent); }
.scroll { overflow-x: auto; } table { border-collapse: collapse; width: 100%; font-variant-numeric: tabular-nums; }
th, td { text-align: right; padding: 7px 10px; border-bottom: 1px solid var(--line); white-space: nowrap; }
th:first-child, td:first-child { text-align: left; } th { color: var(--muted); font-weight: 500; font-size: .85rem; }
.good { color: var(--accent); font-weight: 600; } .bad { color: var(--bad); }
</style></head><body><main>
<header><h1>OxRefine против OpenRefine 3.10.1</h1>
<p class="muted">Прогон ${esc(now.when.slice(0, 16).replace("T", " "))} UTC · одинаковые данные и сценарий через HTTP API каждого: импорт CSV, текстовый фасет, кластеризация fingerprint, рецепт OpenRefine из 5 шагов, экспорт CSV · куча OpenRefine ${esc(now.orMemory)} · по одному прогону на размер</p></header>
<section class="tiles">
<div class="tile"><div class="big">${x(big.oxT, big.orT)}</div>быстрее весь сценарий на ${n(big.rows)} строк (${t(big.oxT)} против ${t(big.orT)})</div>
<div class="tile"><div class="big">${x(big.ox.peak, big.or.peak)}</div>меньше пик памяти (${big.ox.peak} против ${n(big.or.peak)} МБ)</div>
<div class="tile"><div class="big">${n(big.diff?.same ?? 0)}</div>из ${n(big.diff?.cells ?? 0)} выходных ячеек совпадают с OpenRefine</div>
${rows.some((r) => !r.orT) ? `<div class="tile"><div class="big">${n(rows.find((r) => !r.orT).rows)}</div>строк: OpenRefine падает без памяти, OxRefine — ${t(rows.find((r) => !r.orT).oxT)}, ${rows.find((r) => !r.orT).ox.peak} МБ</div>` : ""}
</section>
<section><h2>Весь сценарий</h2><div class="scroll"><table>
<tr><th>Строк</th><th>OxRefine</th><th>OpenRefine</th><th>Быстрее</th><th>Пик RAM Ox</th><th>Пик RAM OR</th><th>Меньше RAM</th><th>Совпадает ячеек</th>${thenCol ? "<th>Ox тогда → сейчас</th>" : ""}</tr>
${rows.map((r) => `<tr><td>${n(r.rows)}</td><td>${t(r.oxT)}</td><td>${r.orT ? t(r.orT) : '<span class="bad">нет памяти</span>'}</td><td class="good">${x(r.oxT, r.orT)}</td><td>${r.ox.peak} МБ</td><td>${r.or?.peak ? n(r.or.peak) + " МБ" : "—"}</td><td class="good">${x(r.ox.peak, r.or?.peak)}</td><td>${r.diff ? `${n(r.diff.same)} / ${n(r.diff.cells)}` : "—"}</td>${thenCol ? `<td>${r.then ? `${t(total(r.then.ox))}, ${r.then.ox.peak} МБ → ${t(r.oxT)}, ${r.ox.peak} МБ` : "—"}</td>` : ""}</tr>`).join("\n")}
</table></div></section>
<section><h2>По шагам, ${n(big.rows)} строк</h2><div class="scroll"><table>
<tr><th>Шаг</th><th>OxRefine</th><th>OpenRefine</th><th>Быстрее</th>${thenCol ? "<th>Ox тогда</th>" : ""}</tr>
${STEPS.map(([k, label]) => `<tr><td>${label}</td><td>${t(big.ox[k])}</td><td>${t(big.or[k])}</td><td class="good">${x(big.ox[k], big.or[k])}</td>${thenCol ? `<td>${big.then ? t(big.then.ox[k]) : "—"}</td>` : ""}</tr>`).join("\n")}
</table></div>
<p class="muted">Шаги короче ~20 мс шумные: один прогон на размер.</p></section>
</main></body></html>
`
mkdirSync(new URL("report/", here), { recursive: true })
writeFileSync(new URL("report/index.html", here), html)
console.log("benchmarks/report/index.html")
