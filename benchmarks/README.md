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
| reopen | close the project, read the native original table and replay saved history |
| export | export the final table to CSV |

Every raw undone cell is compared with independently generated expected cells using paged row requests; this preserves the original surrounding spaces, which CSV export intentionally trims like OpenRefine. Every row of the reopened final export is compared with independently generated expected CSV using SHA-256 and line count. Only line-ending conventions are normalized in the CSV check. No sample-only correctness check. Timings exclude dataset generation, correctness checks and the OS memory probe. HTTP time includes serialization, scheduling and each command's autosave.

`phasePeakMB` is the process's **cumulative** OS resident high-water mark after that phase, not memory allocated by that phase. Subsequent high-water marks also include the paged requests used to validate undo. Windows uses PeakWorkingSet64 and Linux VmHWM; macOS uses sampled current RSS and reports the maximum sample as `peak`, which can miss a transient peak. Reopen is in the same server process after dropping the active project, so its high-water mark can include earlier history. It does perform a disk read and replay; this is not a fresh-process startup benchmark.

The parent Node process, filesystem cache and OS overhead are excluded from server RAM. `savedBytes` is the native library size at the final cursor; the saved format replays operations, so a small library does not imply a small in-memory history. Shared snapshots reuse unchanged columns and row codes for one-to-one value changes. Code-changing edits still keep full column code vectors; unique values retain large dictionaries at every changed step. This workload does not measure spill-to-disk or operation beyond RAM.

Start with 10,000 rows and inspect available RAM before increasing sizes or history. The defaults intentionally stay small: unique-value histories can require memory proportional to rows times history length. The row-dependent transform runs after history so history dictionaries retain each dataset's distinct-value count. A completed run does not establish the largest safe input or an OpenRefine speed ratio. Do not extrapolate the people benchmark's speedup to these datasets.

CI currently runs the people regression workload. Cardinality measurements are opt-in and should be recorded on an otherwise quiet machine; the local JSON is a reproducible observation, not a CI guarantee. There is no OpenRefine timing comparison in this profile.

## Local baseline, 2026-11-14

[cardinality-baseline.json](cardinality-baseline.json) records one release run per dataset on Windows 11, AMD Ryzen AI 7 350, about 31 GiB RAM. The binary SHA-256, dataset hashes, every phase and correctness digests are included. Memory units in the JSON's `MB` fields are MiB (2^20 bytes).

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
