<!--
Keywords: OpenRefine alternative, Google Refine alternative, data cleaning tool, data cleansing, data wrangling,
data preparation, data quality, messy data, clean CSV, clean Excel, deduplicate data, remove duplicates,
fuzzy matching, fuzzy deduplication, clustering, record linkage, entity resolution, reconciliation, Wikidata
reconciliation, GREL, faceted browsing, data transformation, ETL, spreadsheet cleanup, large CSV files,
million rows, fast, Rust, Tauri, desktop app, self-hosted, open source, Trifacta alternative, Talend alternative
-->
<p align="center">
  <img src="docs/readme/hero.svg" width="100%" alt="OxRefine: OpenRefine's way of cleaning messy data, rebuilt in Rust. The same million-row workflow takes 0.60 s in OxRefine and 16.8 s in OpenRefine.">
</p>

<p align="center">
  <a href="https://github.com/ilyaux/OxRefine/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/ilyaux/OxRefine?label=download&color=111113"></a>
  <a href="https://github.com/ilyaux/OxRefine/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ilyaux/OxRefine/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://ilyaux.github.io/OxRefine/"><img alt="Documentation" src="https://img.shields.io/badge/docs-ilyaux.github.io%2FOxRefine-1D4ED8"></a>
  <img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-111113">
  <img alt="Engine: Rust" src="https://img.shields.io/badge/engine-Rust-B7410E">
  <img alt="Desktop: Tauri 2" src="https://img.shields.io/badge/desktop-Tauri%202-24C8DB">
  <img alt="Checks matching OpenRefine 3.10.1" src="https://img.shields.io/badge/matches%20OpenRefine%203.10.1-2122%20of%202136%20checks-brightgreen">
</p>

<p align="center">
  <b><a href="https://github.com/ilyaux/OxRefine/releases/latest">Download</a></b>
  &nbsp;·&nbsp; <a href="https://ilyaux.github.io/OxRefine/">Documentation</a>
  &nbsp;·&nbsp; <a href="https://ilyaux.github.io/OxRefine/moving-over.html">Coming from OpenRefine</a>
  &nbsp;·&nbsp; <a href="https://ilyaux.github.io/OxRefine/grel.html">GREL reference</a>
  &nbsp;·&nbsp; <a href="#benchmark">Benchmark</a>
</p>

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/src/images/screenshot-dark.png">
  <img alt="OxRefine with 100,000 messy rows: text facets on country and city show the spellings to clean up" src="docs/src/images/screenshot.png">
</picture>

## Why

OxRefine is a free, open-source **OpenRefine alternative** for data cleaning and data wrangling: deduplicate and fix messy CSV, Excel and JSON files, cluster near-duplicate spellings, transform columns with GREL, and reconcile values against Wikidata.

