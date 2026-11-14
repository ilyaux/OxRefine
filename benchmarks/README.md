# Reproducible performance workloads

`check.mjs` compares two OxRefine binaries on the same machine, alternating base/new runs. It keeps the existing people workload and its regression thresholds: fastest time per step must stay below `base * 1.3 + 15 ms`; median peak resident memory below `base * 1.1 + 8 MB`. A failing workload or wrong result aborts the check.

```powershell
node benchmarks/check.mjs --base path/to/base/ox.exe --runs 3 250000 1000000
node benchmarks/check.mjs --profile cardinality --base path/to/base/ox.exe --new target/release/ox.exe --runs 3 --history 40 --out benchmarks/data/cardinality-comparison.json 10000 100000
```

To record one build without claiming a regression comparison:

```powershell
node benchmarks/check.mjs --profile cardinality --record --runs 1 --history 40 --out benchmarks/data/cardinality-local.json 10000
node --test benchmarks/cardinality.test.mjs
```

Build release binaries first (`cargo build --release --locked -p ox-cli`). Generated CSVs and exports stay under ignored `benchmarks/data/`; each server gets a temporary project library which is removed after the run. Ports 3041 and 3042 must be free. JSON includes raw runs, binary SHA-256, CPU/OS/RAM and dataset hashes. An `--out` directory must already exist.

## Cardinality and history

The synthetic CSV has `id`, `value`, `group`, no external or personal data, and a fixed `cardinality-v1` generation rule. Only `value` changes cardinality: **low** has up to 16 distinct labels, **mixed** has `ceil(rows / 10)`, **high** has one label per row. IDs are always unique and group has up to eight values. This prevents confusing whole-file uniqueness with cardinality in the transformed column.

Each workload measures these phases separately:

| phase | work |
|---|---|
| import | CSV import and initial native save |
| value | trim and uppercase, depending only on the cell value |
| merge | mass edits merge eight group values into two; row codes change |
| history | append a different suffix per step (40 by default), retaining each dictionary |
| row | append `row.index` after the history test, turning repeated values into unique values |
| structure | add a derived column, rename and move a column, remove the derived one |
| undo / redo | jump to the original and final history steps; each command autosaves |
| save | rewrite native history and metadata with `goto(current)`; the original table is stored once |
| reopen | restart the backend on the same native library, then time loading the original table and replaying saved history; startup is separate |
| export | export the final table to CSV |

Every raw undone cell is compared with independently generated expected cells using paged row requests; this preserves the original surrounding spaces, which CSV export intentionally trims like OpenRefine. Every row of the reopened final export is compared with independently generated expected CSV using SHA-256 and line count. Only line-ending conventions are normalized in the CSV check. No sample-only correctness check. Timings exclude dataset generation, correctness checks and the OS memory probe. HTTP time includes serialization, scheduling and each command's autosave.

`phasePeakMB` records **cumulative** OS resident high-water marks, not memory allocated by one phase. Windows uses PeakWorkingSet64 and Linux VmHWM; macOS uses sampled current RSS and reports the maximum sample as `peak`, which can miss a transient peak. Reopen now uses a fresh backend process on the same private library: its startup is recorded separately, and only the ready server's `open_project` command is timed. This forces native file loading and history replay instead of selecting a project retained by the browser cache. Filesystem caches remain warm; this is not a cold-disk benchmark. Raw results retain the backend PIDs and both lifetime peaks. The whole-workflow peak is the maximum across the two processes, including paged validation requests.

The parent Node process, filesystem cache and OS overhead are excluded from server RAM. `savedBytes` is the native library size at the final cursor; the saved format replays operations, so a small library does not imply a small in-memory history. Shared snapshots reuse unchanged columns and row codes for one-to-one value changes. Code-changing edits still keep full column code vectors; unique values retain large dictionaries at every changed step. This workload does not measure spill-to-disk or operation beyond RAM.

