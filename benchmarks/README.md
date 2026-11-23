# Benchmarks

Every number here was measured, three runs per case, on one Windows laptop (AMD Ryzen AI 7 350, about 31 GiB RAM) unless it says otherwise. Raw reports keep every run, the binaries' SHA-256 and the input hashes; the 0.6.0 ones are in [release-0.6.0/](release-0.6.0/), with the [protocol](release-0.6.0/PLAN.md) and the [verdict](release-0.6.0/verdict.json) that ties them together. The pictures are drawn from those reports by `node tools/readme-art.mjs`, which refuses an incomplete or inconsistent report.

| | |
|---|---|
| [Against OpenRefine](#against-openrefine-3101) | the same five-step workflow, 100k to 2M rows |
| [Importing from a database](#importing-from-a-database) | PostgreSQL, MySQL, MariaDB and SQLite, 100k rows |
| [Bigger than memory](#bigger-than-memory-experimental) | 50 million rows under a 1 GiB limit |
| [Regression gates](#regression-gates) | what CI and release checks compare |
| [Cardinality and history](#cardinality-and-history) | low, mixed and unique values, 40-step histories, restart and reopen |

## Against OpenRefine 3.10.1

Both tools ran the same people workflow through their own HTTP APIs: CSV import, a text facet, fingerprint clustering, a five-operation OpenRefine recipe and CSV export. OpenRefine had a fixed 1,400 MB Java heap. Every request used `Connection: close` in both tools. All twelve completed pairs from 100k to 1M rows matched every output cell, row count, facet choice and cluster; at 2M rows OpenRefine ran out of its heap in all three attempts.

<img src="../docs/readme/bench-time.svg" width="100%" alt="Whole-workflow time by table size: 0.66 s against 15.0 s at 1M rows; OpenRefine runs out of memory at 2M.">

<img src="../docs/readme/bench-steps.svg" width="100%" alt="Each step at 1M rows on a log scale.">

<img src="../docs/readme/bench-memory.svg" width="100%" alt="Peak memory by table size.">

| Rows | OxRefine | OpenRefine | Faster | Peak RAM (OxRefine / OpenRefine) |
|---|---|---|---|---|
| 100,000 | 0.09 s | 1.75 s | 19× | 24 / 460 MiB |
| 250,000 | 0.15 s | 3.40 s | 23× | 38 / 820 MiB |
| 500,000 | 0.28 s | 5.91 s | 21× | 64 / 986 MiB |
| 1,000,000 | 0.66 s | 15.0 s | 23× | 116 / 1,420 MiB |
| 2,000,000 | 1.38 s | out of memory | | 220 MiB / out of memory |

Totals are medians of whole runs, so they can differ from the sum of the per-step medians. Peak RAM is the backend process's Windows `PeakWorkingSet64`; a Java heap setting is not a limit on process RAM. The data repeats values the way messy real columns do; columns where every value is different gain less, see [cardinality and history](#cardinality-and-history). [Raw report](release-0.6.0/openrefine-1400M.json).

```bash
# OpenRefine 3.10.1 in ../OpenRefine/openrefine-3.10.1
cargo build --release --locked -p ox-cli
node benchmarks/compare.mjs --runs 3 --out benchmarks/results.json 100000 250000 500000 1000000 2000000
node tools/readme-art.mjs --input benchmarks/results.json
```

## Importing from a database

<img src="../docs/readme/database-comparison.svg" width="100%" alt="Database import of 100,000 rows: OxRefine against OpenRefine on four backends.">

The same synthetic table in each database: 100,000 rows, integer ids, text labels and 1,031 NULL quantities. Each tool imported it three times in fresh processes, alternating which went first. Every completed import was checked value by value against an independent digest, outside the timer.

| Backend | Import, OxRefine / OpenRefine | Faster | Peak RAM, OxRefine / OpenRefine | Less memory |
|---|---|---|---|---|
| PostgreSQL 17.9 | 155 / 708 ms | **4.6×** | 22 / 322 MiB | **14.6×** |
| MySQL 8.4.13 | 114 / 1,294 ms | **11×** | 22 / 357 MiB | **16.2×** |
| MariaDB 11.4.13 | 118 / 753 ms | **6.4×** | 21 / 317 MiB | **15.1×** |
| SQLite 3.46.0 | 121 ms / not measured | | 23 MiB / not measured | |

<details>
<summary>What the times include</summary>

- **Different finish lines.** OxRefine's import includes the typed snapshot and saving the project to disk; OpenRefine's ends when its database controller registers the project in memory, before any autosave. OpenRefine's optional preview and query test are left out.
- **Excluded:** process startup, a one-row warm-up query, export and validation. Database services and the OS file cache stay warm, so this is not a cold-disk or remote-network test.
- **Memory** is the backend process's peak working set at the end of the import, runtime included, database servers excluded.
- **SQLite:** OpenRefine supports it, but this reference build's bundled JDBC driver failed to register (`No suitable driver`) in all three attempts, so there is no SQLite comparison.
- **The reference** is the same pinned OpenRefine 3.10.1 the conformance suites use; the report keeps its API version string, JVM details and the SHA-256 of the application and all four JDBC jars.

Set up the disposable databases with [the pinned engine versions and fixture SQL](../tests/database/reference-engines.json), then:

```powershell
node benchmarks/database-compare.mjs --config path/to/private-connections.json --sqlite path/to/fixture.sqlite --new path/to/ox.exe --runs 3 --out benchmarks/release-0.6.0/database-comparison.json
node tools/readme-art.mjs
```

Credentials never go into a report. [Paired report](release-0.6.0/database-comparison.json) · [OxRefine-only runs with reopen checks](release-0.6.0/database.json).
</details>

## Bigger than memory (experimental)

<img src="../docs/readme/spill.svg" width="100%" alt="50,000,001 rows and 20 columns under a 1 GiB Linux memory limit with no swap: 924 MiB resident peak, no out-of-memory kills.">

With `OXREFINE_CODE_BUDGET_MIB` set, row codes beyond the budget move to temporary mapped files (`OXREFINE_SPILL_DIR`). One run on Linux (WSL2) imported a 3.6 GB CSV of 50,000,001 rows under a 1 GiB cgroup limit with swap off, then transformed it, undid and redid every step, exported, restarted and reopened it, checking every row each time. Nothing was killed, and no spill file was left behind.

The budget covers row codes only: dictionaries, row indexes and operation scratch stay in RAM, so a column where every value is different still needs memory for each value. This run shows the low-cardinality workflow fits; it isn't a general memory bound or a speed comparison. [Report](out-of-core-baseline.json).

<details>
<summary>The run in numbers, and how to repeat it</summary>

| | |
|---|---|
| CSV | 3,644,506,112 bytes, 50,000,001 × 20 |
| Row codes | 4,000,000,080 bytes, of which 64 MiB in RAM at most |
| Spill files on disk at the peak | 4,200,161,280 bytes |
| OxRefine resident peak | 924 MiB |
| cgroup limit / swap | 1,024 MiB / 0 |
| cgroup peak | limit + 4 KiB (the kernel allows brief overshoot) |
| out-of-memory events | 0 |
| cells checked | 50,151 stored errors, 49,949,850 numbers |

An earlier attempt at the same limit was killed in the numeric range facet, which held two arrays as long as the table; the facet now counts and bins in separate passes. That failure stays in the report.

You need Linux with cgroup v2 and its memory controller, and a fresh directory on a Linux filesystem (not `/mnt/c`). The coordinator makes one private cgroup, starts the generator, verifier and server inside it, and cleans up after itself; it doesn't touch anything else on the host. The 50M run needs about 25 GB free.

```sh
# small smoke test: 100,003 rows, spilling forced with a 1 MiB budget
node benchmarks/out-of-core-cgroup.mjs --directory /var/tmp/ox-ooc-small --bin target/release/ox --code-budget-mib 1

# the 50M run, 1 GiB limit, no swap
node benchmarks/out-of-core-cgroup.mjs --directory /var/tmp/ox-ooc-50m --bin target/release/ox --rows 50000001 --code-budget-mib 64 --allow-large
```
</details>

## Regression gates

`check.mjs` runs two OxRefine builds on the same machine, alternating them, and fails when the new one is clearly worse: a step's fastest time over `base × 1.3 + 15 ms`, or median peak memory over `base × 1.1 + 8 MB`. A failed workload or a wrong result fails the check too. CI runs it on every push against the previous commit.

```powershell
cargo build --release --locked -p ox-cli
node benchmarks/check.mjs --base path/to/base/ox.exe --runs 3 250000 1000000
```

0.7.0 adds an embedded Python; CI's comparison of the 0.7.0 code with the last 0.6.0 build (same runner, people workflow) found the same step times and about 4 MB more peak memory: 54 against 50 MB at 250k rows, 149 against 145 MB at 1M. Python and Clojure expressions run per row, about 50 µs a row for Python.

The 0.6.0 checks against the official 0.5.3 binary:

| Check | Result |
|---|---|
| [People workflow, 250k and 1M rows](release-0.6.0/people-ci-250k.json) | passed |
| [People workflow, up to 2M rows](release-0.6.0/people-regression.json) | one step over: the 2M recipe took 58 ms against 31 ms (limit 55 ms); every output matched. Open follow-up. |
| [Cardinality and history, six datasets](release-0.6.0/cardinality-regression.json) | passed, 36 runs per build |
| [Database import, four backends](release-0.6.0/database.json) | all 12 runs passed their checks, reopen included |

## Cardinality and history

The people workflow repeats values, which is where OxRefine is strongest. This workload varies that on purpose: a synthetic `id, value, group` table where only `value` changes, with **low** (up to 16 distinct values), **mixed** (one per ten rows) and **high** (every value different) cardinality.

| Phase | Work |
|---|---|
| import | CSV import and the first save |
| value | trim and uppercase |
| merge | mass edits merge eight groups into two |
| history | 40 steps, each appending a different suffix |
| row | append `row.index`, so every value becomes unique |
| structure | add, rename, move and remove a column |
| undo / redo | jump to the first and last step |
| reopen | restart the server and open the saved project again |
| export | the final table to CSV |

0.6.0 medians:

| Rows | Cardinality | Distinct values | 40 history steps | Reopen after restart | Peak |
|---|---|---|---|---|---|
| 10,000 | low | 16 | 6 ms | 46 ms | 19 MiB |
| 10,000 | mixed | 1,000 | 19 ms | 61 ms | 25 MiB |
| 10,000 | high | 10,000 | 168 ms | 242 ms | 73 MiB |
| 100,000 | low | 16 | 26 ms | 441 ms | 97 MiB |
| 100,000 | mixed | 10,000 | 203 ms | 724 ms | 154 MiB |
| 100,000 | high | 100,000 | 2.73 s | 2.72 s | 623 MiB |

With unique values every history step keeps its own dictionary, so memory grows with rows × steps: start at 10,000 rows and watch RAM before going bigger. The peak covers the whole workflow, including the row-dependent step, so even the low case ends higher than its history phase alone.

<details>
<summary>How it's measured</summary>

- Every undone cell is compared with independently generated values through paged row requests, and the reopened export with an independent CSV by SHA-256 and line count.
- Times leave out data generation, the checks and the memory probe; HTTP time includes serialisation and each command's autosave.
- Peak memory is the OS high-water mark (Windows `PeakWorkingSet64`, Linux `VmHWM`; macOS samples RSS and can miss a short peak), the maximum over the two server processes, parent Node and file cache excluded.
- Reopen runs in a fresh server process on the same project library, so it loads from disk and replays the saved history; startup is timed separately and file caches stay warm.
- [cardinality-baseline.json](cardinality-baseline.json) is an earlier single run whose reopen didn't restart the process; keep it apart from these numbers.

```powershell
node benchmarks/check.mjs --profile cardinality --base path/to/base/ox.exe --new target/release/ox.exe --runs 3 --history 40 --out benchmarks/data/cardinality-comparison.json 10000 100000
node benchmarks/check.mjs --profile cardinality --record --runs 1 --history 40 --out benchmarks/data/cardinality-local.json 10000
```

Generated CSVs and exports stay in the ignored `benchmarks/data/`; each server gets a temporary project library, removed afterwards. Ports 3041 and 3042 must be free, and an `--out` directory must exist.
</details>
