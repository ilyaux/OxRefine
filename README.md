# OxRefine

A fast, desktop alternative to [OpenRefine](https://openrefine.org) for cleaning messy tabular data. The engine is in Rust and the UI uses Tauri 2, React and shadcn/ui.

The workflow is OpenRefine's: facet, filter, cluster near-duplicates, transform, undo and redo. The engine underneath is different. Every column is **dictionary-encoded**, so each distinct value is stored once. Transforms, facets and clustering run per distinct value, not per row.

## OxRefine vs OpenRefine 3.10.1

Both tools ran the same workflow through their own local HTTP API: import, a text facet, fingerprint clustering, the same 5-step OpenRefine recipe, and CSV export. Each run used a fresh server process, one Windows laptop, and OpenRefine's default 1400 MB heap.

| rows | OxRefine | OpenRefine | faster | peak RAM (Ox / OR) | identical output cells |
|---|---|---|---|---|---|
| 100k | 0.16 s | 2.2 s | 14× | 17 / 424 MB | 800,008 / 800,008 |
| 250k | 0.29 s | 4.9 s | 17× | 36 / 758 MB | 2,000,008 / 2,000,008 |
| 500k | 0.69 s | 8.1 s | 12× | 65 / 976 MB | 4,000,008 / 4,000,008 |
| 1M | 1.9 s | 17.3 s | 8.9× | 122 / 1,361 MB | 8,000,008 / 8,000,008 |
| 2M | 2.6 s | out of memory | | 235 MB / OOM | |
| 2M, OpenRefine on a 4 GB heap | 2.6 s | 32.3 s | 13× | 235 / 3,433 MB | 16,000,008 / 16,000,008 |

Interactive steps show the biggest gaps. At 2M rows a facet takes 8 ms against 1.25 s, clustering 16 ms against 3.8 s, and the recipe 35 ms against 20.3 s. Import shows the smallest gap, 1.6×.

There was one run per size, so timings under about 20 ms are noisy. To reproduce, put OpenRefine 3.10.1 in `../OpenRefine/openrefine-3.10.1`, then run:

```bash
cargo build --release
node benchmarks/compare.mjs
```

Set `OR_MEM=4096M` to give OpenRefine more heap. Results go to `benchmarks/results*.json`.

## OpenRefine compatibility

Recipes use the same JSON as OpenRefine's *Undo/Redo → Extract / Apply*, including `engineConfig` facets, so a history moves between the two tools in both directions.

- Operations: `core/text-transform`, `core/mass-edit`, `core/column-addition`, `core/column-rename`, `core/column-removal`
- Facets: list (text) facets and text filters (substring or regex), with invert; rows mode only
- GREL subset: `value`, `null`, string literals, and chains of `trim`, `strip`, `toLowercase`, `toUppercase`, `toTitlecase`, `toString`, `fingerprint`, `replace("a" | /re/, "b")`. This covers OpenRefine's *Common transforms* menu. Unsupported expressions fail with a clear error and are never silently skipped.
- Clustering: key collision using `fingerprint` and `ngram-fingerprint`

## Run

```bash
# headless: replay an OpenRefine history on a file
cargo run --release -p ox-cli -- apply history.json input.csv -o output.csv

# desktop app
cd ui && npm install && npm run tauri dev

# browser mode (like OpenRefine): engine API + Vite UI on http://localhost:5173
cargo run --release -p ox-cli -- serve data.csv
cd ui && npm run dev
```

## Layout

```
crates/ox-core   engine: table, facets (engine.rs), ops, GREL subset, clustering, history, JSON dispatch
crates/ox-cli    `ox apply` (batch) and `ox serve` (local JSON API)
ui/              React + shadcn/ui (virtualized grid, facets, history, transform and cluster dialogs)
ui/src-tauri     desktop shell: one `ox` command that forwards to ox-core::dispatch
benchmarks/      data generator, recipe, timing + peak-RAM script
```

## Not yet

Excel/JSONL import, numeric/date facets, records mode, kNN clustering, reconciliation, the full GREL language, single-cell edits, and scrolling past about 1M rows in the grid (the browser's element-height limit).