Start with 10,000 rows and inspect available RAM before increasing sizes or history. The defaults intentionally stay small: unique-value histories can require memory proportional to rows times history length. The row-dependent transform runs after history so history dictionaries retain each dataset's distinct-value count. A completed run does not establish the largest safe input or an OpenRefine speed ratio. Do not extrapolate the people benchmark's speedup to these datasets.

CI currently runs the people regression workload. Cardinality measurements are opt-in and should be recorded on an otherwise quiet machine; the local JSON is a reproducible observation, not a CI guarantee. There is no OpenRefine timing comparison in this profile.

## Local baseline, 2026-11-14

[cardinality-baseline.json](cardinality-baseline.json) records one release run per dataset on Windows 11, AMD Ryzen AI 7 350, about 31 GiB RAM. The binary SHA-256, dataset hashes, every phase and correctness digests are included. Memory units in the JSON's `MB` fields are MiB (2^20 bytes).

This historical baseline predates the fresh-process reopen protocol. Its reopen timings come from close/open within one server process; keep those observations separate from the new restart measurements.

| rows | value cardinality | distinct values | 40 history steps | reopen | peak for whole workflow |
|---|---|---|---|---|---|
| 10,000 | low | 16 | 4 ms | 50 ms | 18 MiB |
| 10,000 | mixed | 1,000 | 26 ms | 79 ms | 24 MiB |
| 10,000 | high | 10,000 | 175 ms | 197 ms | 72 MiB |
| 100,000 | low | 16 | 28 ms | 1,222 ms | 97 MiB |
| 100,000 | mixed | 10,000 | 395 ms | 1,069 ms | 153 MiB |
| 100,000 | high | 100,000 | 3,971 ms | 4,500 ms | 623 MiB |

The whole-workflow peak includes the row-dependent transform, structural edits and reopen, after the cardinality-specific history phase. For example, the low-cardinality 100,000-row run had a cumulative HWM of 15 MiB after history and 97 MiB by the end. This shows why a repeated-value benchmark cannot predict unique-value history costs. These are single local observations; short timings are noisy, no base binary or OpenRefine was compared, and the RAM limit was not tested.

```powershell
node benchmarks/check.mjs --profile cardinality --record --runs 1 --history 40 --out benchmarks/cardinality-baseline.json 10000 100000
```

## Experimental row-code spilling

`out-of-core-data.mjs` physically writes every CSV row and records a SHA-256 manifest. The low-cardinality dataset has 20 columns: 16 fixed-width labels in `value`, 1,000 numeric strings plus a deliberate `BAD` every 997th row, eight groups, periodic/end checkpoints, and 16 additional columns with two to eight values each. There is no unique ID column that would silently make the low-cardinality dictionary grow with every row. The high variant makes every `value` distinct. The validation runners explicitly refuse large high-cardinality workloads; backend dictionaries remain resident.

`out-of-core.mjs` verifies every source/exported row against a deterministic oracle. It checks uppercase, numeric conversion with stored errors, merging mass edits, typed numeric/error counts, complete undo/redo, real in-flight import cancellation, failed-operation history, process restart/native reopen, unchanged source/native-original hashes, and private spill cleanup. JSON responses are capped at 8 MiB; table verification uses streamed CSV exports. A finished import which wins a cancellation race is recorded and removed as a known new test project, then retried; it never counts as cancellation coverage.

The separate code policy (`OXREFINE_CODE_BUDGET_MIB`, `OXREFINE_SPILL_DIR`) bounds tracked `CodeBuilder` allocations. `/api/storage_stats` exposes current/peak heap, separately counted writer buffers, logical mapped bytes and live spill-file counts. It does **not** bound resident dictionaries, flags, row indices, other code constructors or operation scratch. RSS includes mapped file pages and is reported separately. Total allocator heap is explicitly unknown in the report.

