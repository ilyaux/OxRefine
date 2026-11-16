<!--
Keywords: OpenRefine alternative, Google Refine alternative, data cleaning tool, data cleansing, data wrangling,
data preparation, data quality, messy data, clean CSV, clean Excel, deduplicate data, remove duplicates,
fuzzy matching, fuzzy deduplication, clustering, record linkage, entity resolution, reconciliation, Wikidata
reconciliation, GREL, faceted browsing, data transformation, ETL, spreadsheet cleanup, large CSV files,
million rows, database import, PostgreSQL, MySQL, SQLite, MARC, MCP, fast, Rust, Tauri, desktop app,
self-hosted, open source, Trifacta alternative, Talend alternative
-->
<p align="center">
  <img src="docs/readme/hero.svg" width="100%" alt="OxRefine: OpenRefine's way of cleaning messy data, rebuilt in Rust. The same million-row workflow takes 0.66 s in OxRefine and 15.0 s in OpenRefine.">
</p>

<p align="center">
  <a href="https://github.com/ilyaux/OxRefine/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/ilyaux/OxRefine?label=download&color=111113"></a>
  <a href="https://github.com/ilyaux/OxRefine/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/ilyaux/OxRefine/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://ilyaux.github.io/OxRefine/"><img alt="Documentation" src="https://img.shields.io/badge/docs-ilyaux.github.io%2FOxRefine-1D4ED8"></a>
  <img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-111113">
  <img alt="Engine: Rust" src="https://img.shields.io/badge/engine-Rust-B7410E">
  <img alt="Desktop: Tauri 2" src="https://img.shields.io/badge/desktop-Tauri%202-24C8DB">
  <img alt="Checks matching OpenRefine 3.10.1" src="https://img.shields.io/badge/matches%20OpenRefine%203.10.1-2124%20of%202136%20checks-brightgreen">
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

OxRefine is a free, open-source **OpenRefine alternative** for data cleaning and data wrangling: deduplicate and fix messy CSV, Excel, JSON and database data, cluster near-duplicate spellings, transform columns with GREL, and reconcile values against Wikidata.

