# OxRefine

A fast, desktop alternative to [OpenRefine](https://openrefine.org) for cleaning messy tabular data. The engine is in Rust and the UI uses Tauri 2, React and shadcn/ui.

The workflow is OpenRefine's: facet, filter, cluster near-duplicates, transform, undo and redo. The engine underneath is different. Every column is **dictionary-encoded**, so each distinct value is stored once. Transforms, facets and clustering run per distinct value, not per row.

## OxRefine vs OpenRefine 3.10.1

Both tools ran the same workflow through their own local HTTP API: import, a text facet, fingerprint clustering, the same 5-step OpenRefine recipe, and CSV export. Each run used a fresh server process, one Windows laptop, and OpenRefine's default 1400 MB heap.

| rows | OxRefine | OpenRefine | faster | peak RAM (Ox / OR) |
|---|---|---|---|---|
| 100k | 0.18 s | 2.6 s | 14× | 23 / 415 MB |
| 250k | 0.21 s | 5.1 s | 24× | 44 / 781 MB |
| 500k | 0.36 s | 9.0 s | 25× | 76 / 971 MB |
| 1M | 0.70 s | 16.5 s | 24× | 141 / 1,360 MB |
| 2M | 1.3 s | out of memory | | 259 MB / OOM |

At every size every output cell is identical (8,000,008 of 8,000,008 at 1M). Per step at 1M: import 0.43 s against 2.1 s, the facet 9 ms against 0.6 s, clustering 17 ms against 2.0 s, the recipe 32 ms against 10.3 s, export 0.21 s against 1.6 s. `node benchmarks/report.mjs` turns the results into `benchmarks/report/index.html`.

There was one run per size, so timings under about 20 ms are noisy. To reproduce, put OpenRefine 3.10.1 in `../OpenRefine/openrefine-3.10.1`, then run:

```bash
cargo build --release
node benchmarks/compare.mjs
```

Set `OR_MEM=4096M` to give OpenRefine more heap. Results go to `benchmarks/results*.json`.

## OpenRefine compatibility

Recipes use the same JSON as OpenRefine's *Undo/Redo → Extract / Apply*, including `engineConfig` facets, so a history moves between the two tools in both directions. Whole projects move too: an OpenRefine export (`.openrefine.tar.gz`) opens with its undo history, and *Export → OpenRefine project* writes one that OpenRefine opens with OxRefine's history (`ox apply` reads and writes them as well). Behaviour is checked against fixtures generated from a real OpenRefine 3.10.1 (see [Tests](#tests)); every known difference is listed with its reason in a `known_failures.txt` next to the fixtures.

| Area | Cases matching OpenRefine |
|---|---|
| GREL expressions | 469 / 472 |
| Operations (incl. single-cell edits, stars, flags, transposing, fetching URLs, cross()) | 80 / 81 |
| Facets (list, range, timeline, text, custom, records mode) | 34 / 34 |
| Import options (CSV/TSV, fixed-width, Excel, ODS, JSON, XML, zip/tar/gzip archives) | 40 / 42 |
| Clipboard and web-address import (wizard defaults, archives, XML) | 17 / 17 |
| Messy-data corpus (encodings, line endings, ragged rows, huge cells…) | 26 / 27 |
| Exports (CSV/TSV, custom tabular, HTML, templating, Excel, ODS, SQL) | 14 / 15 |
| Clustering and GREL phonetic() (6 keyers, Levenshtein kNN, 9 encoders × 127 values) | 1148 / 1157 |
| Reconciliation (reconcile against a service, Recons in GREL, kept through later steps) | 16 / 16 |
| OpenRefine project archives, opened with their full history (and written back), reconciliation included | 103 / 103 |

## Run

```bash
# headless: replay an OpenRefine history on a file
cargo run --release -p ox-cli -- apply history.json input.csv -o output.csv

# desktop app
cd ui && npm install && npm run tauri dev

# browser mode (like OpenRefine): build the UI once, then `ox serve` has it built in
cd ui && npm install && npm run build && cd ..
cargo run --release -p ox-cli -- serve            # http://127.0.0.1:3030/

# on a server: other machines get in with the link it prints (it carries an access token);
# files are uploaded from the browser and exports download to it
ox serve --host 0.0.0.0 --data /srv/oxrefine
docker build -t oxrefine . && docker run -p 3030:3030 -v oxrefine-data:/data oxrefine

# working on the UI: Vite on http://localhost:5173 in front of `ox serve`
cd ui && npm run dev
```

## Layout

```
crates/ox-core   engine: table, import, facets, ops, GREL, export, clustering, history, jobs, JSON dispatch
crates/ox-cli    `ox apply` (batch) and `ox serve` (the browser app and its JSON API, UI built in)
ui/              React + shadcn/ui (virtualized grid, facets, history, transform and cluster dialogs)
ui/src-tauri     desktop shell: one `ox` command that forwards to ox-core::dispatch
benchmarks/      data generator, recipe, timing + peak-RAM script
```

## Tests

```bash
cargo test --release          # unit tests + every OpenRefine conformance suite
cd ui && npm run build        # UI typecheck and build
```

Each suite in `tests/<suite>/` has `cases.mjs` (what to try), `gen.mjs` (runs it through OpenRefine 3.10.1 and writes `fixtures.json`) and `known_failures.txt` (deliberate differences, each with a reason). A test fails on a new difference, and also when a known one starts passing, so the list only shrinks. Regenerating fixtures needs the OpenRefine download next to the repo (`tools/openrefine.mjs`); `node tests/corpus/make.mjs` rewrites the corpus files; `node tests/orproject/check-export.mjs` opens the archives OxRefine wrote during `cargo test` in OpenRefine and checks every undo step there. CI (`.github/workflows/ci.yml`) runs all of this on Windows, macOS and Linux.

## Not yet

Reconciliation (an imported project keeps its cell values but not their reconciliation data), the PPM clustering distance, RDF and bzip2 import, the scatterplot facet, and installers. The roadmap is in [ROADMAP.md](ROADMAP.md).