[OpenRefine](https://openrefine.org) is the standard tool for cleaning messy tabular data, and it slows down and runs out of memory as tables grow. OxRefine keeps its workflow, its expression language and its file formats, and replaces the engine: every column is **dictionary-encoded**, so each distinct value is stored once, and transforms, facets and clustering run once per distinct value instead of once per row.

|  |  |
|---|---|
| **About 20× faster, 12–22× less memory** | A million-row workflow takes 0.6 s instead of 16.8 s. At 2 million rows OpenRefine runs out of memory on its default heap; OxRefine needs 219 MB. |
| **The same results** | Behaviour is pinned to fixtures generated from a real OpenRefine 3.10.1: 2,122 of 2,136 checks match, and each known difference is written down with its reason. |
| **Your work moves both ways** | OpenRefine recipes (*Undo/Redo → Extract / Apply*) run unchanged, and whole OpenRefine projects open with their undo history. OxRefine writes projects that OpenRefine opens with every step intact. |
| **Desktop, browser, server or command line** | A native desktop app, a browser app (`ox serve`, like OpenRefine), a server or Docker container behind an access token, or headless with `ox apply`. |

## Benchmark

The same workflow ran through each tool's own HTTP API: CSV import, a text facet, fingerprint clustering, a 5-step OpenRefine recipe and CSV export. Each run used a fresh process on one Windows laptop, and OpenRefine had its default 1,400 MB heap. Every output cell is identical at every size (8,000,008 of 8,000,008 at a million rows).

<img src="docs/readme/bench-time.svg" width="100%" alt="Whole-workflow time: 0.11 s against 2.1 s at 100k rows, 0.60 s against 16.8 s at 1M rows; at 2M rows OxRefine takes 1.2 s and OpenRefine runs out of memory.">

The gap is widest where OpenRefine works row by row and OxRefine works once per distinct value:

<img src="docs/readme/bench-steps.svg" width="100%" alt="Each step at 1M rows: import 5×, text facet 62×, clustering 121×, a 5-operation recipe 377×, export 8× faster.">

<img src="docs/readme/bench-memory.svg" width="100%" alt="Peak memory: OxRefine grows from 23 to 219 MB between 100k and 2M rows; OpenRefine reaches 1,359 MB at 1M rows and runs out of its 1,400 MB heap at 2M.">

<details>
<summary>The numbers</summary>

| Rows | OxRefine | OpenRefine | Faster | Peak RAM (OxRefine / OpenRefine) |
|---|---|---|---|---|
| 100,000 | 0.11 s | 2.1 s | 19× | 23 / 419 MB |
| 250,000 | 0.20 s | 3.9 s | 19× | 37 / 797 MB |
| 500,000 | 0.37 s | 9.5 s | 26× | 63 / 972 MB |
| 1,000,000 | 0.60 s | 16.8 s | 28× | 116 / 1,359 MB |
| 2,000,000 | 1.2 s | out of memory | | 219 MB / out of memory |

Per step at a million rows:

| Step | OxRefine | OpenRefine |
|---|---|---|
| Import | 0.35 s | 1.8 s |
| Text facet | 10 ms | 0.6 s |
| Clustering | 19 ms | 2.2 s |
| Recipe (5 operations) | 28 ms | 10.6 s |
| Export | 0.20 s | 1.6 s |

</details>

<details>
<summary>Reproducing it</summary>

Put OpenRefine 3.10.1 in `../OpenRefine/openrefine-3.10.1`, then:

```bash
cargo build --release
node benchmarks/compare.mjs            # writes benchmarks/results.json
node benchmarks/report.mjs             # turns it into benchmarks/report/index.html
node tools/readme-assets.mjs           # redraws the charts in this README
```

Set `OR_MEM=4096M` to give OpenRefine more heap. There is one run per size, so steps under about 20 ms are noisy.

`node benchmarks/check.mjs --base <an older ox binary>` runs two builds through the same steps on the same machine and fails when a step is clearly slower (over 1.3× and 15 ms) or uses clearly more memory (over 1.1× and 8 MB). CI runs it on every push and pull request against the previous build.
</details>

## What it does

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/src/images/cluster-dark.png">
  <img alt="Clustering the city column: spellings of São Paulo, Köln and Kraków grouped, with row counts, a bar for each spelling and the value to merge into" src="docs/src/images/cluster.png">
</picture>

| | |
|---|---|
| **Import** | CSV and TSV, fixed-width text, Excel (`.xlsx`, `.xls`), OpenDocument (`.ods`), JSON and JSON Lines, XML, and plain lines, from files, folders, zip/tar/gzip/bzip2 archives, the clipboard or a web address. A preview wizard with OpenRefine's options. |
| **Explore** | Text, numeric, timeline and custom facets, text filters, records mode, and a grid that stays fast at millions of rows. |
| **Clean** | GREL expressions, common transforms, mass edits, splitting and joining columns and multi-valued cells, fill and blank down, transposing, fetching URLs, and `cross()` between projects. |
| **Cluster** | Key collision (fingerprint, n-gram, metaphone and the other phonetic keyers) and nearest-neighbour (Levenshtein) clustering, with filters on cluster size, row count and length. |
| **Reconcile** | Match a column against any W3C Reconciliation service, such as Wikidata or your own: judge candidates cell by cell or in bulk, search the service, preview entities, and add columns from the matched items. |
| **Keep track** | Full undo and redo, saved projects, recipes, and progress with cancel for long steps. |
| **Export** | CSV and TSV, custom tabular, HTML, templating, Excel, OpenDocument, SQL, and OpenRefine projects. |

## OpenRefine compatibility

Each area is checked against fixtures produced by running the same cases through OpenRefine 3.10.1. Every difference that remains is listed, with its reason, in a `known_failures.txt` next to the fixtures.

<img src="docs/readme/compat.svg" width="100%" alt="2,122 of 2,136 checks match OpenRefine 3.10.1, 99.2%, across ten suites.">

<details>
<summary>What each suite covers</summary>

| Area | Cases matching OpenRefine |
|---|---|
| GREL expressions | 559 / 561 |
| Operations, including single-cell edits, stars, flags, transposing, fetching URLs and `cross()` | 83 / 84 |
| Facets: list, range, timeline, text, custom and records mode | 39 / 39 |
| Import options: CSV/TSV, fixed-width, Excel, ODS, JSON, XML, and zip/tar/gzip/bzip2 archives | 43 / 45 |
| Import from the clipboard and web addresses | 18 / 18 |
| Messy-data corpus: encodings, line endings, ragged rows, huge cells | 26 / 27 |
| Exports: CSV/TSV, custom tabular, HTML, templating, Excel (.xlsx and .xls), ODS, SQL | 16 / 17 |
| Clustering and GREL `phonetic()`: 6 keyers, Levenshtein kNN, 9 encoders × 127 values | 1,152 / 1,159 |
| Reconciliation: reconciling, judging, the operations on matches, data extension | 49 / 49 |
| OpenRefine project archives opened with their full history and written back, metadata included | 137 / 137 |

</details>

**[Documentation](https://ilyaux.github.io/OxRefine/)**: install, [where everything is if you know OpenRefine](https://ilyaux.github.io/OxRefine/feature-map.html), [moving your projects and recipes](https://ilyaux.github.io/OxRefine/moving-over.html), and the [GREL reference](https://ilyaux.github.io/OxRefine/grel.html).

## Download

**[Get the latest release](https://github.com/ilyaux/OxRefine/releases/latest).** It includes:

- installers for Windows (`.exe` or `.msi`), macOS (universal `.dmg`) and Linux (`.AppImage`, `.deb`, `.rpm`);
- the `ox` command line for each of them.

The desktop app checks for updates when it starts and offers to install them.

## Build from source

You need Rust (stable) and Node.js 22.

```bash
git clone https://github.com/ilyaux/OxRefine.git && cd OxRefine

# desktop app
cd ui && npm install && npm run tauri dev

# or in a browser, like OpenRefine: build the UI once and `ox serve` has it built in
cd ui && npm install && npm run build && cd ..
cargo run --release -p ox-cli -- serve           # open http://127.0.0.1:3030/
```

### On a server

```bash
# other machines get in through the link it prints, which carries an access token;
# files are uploaded from the browser and exports download to it
ox serve --host 0.0.0.0 --data /srv/oxrefine

docker build -t oxrefine .
docker run -p 3030:3030 -v oxrefine-data:/data oxrefine
```

### Headless

```bash
# replay an OpenRefine operation history on a file
ox apply history.json input.csv -o output.csv

# OpenRefine project archives work as input and output
ox apply history.json project.openrefine.tar.gz -o cleaned.openrefine.tar.gz
```

## How compatibility is checked

```bash
cargo test --release      # unit tests and every OpenRefine conformance suite
cd ui && npm run build    # UI typecheck and build
```

Each suite in `tests/<suite>/` has three files:

- `cases.mjs`: what to try.
- `gen.mjs`: runs the cases through OpenRefine 3.10.1 and writes `fixtures.json`.
- `known_failures.txt`: the deliberate differences, each with its reason.

A test fails when a new difference appears, and also when a known one starts passing, so the list only shrinks. Regenerating fixtures needs OpenRefine 3.10.1 next to the repository (see `tools/openrefine.mjs`). `node tests/orproject/check-export.mjs` opens the archives OxRefine wrote during `cargo test` in OpenRefine and checks every undo step there. CI runs all of this on Windows, macOS and Linux.

## Project layout

```
crates/ox-core   the engine: tables, import, facets, operations, GREL, clustering, reconciliation, export, history
crates/ox-cli    `ox apply` (batch) and `ox serve` (the browser app and its JSON API, UI built in)
ui/              React + shadcn/ui in the "Instrument" design: virtualized grid, facets, and full-window views for transform, cluster, reconcile, import, export and history
ui/src-tauri     the desktop shell: one command that forwards to the engine
tests/           OpenRefine conformance suites and their fixtures
benchmarks/      data generator, recipe, comparison against OpenRefine, regression check, report
```

## Not yet

These aren't in OxRefine yet:

- RDF, MARC and wikitext import;
- the scatterplot facet;
- Python and Clojure expressions;
- OpenRefine's database, Wikibase and Google extensions;
- translations.

[ROADMAP.md](ROADMAP.md) has the full plan.

## License

MIT. OxRefine is an independent reimplementation and contains no OpenRefine code. [NOTICE](NOTICE) lists the third-party material it includes, such as the test fixtures generated with OpenRefine, the GREL function descriptions and the Apache Commons Codec rule files.