[OpenRefine](https://openrefine.org) is the standard tool for cleaning messy tabular data, and it slows down and runs out of memory as tables grow. OxRefine keeps its workflow, its expression language and its file formats, and replaces the engine: every column is **dictionary-encoded**, so each distinct value is stored once, and facets, clustering and value transforms work once per distinct value instead of once per row.

|  |  |
|---|---|
| **About 20× faster, 12–22× less memory** | A million-row workflow takes 0.66 s instead of 15.0 s. At 2 million rows OpenRefine runs out of its 1,400 MB heap; OxRefine needs 220 MiB. |
| **The same results** | Behaviour is pinned to fixtures generated from a real OpenRefine 3.10.1: 2,124 of 2,136 checks match, and each known difference is written down with its reason. |
| **Your work moves both ways** | OpenRefine recipes (*Undo/Redo → Extract / Apply*) run unchanged, and whole OpenRefine projects open with their undo history. OxRefine writes projects that OpenRefine opens with every step intact. |
| **Desktop, browser, server, command line or AI assistant** | A native desktop app, a browser app (`ox serve`, like OpenRefine), a server or Docker container behind an access token, headless with `ox apply`, or your AI assistant through `ox mcp`. |

## Benchmark

The same workflow ran through each tool's own HTTP API: CSV import, a text facet, fingerprint clustering, a 5-step OpenRefine recipe and CSV export. Each size ran three times in fresh processes on one Windows laptop, with OpenRefine on a 1,400 MB Java heap; the charts show medians. Every output cell is identical at every size both tools finished (8,000,008 of 8,000,008 at a million rows).

<img src="docs/readme/bench-time.svg" width="100%" alt="Whole-workflow time: 0.09 s against 1.75 s at 100k rows, 0.66 s against 15.0 s at 1M rows; at 2M rows OxRefine takes 1.38 s and OpenRefine runs out of memory.">

The gap is widest where OpenRefine works row by row and OxRefine works once per distinct value:

<img src="docs/readme/bench-steps.svg" width="100%" alt="Each step at 1M rows: import 3.9×, text facet 95×, clustering 207×, a 5-operation recipe 292×, export 6.1× faster.">

<img src="docs/readme/bench-memory.svg" width="100%" alt="Peak memory: OxRefine grows from 24 to 220 MiB between 100k and 2M rows; OpenRefine reaches 1,420 MiB at 1M rows and runs out of its 1,400 MB heap at 2M.">

<details>
<summary>The numbers</summary>

| Rows | OxRefine | OpenRefine | Faster | Peak RAM (OxRefine / OpenRefine) |
|---|---|---|---|---|
| 100,000 | 0.09 s | 1.75 s | 19× | 24 / 460 MiB |
| 250,000 | 0.15 s | 3.40 s | 23× | 38 / 820 MiB |
| 500,000 | 0.28 s | 5.91 s | 21× | 64 / 986 MiB |
| 1,000,000 | 0.66 s | 15.0 s | 23× | 116 / 1,420 MiB |
| 2,000,000 | 1.38 s | out of memory | | 220 MiB / out of memory |

Per step at a million rows:

| Step | OxRefine | OpenRefine |
|---|---|---|
| Import | 0.37 s | 1.48 s |
| Text facet | 7 ms | 0.65 s |
| Clustering | 9 ms | 1.75 s |
| Recipe (5 operations) | 33 ms | 9.72 s |
| Export | 0.24 s | 1.43 s |

Peak RAM is the process's Windows peak working set. The data repeats values the way real messy columns do; columns where every value is different gain less. Raw runs, binary hashes and the protocol: [benchmarks/release-0.6.0](benchmarks/release-0.6.0/), [benchmarks/README.md](benchmarks/README.md). To record a new sample, put OpenRefine 3.10.1 in `../OpenRefine/openrefine-3.10.1` and run:

```bash
cargo build --release --locked -p ox-cli
node benchmarks/compare.mjs --runs 3 --out benchmarks/results.json 100000 250000 500000 1000000 2000000
node tools/readme-art.mjs --input benchmarks/results.json
```
</details>

### Importing from a database

<img src="docs/readme/database-comparison.svg" width="100%" alt="Database import of 100,000 rows: PostgreSQL 155 against 708 ms, MySQL 114 against 1,294 ms, MariaDB 118 against 753 ms; peak memory about 22 MiB against 317 to 357 MiB. OpenRefine's SQLite driver failed to load.">

OxRefine's time includes saving the project; OpenRefine's stops when the project exists in memory. OpenRefine's bundled SQLite driver didn't load in the reference setup, so SQLite has no comparison. [Paired report](benchmarks/release-0.6.0/database-comparison.json).

## What it does

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/src/images/cluster-dark.png">
  <img alt="Clustering the city column: spellings of São Paulo, Köln and Kraków grouped, with row counts, a bar for each spelling and the value to merge into" src="docs/src/images/cluster.png">
</picture>

| | |
|---|---|
| **Import** | CSV and TSV, fixed-width text, Excel (`.xlsx`, `.xls`), OpenDocument, JSON and JSON Lines, XML, MARC, wikitext tables, PC-Axis and plain lines, from files, folders, zip/tar/gzip/bzip2 archives, the clipboard, a web address or Wikimedia Commons. A preview wizard with OpenRefine's options. |
| **Databases** | SQLite, PostgreSQL, MySQL and MariaDB: browse the schema or write a `SELECT`, preview the whole result, then create the project. Saved connections keep no passwords. |
| **Explore** | Text, numeric, timeline and custom facets, text filters, records mode, column profiles in every header, and a grid that stays fast at millions of rows. |
| **Clean** | GREL expressions, common transforms, mass edits, splitting and joining columns and multi-valued cells, fill and blank down, transposing, fetching URLs, language detection and `cross()` between projects. |
| **Cluster** | Key collision (fingerprint, n-gram, Metaphone 3 and the other phonetic keyers) and nearest-neighbour (Levenshtein, PPM) clustering, with filters on cluster size, row count and length. |
| **Reconcile** | Match a column against any W3C Reconciliation service, such as Wikidata or your own: judge candidates cell by cell or in bulk, search the service, preview entities, and add columns from the matched items. |
| **Keep track** | Full undo and redo, saved projects with tags and metadata, reusable recipes, and progress with cancel for long steps. |
| **Export** | CSV and TSV, custom tabular, HTML, templating, Excel (`.xlsx`, `.xls`), OpenDocument, SQL, and OpenRefine projects. |

Tables bigger than memory: set `OXREFINE_CODE_BUDGET_MIB` and the row codes spill to temporary files (experimental; see [big files](docs/src/importing.md#big-files)).

## OpenRefine compatibility

Each area is checked against fixtures produced by running the same cases through OpenRefine 3.10.1. Every difference that remains is listed, with its reason, in a `known_failures.txt` next to the fixtures.

<img src="docs/readme/compat.svg" width="100%" alt="2,124 of 2,136 checks match OpenRefine 3.10.1, by suite.">

<details>
<summary>What each suite covers</summary>

| Area | Cases matching OpenRefine |
|---|---|
| GREL expressions | 561 / 561 |
| Operations, including single-cell edits, stars, flags, transposing, fetching URLs and `cross()` | 83 / 84 |
| Facets: list, range, timeline, text, custom and records mode | 39 / 39 |
| Import options: CSV/TSV, fixed-width, Excel, ODS, JSON, XML, and zip/tar/gzip/bzip2 archives | 43 / 45 |
| Import from the clipboard and web addresses | 18 / 18 |
| Messy-data corpus: encodings, line endings, ragged rows, huge cells | 26 / 27 |
| Exports: CSV/TSV, custom tabular, HTML, templating, Excel (.xlsx and .xls), ODS, SQL | 16 / 17 |
| Clustering and GREL `phonetic()`: 6 keyers, Levenshtein and PPM kNN, 9 encoders × 127 values | 1,152 / 1,159 |
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

### With an AI assistant

`ox mcp` connects Claude Desktop, Claude Code or any MCP client to your running OxRefine: it can look at projects, preview GREL, apply steps you can undo, and export, only within the file paths you grant. [Setup](docs/src/mcp.md).

## How compatibility is checked

```bash
cargo test --release      # unit tests and every OpenRefine conformance suite
cd ui && npm run build    # UI typecheck and build
```

Each suite in `tests/<suite>/` has three files:

- `cases.mjs`: what to try.
- `gen.mjs`: runs the cases through OpenRefine 3.10.1 and writes `fixtures.json`.
- `known_failures.txt`: the deliberate differences, each with its reason.

A test fails when a new difference appears, and also when a known one starts passing, so the list only shrinks. Regenerating fixtures needs OpenRefine 3.10.1 next to the repository (see `tools/openrefine.mjs`). `node tests/orproject/check-export.mjs` opens the archives OxRefine wrote during `cargo test` in OpenRefine and checks every undo step there. CI runs the Rust suites on Windows, macOS and Linux, and typechecks and builds the UI.

## Project layout

```
crates/ox-core   the engine: tables, import, facets, operations, GREL, clustering, reconciliation, export, history
crates/ox-cli    `ox apply` (batch), `ox serve` (the browser app and its JSON API, UI built in), `ox mcp` (MCP over stdio)
ui/              React + shadcn/ui in the "Instrument" design: virtualized grid, facets, and full-window views
ui/src-tauri     the desktop shell: one command that forwards to the engine
tests/           OpenRefine conformance suites and their fixtures
benchmarks/      data generator, recipe, comparison against OpenRefine, regression check, report
```

## Not yet

- RDF import;
- the scatterplot facet;
- Python and Clojure expressions;
- Wikibase schemas and uploads, and Google Sheets;
- translations.

[ROADMAP.md](ROADMAP.md) has the full plan.

## License

MIT. OxRefine is an independent reimplementation and contains no OpenRefine code. [NOTICE](NOTICE) lists the third-party material it includes, such as the test fixtures generated with OpenRefine, the GREL function descriptions and the Apache Commons Codec rule files.
