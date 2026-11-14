# OxRefine 0.6.0 release candidate

Preparation snapshot: 2026-11-14. **0.6.0 is the candidate version.** All package manifests and lockfiles agree. No tag, upload, workflow dispatch or publication was performed during this preparation.

## Scope and source state

The published baseline is [v0.5.3](https://github.com/ilyaux/OxRefine/releases/tag/v0.5.3), commit `0912b4f9c4a7fe6f5c8346cd28425c06638366c7`. Production runtime source is committed as [`be54731`](https://github.com/ilyaux/OxRefine/commit/be54731a85ce116521af306e045c8e4159f5931b). The subsequent [`e62356b`](https://github.com/ilyaux/OxRefine/commit/e62356bc927b6be5ff44a55ced7b230af5a21421) changes test/harness configuration without changing that runtime. The release code candidate is [`7165353`](https://github.com/ilyaux/OxRefine/commit/71653533ce990e7c703c7cee454f268090eccc4d), including isolated MARC test fixtures. Its [exact-SHA CI](https://github.com/ilyaux/OxRefine/actions/runs/43634034351) must be green before packaging. Later documentation/report commits do not change the runtime; the release tag remains pinned to the reviewed code candidate.

Already committed after v0.5.3:

| Commit | Changes |
|---|---|
| `343ac73` | Preserve opaque extension data when moving OpenRefine projects (#454). |
| `8eb8746` | Recipe validation, diagnostics, job stages and cardinality/history benchmarks (#456, #458, #368). |
| `9453a60` | Independent browser project selection and views for distinct transform/reconciliation values (#455, #369, #370). |
| `0152dcc` | MCP adapter and project/import/facet workflow improvements. |
| `1e16b79` | JSON paths/import behavior, customized exports and facet interactions. |
| `c252d28` | Compatible language detection and Wikimedia Commons category import (#352, #355). |

The committed import batch adds MARC/MARC-8 (#271), wikitext tables (#272), PC-Axis (#346), their UI, actual OpenRefine reference fixtures and roundtrip tests.

The candidate also implements native SQLite, PostgreSQL, MySQL and MariaDB import (#344), and opt-in experimental row-code spilling (#336). The implemented scope and local evidence are recorded below. Feature validation does not replace the frozen-candidate release checks, packaged OS checks or same-SHA CI gate.

## Implemented scope and local evidence

**Database import:** test a connection, browse its schema, run one guarded read-only query, page through the result, and create a project from that immutable snapshot without a second query. Profiles persist connection settings without passwords; SQL and credentials are absent from project metadata. SQL NULL and empty text remain distinct, decimals and nested JSON numbers retain precision, binary values use `base64:` text, and dates/booleans remain text. A read-only database account is required; the SELECT guard is not a sandbox for privileged or custom server functions. Result sizes, query timeouts and temporary snapshot retention are bounded; this is not a direct editor for source tables.

The final targeted Windows database run passed all five integration tests against actual SQLite, PostgreSQL, MySQL and MariaDB fixtures, plus four unit tests. The checks cover exact scalar values, unsigned `TINYINT(1)`, native BIT bytes, SQL guards, timeout/cancellation with no lingering server query, saved-profile revisions, and save/reopen from a prepared snapshot after the source file is removed. The host-dispatch regression forces a zero code budget and verifies mapped row codes without a manually installed scope. The four database import flows also passed the local browser UI checks. An earlier full run exposed a MariaDB timeout classification race. The new deterministic live regression confirms that MariaDB 1969/70100 and MySQL 3024/HY000 are timeouts while an own-query interruption (1317) is not. Job cancellation still takes priority. Logs: `target/kanzo-344-db-focused-release-unit.log` and `target/kanzo-344-db-focused-release-integration.log`. These results do not establish packaged desktop or upgrade behavior.

**Experimental large-table storage:** set `OXREFINE_CODE_BUDGET_MIB` before launch to limit tracked row-code builder arrays; `0` forces nonempty builders to private mapped spill files. `OXREFINE_SPILL_DIR` selects their directory. CSV uses a serial streaming parser with a finite budget, and saved OXT codes load without a second full-file buffer. Browser, CLI and desktop dispatch install the Store budget on the executing worker; desktop commands use a blocking worker so the database runtime does not nest inside the async command runtime.

The code budget excludes distinct-value dictionaries, flags, row indexes, reconciliation metadata, document trees, other code constructors and operation scratch. Writer buffers have separate counters, and the OS manages mapped pages. This is neither a total-heap limit nor a universal process-memory limit. High-cardinality inputs and untested operations can still need substantial RAM.

The completed 50M+ correctness proof used **50,000,001 rows, 20 columns, low cardinality and 32-byte generated values** on Linux/WSL2. The input was 3,644,506,112 bytes, with at least 4,000,000,080 bytes of row codes, a 64 MiB tracked-code budget and an independently verified **1 GiB cgroup-v2 limit with swap disabled**. The kernel reported a final cgroup peak of **1 GiB + 4 KiB**, including charged file cache; there were no OOM/OOM-kill events. The maximum observed backend RSS was 968,953,856 bytes. These are distinct measurements, not an allocator-heap figure.

That run passed import, uppercase transformation, stored error cells, value merging, typed error counts, full CSV validation, undo/redo, cancellation without publishing a partial project, process restart/reopen, unchanged source/original hashes and spill-file cleanup. Reopened exports matched the transformed export hash. It proves that workload; it does not promise every 50M-row dataset fits the same limit. Concurrent compilation overlapped part of the run, so its phase timings are not release performance claims. The report identifies its own native binary hash; distinguish that proof binary from the frozen Windows benchmark binary below and the final installer artifacts.

The reviewed [proof report](../benchmarks/out-of-core-baseline.json) and [measurement/reproduction instructions](../benchmarks/README.md) are included with the candidate. The original local artifacts are also retained outside Git:

- `target/ooc-benchmark-probe/rangefix-full-low-50000001-c879c881-result.json`
- `target/ooc-benchmark-probe/rangefix-full-low-50000001-c879c881-isolation.json`
- `target/ooc-benchmark-probe/rangefix-full-low-50000001-c879c881-context.json`

The committed report records `functional-passed`, verified tracked-code budget and a cgroup limit smaller than both the input and minimum code volume. It explicitly leaves `totalHeapBudgetVerified` false. Ignored local paths are supporting artifacts, not the release reader's evidence link.

## Release gates

The previous published release has [green CI](https://github.com/ilyaux/OxRefine/actions/runs/43252435566) and a [successful package workflow](https://github.com/ilyaux/OxRefine/actions/runs/43252434871). Those runs cover the 0.5.3 commit only.

The release workflow now has a read-only preflight before any desktop or CLI assets are created. It accepts stable `vMAJOR.MINOR.PATCH` tags, checks all ten version entries, requires nonempty `docs/releases/<tag>.md` notes headed with the matching version, rejects replacing an already published release, and requires the latest `main` push CI run for the exact candidate SHA to pass all three engine OS jobs, the UI build and the performance comparison. It waits up to 30 minutes for an active or not-yet-visible run, then fails. CI job names are deliberately explicit in `tools/release-preflight.mjs`; keep them in sync with `ci.yml`. The reviewed notes pass through `GITHUB_OUTPUT` into the existing desktop release-body input without shell interpolation.

This closes two observed gaps: manual dispatch previously accepted a branch name as a release tag, and release builds were independent of CI. For example, the [v0.5.2 package build](https://github.com/ilyaux/OxRefine/actions/runs/43225397437) succeeded while its [performance CI job failed](https://github.com/ilyaux/OxRefine/actions/runs/43225397408).

The [`e62356b` CI run](https://github.com/ilyaux/OxRefine/actions/runs/43632064818) completed with four of five required jobs green: Windows and Linux engines, UI and performance. macOS failed two MARC tests using non-reserved timestamp-based temporary names. That helper permits collisions; the completed log did not preserve the underlying path/error, so the specific cause remains unconfirmed. The isolated-directory fix and better diagnostics passed all five MARC tests locally and are being checked in the exact-SHA run above. The production macOS build itself passed. Every required job must still be green; these four successes do not bypass that requirement.

CI tests now use `[profile.ci]`, inheriting release optimization with LTO disabled and 16 codegen units to avoid repeated long test-binary links. Production builds retain the existing `--release` flags and LTO. Test cases are unchanged, and the full production-release suites were separately run locally on Windows and Linux.

Before a candidate can be published:

- [x] Commit the production import, database, spill, UI and reference/test implementation as `be54731`; keep generated datasets, local project libraries and credentials out of Git.
- [ ] Commit the final isolated MARC fixture fix and reviewed docs/evidence, and identify that release candidate SHA.
- [x] The three package Cargo manifests, `ui/package.json`, both root-version fields in `ui/package-lock.json`, `ui/src-tauri/tauri.conf.json`, and the three workspace package entries in `Cargo.lock` all report 0.6.0. The updater public key is unchanged.
- [ ] Run the local checks below against the frozen candidate, then obtain green CI for that same SHA on `main`.
- [x] Final Windows and Linux production-release suites each passed 167 tests with 0 failures and 2 intentionally ignored tests. Logs: `target/kanzo-336-bulk-windows-full.log` and `target/kanzo-336-bulk-linux-full.log`. The live database timeout regression, final Windows CLI build and final Tauri `cargo check` passed.
- [x] Complete the final OpenRefine, database and cardinality reports, retaining the real expanded-people regression described below. The required 250K/1M people gate passed; the expanded 2M recipe phase did not.
- [x] Validate native CLI library upgrade from official 0.5.3 to the frozen 0.6.0 candidate across all seven history states, types, reconciliation, flags, hashes and restart/continued editing. Manifest: `target/upgrade-060/candidate/manifest.json`. This is not installer/updater E2E evidence.
- [x] Replace the placeholder release body with version-matched notes from [docs/releases/v0.6.0.md](releases/v0.6.0.md). Missing, empty or wrong-version notes fail preflight.
- [ ] Review the draft desktop and CLI artifacts and `latest.json`, and approve the canonical notes against those artifacts.
- [ ] Inspect the actual archives and installed desktop resources for the full license/credit inventory below.
- [ ] Complete the packaged OS checks below. A successful cross-build is not a launch or upgrade test.

## Local validation

Run from the repository root. These commands build and test locally; they do not create a release.

```powershell
git diff --check
node --test tests/release/*.test.mjs benchmarks/*.test.mjs tools/*.test.mjs
cargo test --release --locked
Push-Location ui
npm ci
node --test tests/*.test.mjs
npm run build
Pop-Location
cargo build --release --locked -p ox-cli
```

The latest GitHub UI job passed 128 of 130 Node tests with two explicit integration skips, then passed `tsc -b && vite build`. This includes UI regressions, benchmark/oracle validation, SVG generation and release gates. The final native desktop crate passed `cargo check --locked -p oxrefine` at version 0.6.0. These are source/build checks, not installer tests.

The frozen Windows benchmark executable is `target/benchmark-candidate-v0.6.0/bulk/ox.exe`, SHA-256 `31e9e0d1a177f74b895475dda821928aeecb5b649fe4ead3b0676b7a1f158cd9`, built from the committed production runtime. It is not rebuilt during measurement and is not a published GitHub artifact. The native CLI upgrade manifest identifies this same binary and records all seven history states matching, no rewrite on first open, preserved values/types/flags/reconciliation/history, successful branch replacement and restart, unchanged baseline/source hashes, and stopped test servers.

The import fuzzer now includes actual MARC, PC-Axis and inline wikitext reference inputs, with explicit targets for all three formats. Default wikitext fuzz options do not resolve links. A 3,000-iteration run (seed 1592652007) passed under a verified 512 MiB Windows process-commit limit with zero panics and no input above two seconds. Both discovered wikitext failures have retained regression inputs and bounded parser guards. Run the longer pre-release mutation session in a fresh shell and retain its seed/results:

```powershell
$env:FUZZ_ITERS = "1000000"
$env:FUZZ_SEED = "7"
cargo test --release --locked -p ox-core --test fuzz -- --nocapture
```

For performance, use a native 0.5.3 base binary and the candidate on the same quiet machine. Replace the example base path with the unpacked binary. Ports 3041 and 3042 must be free; create the output directory first.

```powershell
New-Item -ItemType Directory -Force benchmarks/data | Out-Null
node benchmarks/check.mjs --base target/release-baseline/ox.exe --new target/release/ox.exe --runs 3 --out benchmarks/data/release-people.json 250000 1000000
node benchmarks/check.mjs --profile cardinality --base target/release-baseline/ox.exe --new target/release/ox.exe --runs 3 --history 40 --out benchmarks/data/release-cardinality.json 10000 100000
```

The people gate uses `base * 1.3 + 15 ms` for time and `base * 1.1 + 8 MiB` for memory. Cardinality/history is opt-in rather than part of CI. Neither command proves spill-to-disk behavior or the largest safe input. See [measurement details](../benchmarks/README.md).

Final local reports use symmetric `Connection: close` transport for both engines. A bounded diagnostic found roughly 40 ms delayed response-body delivery with keepalive while server work remained about 1–2 ms; the transport control removes that effect without changing runtime code, measured work or gate thresholds. Raw earlier evidence remains in [pre-fix-af937201](../benchmarks/release-0.6.0/pre-fix-af937201/) and [keepalive-31e9e0d1](../benchmarks/release-0.6.0/keepalive-31e9e0d1/).

- [OpenRefine comparison](../benchmarks/release-0.6.0/openrefine-1400M.json): on the measured 1M-row workflow, median per-run totals were 0.658 s for OxRefine and 15.021 s for OpenRefine 3.10.1, a 22.8× ratio; measured process peaks were 116 versus 1420 MiB. This describes that workload and configured JVM, not a universal speed or memory ratio.
- [Database v2 report](../benchmarks/release-0.6.0/database.json): all 12 runs passed (four engines × three repetitions, 100K rows), including complete validation and a real backend restart before reopening. Database services and filesystem caches remained warm; server memory is excluded from OxRefine process memory.
- [Cardinality/history report](../benchmarks/release-0.6.0/cardinality-regression.json): complete prescribed repetitions and correctness checks, regression command exit 0.
- [Expanded people report](../benchmarks/release-0.6.0/people-regression.json): correctness passed, but the 2M-row recipe phase remains about 27 ms slower and the unchanged performance gate exited 1. The required 250K/1M sizes passed, including the separate [250K report](../benchmarks/release-0.6.0/people-ci-250k.json) and the GitHub performance job. This residual result is retained; it is not labelled a clean expanded-suite pass.

The [release benchmark plan](../benchmarks/release-0.6.0/PLAN.md), [baseline provenance](../benchmarks/release-0.6.0/baseline.json) and [HTML report](../benchmarks/release-0.6.0/report.html) accompany the raw measurements. Final same-SHA macOS CI confirmation, packaged OS launches, installer upgrades, artifact inventories and updater verification remain pending.

## License packaging

The desktop `bundle.resources` map and CLI packaging now share an explicit inventory: root LICENSE/NOTICE, full Apache texts for Commons Codec and language-detector, language credits and upstream README, MARC mapping provenance, OpenRefine's BSD text, parse_wiki_text's exact license and the Geist/Geist Mono SIL font licenses. The CLI copier fails on missing, empty or out-of-bounds resources and verifies the copied bytes. See [the inventory and source hashes](../licenses/README.md).

The locked NPM dependencies must be installed before the real inventory is copied, as they provide the two font license files. A local copy check can be reproduced without building installers:

```powershell
node tools/package-licenses.mjs target/release-license-proof
tar -czf target/release-license-proof.tar.gz -C target/release-license-proof .
tar -tf target/release-license-proof.tar.gz
```

That proof archive contains license texts, not a candidate binary or installer. Before publication, retain file-list evidence from the actual CLI ZIP/tar archives and installed desktop resources; check every destination in `bundle.resources`, including both font licenses. For example, use `7z l` for the Windows CLI ZIP, `tar -tf` for CLI/app tar archives and `dpkg-deb --contents`/`rpm -qlp` for Linux packages. macOS app resources are under `Contents/Resources`; inspect the installed Windows resource directory too.

One provenance limitation remains explicit: the vendored Metaphone 3 crate declares MIT but its published source archive contains no standalone LICENSE file. The existing source attribution is retained; this preparation does not invent an upstream copyright statement or claim an exhaustive dependency-license audit.

## Packaged OS and updater checks

| Target | Required evidence |
|---|---|
| Windows x64 | Launch NSIS/MSI installation, import/preview/save/restart a project, open external links, and upgrade an isolated 0.5.3 library without losing data/history/reconciliation. Unpack the CLI archive and exercise `apply` and `serve` with its embedded UI. |
| macOS Apple silicon and Intel | Launch the universal app on both architectures; check picker/import/save/restart and manual upgrade. Exercise each native CLI archive. The Intel cross-build alone does not prove Intel runtime behavior. |
| Linux x64 | Launch AppImage and verify the DEB/RPM package dependencies on their supported distributions. Check import/save/restart and the unpacked CLI browser app. |

Keep the existing unsigned-installer notice: Windows Authenticode and macOS notarization are separate from updater signing and are not configured by this workflow.

The current public 0.5.3 release has 18 assets. Its updater JSON reports 0.5.3 with 11 platform keys, signatures and URLs for the matching assets; the configured public key is unchanged from that tag. This was a metadata inspection, not cryptographic verification or an installation test. No private signing key or secret value was read.

For the candidate, verify updater signatures against the existing public key, the JSON version and every asset URL before publication. The Windows default feed points to MSI; Linux has AppImage/DEB/RPM entries; both macOS architectures point to the universal app archive. After publication, check the public `releases/latest/download/latest.json` channel and an actual signed update from 0.5.3. Draft artifacts are not available through that public latest channel.

## Draft GitHub release body

The canonical English body is [docs/releases/v0.6.0.md](releases/v0.6.0.md). Edit that file only; the release workflow loads the notes matching its tag. It covers the implemented import, database and experimental spill work, plus the preceding commits, and links the reviewed 50M proof report. Local release suites and benchmark reports are complete with the residual 2M recipe finding above; final same-SHA CI, packaged OS and updater results remain pending. Add download links or artifact-availability claims only after the artifacts exist.