To reproduce the constrained workflow on Linux, build the release CLI and choose a **new directory on Linux ext4**, outside host mounts such as `/mnt/c`. The coordinator needs write access to a cgroup v2 hierarchy with the memory controller enabled. It creates one uniquely named private leaf, reads back its limits, moves each child into it **before exec**, and keeps the producer, verifier and backend in that same cgroup. The supervisor remains outside so it can record OOM failures and remove its own empty cgroup. It does not change host configuration, other processes, or global page caches.

```sh
# Small smoke: defaults to 100,003 rows. Force spill with a 1 MiB code budget.
node benchmarks/out-of-core-cgroup.mjs --directory /var/tmp/ox-ooc-small \
  --bin target/release/ox --code-budget-mib 1

# Physical 50,000,001 rows, with 1 GiB cgroup memory and no swap; requires explicit opt-in.
node benchmarks/out-of-core-cgroup.mjs --directory /var/tmp/ox-ooc-50m \
  --bin target/release/ox --rows 50000001 --code-budget-mib 64 --allow-large
```

The coordinator checks filesystem space before generating data and preserves inputs, reports and native projects for inspection. The recorded 50M workflow used about 16 GB at its active peak; leave at least 25 GB free in both the guest filesystem and its backing disk. Ports default to 3056. An existing result directory is refused. Smaller unconstrained runs can use `out-of-core-data.mjs generate` followed by `out-of-core.mjs --file ... --result-dir ...`; those commands alone do not install a process memory constraint.

The [kernel's cgroup memory-ownership rules](https://www.kernel.org/doc/html/latest/admin-guide/cgroup-v2.html#memory-ownership) matter: cached pages can stay charged to the cgroup which created them. Keeping generation and verification inside the same leaf avoids claiming a memory proof from externally charged warm input pages. `memory.max` is a cgroup charge limit, not a heap counter or an exact per-process RSS limit, and the kernel permits temporary overshoot. The report retains real RSS, cgroup peak/stat/events, configured limits and their scopes. Anonymous spill files are unlinked while open: directory walks cannot see their disk allocation, so Linux also measures their open file descriptors and allocated blocks.

### Observed 50M validation, 2026-11-14

[out-of-core-baseline.json](out-of-core-baseline.json) records a successful single development-candidate run on Linux/WSL2. The binary SHA identifies the measured artifact; its package version had not yet been bumped to 0.6.0. This is correctness and memory evidence for this bounded-cardinality workflow. Later phases overlapped a Windows release compilation, explicitly recorded in the JSON, so their timings are not quiet performance comparisons.

| observation | measured value |
|---|---|
| physical CSV rows × columns | 50,000,001 × 20 |
| source CSV | 3,644,506,112 bytes |
| logical original row codes | 4,000,000,080 bytes |
| tracked code-builder heap peak | 67,108,864 bytes (64 MiB) |
| active mapped code peak | 4,200,000,084 logical bytes |
| anonymous spill allocated disk peak | 4,200,161,280 bytes |
| backend OS resident peak | 968,953,856 bytes (924.07 MiB) |
| cgroup memory.max / swap.max | 1,073,741,824 / 0 bytes |
| actual whole-cgroup memory peak | 1,073,745,920 bytes (limit + 4 KiB) |
| cgroup OOM / kill / group-kill events | 0 / 0 / 0 |
| stored error / numeric cells | 50,151 / 49,949,850 |
| spill files after backend exit | 0 |

Every row matched after editing, undo and process restart; original CSV and native original-table SHA-256 remained unchanged. Cancellation stopped a new import after its first MiB and left no partial saved project. The previous 1 GiB attempt failed in the numeric range facet because it retained two numeric arrays proportional to row count. The successful implementation counts/min-maxes and builds histograms in separate passes; the failed attempt's OOM evidence remains in the baseline for comparison.

This run does not establish bounds for arbitrary dictionaries, row-dependent GREL, all facets/operations or other import formats. It does not test 50M unique values, account for host VM caches as backend heap, or establish a speed ratio against OpenRefine. The scripts deliberately retain these distinctions.
