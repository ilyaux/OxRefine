# Release 0.6.0 benchmark preparation

Status: the first quiet protocol is archived in [pre-fix-af937201/verdict.json](pre-fix-af937201/verdict.json). Correctness checks passed, but the people and low-cardinality history performance gates failed. Its reopen timings also reused the candidate's live project cache. Preserve those raw observations; they are not the final release figures. The final protocol below requires the corrected frozen candidate and true process restart.

The first post-fix people run is preserved in [keepalive-31e9e0d1/verdict.json](keepalive-31e9e0d1/verdict.json): data checks passed and the unchanged phase gate still failed at 2M rows. A separate bounded transport diagnostic found keep-alive response-body stalls. The final protocol therefore uses explicit **`Connection: close` for every benchmark HTTP request, identically for both OxRefine builds and OpenRefine**. This connection mode is recorded in raw metadata. Historical runs keep their original transport and numbers; they are not silently relabelled as the new protocol.

Run only after builds, fixture provisioning and browser tests stop competing for CPU/disk. Freeze one candidate binary and record its SHA-256; do not rebuild it during measurement. The 50-million-row spill proof remains a separate workload.

## Inputs and baseline

- Existing people generator `benchmarks/gen.mjs`, seed 42, seven input columns. Sizes: 100k, 250k, 500k, 1M and 2M. The unchanged five-operation recipe is `benchmarks/recipe.json`. Both engines receive exactly the same input file. Fresh server process and private project library per repetition; three repetitions, alternating engine order.
- OpenRefine **3.10.1**, `-Xms256M -Xmx1400M`, same laptop and local HTTP workflow as the historical report. The comparison launcher currently uses the installed Windows distribution. Time is wall time per HTTP phase; memory is that backend process's OS peak working set in MiB. Parent Node, database services and OS overhead are excluded. These are default-storage tests; clear `OXREFINE_CODE_BUDGET_MIB` first.
- Official v0.5.3 CLI archives for Windows and Linux are downloaded under ignored `target/release-baseline-v0.5.3/`. GitHub asset digests, extracted executable hashes and tag/commit verification are in [baseline.json](baseline.json). The annotated tag resolves to `0912b4f9c4a7fe6f5c8346cd28425c06638366c7`.
- Cardinality/history retains the existing low/mixed/high rules, 40 history steps and undo/redo. For reopen, stop the old backend, start a fresh backend on the same private library and time `open_project` after startup readiness. This forces native file loading and history replay rather than selecting a project from the live browser cache. Record startup separately and retain both backend PIDs and lifetime peaks; whole-workflow peak is their maximum. OS filesystem caches remain warm, so this is not a cold-disk measurement. Compare v0.5.3 and the frozen candidate on the same OS. Do not mix Linux timings with Windows timings or describe these as OpenRefine ratios.

## Commands for the quiet measurement phase

PowerShell, from the repository root. `$Candidate` must point to the frozen **0.6.0 candidate**, not a binary being rebuilt. The output directory already contains this plan and baseline manifest.

```powershell
$Candidate = 'target/benchmark-candidate-v0.6.0/bulk/ox.exe'
$Base = 'target/release-baseline-v0.5.3/windows/ox-v0.5.3-x86_64-pc-windows-msvc/ox.exe'
$env:OR_MEM = '1400M'
Remove-Item Env:OXREFINE_CODE_BUDGET_MIB, Env:OXREFINE_SPILL_DIR -ErrorAction SilentlyContinue
Get-FileHash -Algorithm SHA256 $Candidate, $Base

node benchmarks/check.mjs --base $Base --new $Candidate --runs 3 --out benchmarks/release-0.6.0/people-regression.json 100000 1000000 2000000
node benchmarks/check.mjs --profile cardinality --history 40 --base $Base --new $Candidate --runs 3 --out benchmarks/release-0.6.0/cardinality-regression.json 10000 100000
node benchmarks/compare.mjs --runs 3 --ox $Candidate --or-port 3336 --ox-port 3036 --out benchmarks/release-0.6.0/openrefine-1400M.json 100000 250000 500000 1000000 2000000
node benchmarks/report.mjs --input benchmarks/release-0.6.0/openrefine-1400M.json --out benchmarks/release-0.6.0/report.html
```

Run the people gate first, then cardinality with fresh-process reopen. Stop and report a failing gate before continuing to the OpenRefine and database comparisons. Planning allowances, not measured runtimes: about 5–15 minutes for the OpenRefine comparison, 1–3 minutes for the people regression check, and 3–8 minutes for cardinality/history. Failure timeouts can extend this. Run sequentially. The initial small OpenRefine comparison also acts as a real launcher/HTTP smoke check; pure runner tests do not replace it.

The OpenRefine report includes each raw repetition, medians of phases and **medians of per-run totals**, input/recipe/generator/binary hashes and host facts. Every completed pair compares every output cell, row count, header, facet-choice count and cluster count. Three failed 2M runs are recorded as allocation failures only when the JVM log contains explicit OOM evidence. A timeout or other error is labelled separately and aborts publication; no speed ratio is calculated for a failed comparison. Partial reports have `complete: false` and `valid: false`.

The existing regression harness uses fastest phase timings and median peak memory for its established gate (`base*1.3+15ms`, `base*1.1+8MiB`). It retains all raw runs. A regression is a finding to inspect, not a reason to discard slower repetitions.

## New database workload

This is an import workload with **100,000 real SQL rows per backend**, not a three-row timing sample. Native isolated fixture services are PostgreSQL 17.9, MySQL 8.4.13 and MariaDB 11.4.13. Their owner provisioned identical tables and verified 100,000 rows, IDs 0–99,999 and 1,031 SQL NULLs. Reader accounts have SELECT access. Reproducible source SQL is in `tests/database/reference-benchmark-postgres.sql` and `tests/database/reference-benchmark-mysql-family.sql`.

Schema: `id INTEGER PRIMARY KEY, label VARCHAR(40) NOT NULL, quantity INTEGER NULL`. Row `i` has label `item-i`; quantity is NULL when `i % 97 == 0`, otherwise `i % 1000`. Query: `SELECT id,label,quantity FROM db_bench_fixture ORDER BY id`.

Prepare the equivalent **new** SQLite file, then measure only after the quiet-phase go-ahead:

```powershell
python benchmarks/database-fixture.py target/database-benchmark/fixture.sqlite
node benchmarks/database.mjs --config target/database-live/connections.json --sqlite target/database-benchmark/fixture.sqlite --new $Candidate --runs 3 --out benchmarks/release-0.6.0/database.json
```

The private connection file stays ignored and is read directly by the runner. Passwords go only into per-request credentials and never into published JSON. The runner does not start or stop database services. Expect roughly 1–3 minutes, subject to actual service/host load.

Each repetition uses a fresh OxRefine process and project library. Database services and OS caches remain warm. Record SQL prepare/fetch/typed snapshot time, native project creation/save time, their sum, process-restarted reopen, export, import peak working set, and workflow peak separately. Restart the backend on the same private library before timing `open_project`; process startup is recorded separately. The saved project must reopen without another SQL query. Whole-workflow peak is the maximum HWM across the pre-restart and post-restart backend lifetimes. Database-server RAM is outside the OxRefine peak and is stated explicitly. Every exported row is checked by an independent expected CSV digest, and all 100,000 rows are checked for numeric/text/null cell types after reopen, outside phase timing. The four backends measure this local fixture; they do not establish remote-network throughput or a universal database ranking.

## Publication handoff

OpenRefine-comparison and database reports need all intended repetitions, `complete: true`, `valid: true` and matching binary hashes before they can feed release claims. The existing `check.mjs` regression JSON does not contain those flags: check its complete dataset/mode coverage, three raw base/new repetitions, successful correctness assertions and recorded exit/verdict separately. Exit 1 from its performance gate preserves a real regression finding; it does not invalidate the exported-cell equality checks or justify dropping slower samples. The release verdict manifest records these outcomes without changing raw measurements.

Keep historical raw files; update README tables, per-step figures and wording from the new artifacts. Send SVG authors the final report paths and SHA-256 values. Two new visuals can use the separately validated spill proof and database import report; mark measurements and limits in each visual. Update accessibility alt text to the same figures.
