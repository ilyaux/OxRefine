# OxRefine roadmap: full OpenRefine parity in Rust

Generated from `roadmap/tasks.json` by `node roadmap/build.mjs`. 33 of 104 tasks done.

Milestones: **v0.2** is a daily driver for cleaning CSV/Excel/JSON (persistence, full GREL core, all common operations, parity tests). **v0.5** covers the remaining formats, clustering methods and exporters plus installers and launch. **v1.0** adds reconciliation and full UI parity. **later** lists what only a minority of OpenRefine users need.

| Milestone | Tasks | Done |
|---|---|---|
| v0.1 | 10 | 10 |
| v0.2 | 46 | 21 |
| v0.5 | 32 | 2 |
| v1.0 | 5 | 0 |
| later | 11 | 0 |

## Prototype

Prove the engine model and the benchmark story.

- [x] **Dictionary-encoded column engine** · v0.1 · P0 · L. Each distinct value stored once; ops run per distinct value, rows hold u32 codes. _OpenRefine: ox-core/table.rs_
- [x] **CSV/TSV import and export** · v0.1 · P0 · S. Delimiter sniffing, BOM, ragged rows, duplicate headers. _OpenRefine: text/line-based/*sv_
- [x] **First 5 operations in OpenRefine JSON** · v0.1 · P0 · M. text-transform, mass-edit, column-addition, column-rename, column-removal with engineConfig. _OpenRefine: core/*_
- [x] **List and text facets** · v0.1 · P0 · M. engineConfig compatible, invert, blank selection, own-facet exclusion for counts. _OpenRefine: list, text_
- [x] **GREL subset** · v0.1 · P0 · M. value chains with trim, case, replace (string/regex), fingerprint; covers Common transforms. _OpenRefine: grel_
- [x] **Fingerprint and n-gram clustering** · v0.1 · P0 · S. Key collision over filtered rows, merge via mass-edit. _OpenRefine: binning: fingerprint, ngram-fingerprint_
- [x] **Undo/redo and recipe extract/apply** · v0.1 · P0 · S. Snapshot per step; recipes interchange with OpenRefine. _OpenRefine: history_
- [x] **ox apply / ox serve** · v0.1 · P0 · S. Headless recipe replay and a local JSON API. _OpenRefine: ox-cli_
- [x] **React + Tauri desktop shell** · v0.1 · P0 · L. Virtualized grid, column menus, facets, history, transform and cluster dialogs. _OpenRefine: ui/_
- [x] **Benchmark vs OpenRefine 3.10.1** · v0.1 · P0 · M. 100k-2M rows, identical output, 9-17x faster, 11-25x less RAM. _OpenRefine: benchmarks/compare.mjs_

## Core model & projects

OpenRefine's data model: typed cells, records, persistent projects.

- [x] **Typed cells** · v0.2 · P0 · L. String, number, boolean, date, null and error cells (OpenRefine's Cell model) while keeping dictionary encoding; toNumber/toDate and type guessing on import. _OpenRefine: Cell, guessCellValueTypes_
- [x] **Null vs empty string** · v0.2 · P0 · S. Distinguish blank (null) from "", matching storeBlankCellsAsNulls and isBlank/isEmptyString. _OpenRefine: storeBlankCellsAsNulls_
- [~] **Records mode** · v0.2 · P1 · L. Rows grouped into records by the key column; record-based engineConfig, record-aware facets and ops. _OpenRefine: mode: record-based_
- [x] **Row stars and flags** · v0.2 · P1 · S. Per-row star/flag bits, filterable and preserved in history. _OpenRefine: row-star, row-flag_
- [~] **Save and reopen projects** · v0.2 · P0 · L. Project files on disk (columns + history), autosave, crash-safe writes; reopening a 1M-row project in under a second. _OpenRefine: workspace, ProjectManager_
- [~] **Project library** · v0.2 · P1 · M. Open/rename/delete/tag projects, recent list, metadata like OpenRefine's project index. _OpenRefine: get-all-project-metadata_
- [~] **Open OpenRefine project archives** · v0.5 · P1 · L. Read .openrefine.tar.gz including history, so users can migrate existing projects in one step. _OpenRefine: import-project_
- [ ] **Reconciliation data in the cell model** · v0.5 · P2 · M. Per-cell recon judgment, candidates and match, stored outside the dictionary. _OpenRefine: Recon, ReconCandidate_

## Import

Every format OpenRefine imports, with its options and preview.

- [~] **CSV options parity** · v0.2 · P0 · M. Encoding detection, quote char, ignore/skip lines, header lines, limit, trim, guess types, column names on/off. _OpenRefine: text/line-based/*sv options_
- [~] **Excel xlsx/xls** · v0.2 · P0 · M. Sheet selection, multiple sheets into one project, dates as dates (calamine). _OpenRefine: binary/text/xml/xls/xlsx_
- [~] **OpenDocument spreadsheet** · v0.5 · P2 · S. ODS through the same sheet importer. _OpenRefine: text/xml/ods_
- [~] **JSON and JSON Lines** · v0.2 · P0 · L. Pick the record path in a tree preview; nested fields become columns; JSONL streams. _OpenRefine: text/json_
- [~] **XML** · v0.5 · P1 · L. Pick the record element in a tree preview, attributes and nested elements as columns. _OpenRefine: text/xml_
- [~] **Fixed-width text** · v0.5 · P2 · M. Column widths with a draggable ruler in preview. _OpenRefine: text/line-based/fixed-width_
- [~] **Line-based text** · v0.5 · P2 · S. One line per row, lines-per-row grouping. _OpenRefine: text/line-based_
- [ ] **RDF (N-Triples, N3, Turtle, RDF/XML, JSON-LD)** · later · P2 · L. Subject-per-record import as OpenRefine does. _OpenRefine: text/rdf/*_
- [ ] **MARC records** · later · P2 · M. Library records via MARCXML conversion. _OpenRefine: text/marc_
- [ ] **Wikitext tables** · later · P2 · M. Parse MediaWiki table markup, keep links for reconciliation. _OpenRefine: text/wiki_
- [~] **Import from URL and clipboard** · v0.2 · P1 · S. Download with progress and paste-as-table, like OpenRefine's Web Addresses and Clipboard tabs. _OpenRefine: create-project-from-upload_
- [~] **Zip/gzip and multi-file import** · v0.5 · P2 · S. Pick files inside archives, add a file-name column. _OpenRefine: includeFileSources, includeArchiveFileName_
- [~] **Import preview wizard** · v0.2 · P0 · M. Live preview of the first rows while options change, format auto-detect. _OpenRefine: parsing-ui_

## Export

Every exporter OpenRefine ships.

- [~] **CSV/TSV/custom separator export** · v0.2 · P0 · S. Separator, quoting, line endings, BOM, filtered rows only. _OpenRefine: csv, tsv, *sv_
- [~] **Excel export** · v0.2 · P0 · M. xlsx with typed cells (rust_xlsxwriter); legacy xls only if users ask. _OpenRefine: xlsx, xls_
- [ ] **ODS export** · v0.5 · P2 · M. OpenDocument spreadsheet writer. _OpenRefine: ods_
- [~] **HTML table export** · v0.5 · P2 · S. Plain HTML table of the filtered rows. _OpenRefine: html_
- [~] **Templating exporter** · v0.5 · P1 · M. Prefix / row template / separator / suffix with GREL inside {{ }}; the usual way to produce JSON. _OpenRefine: template_
- [~] **SQL exporter** · v0.5 · P2 · M. CREATE TABLE + INSERT statements with column types and options. _OpenRefine: sql_
- [~] **Custom tabular exporter** · v0.5 · P1 · M. Choose and order columns, format dates and recon values, save as a reusable preset. _OpenRefine: custom-tabular-exporter_
- [~] **Export project archive** · v0.5 · P2 · M. Write .openrefine.tar.gz that OpenRefine can open, so moving back is possible. _OpenRefine: export-project_

## Operations

All 37 OpenRefine operations, same JSON, same results.

- [x] **Column move, reorder, multi-removal** · v0.2 · P0 · S. column-move(-left/-right/-first/-last), column-reorder, column-multi-removal. _OpenRefine: column-move*, column-reorder, column-multi-removal_
- [x] **Split column** · v0.2 · P0 · M. By separator, regex or field lengths; guess types; remove original. _OpenRefine: column-split_
- [x] **Split / join multi-valued cells** · v0.2 · P0 · M. Explode cells into rows of a record and back. _OpenRefine: multivalued-cell-split, multivalued-cell-join_
- [x] **Fill down / blank down** · v0.2 · P0 · S. Row- and record-aware. _OpenRefine: fill-down, blank-down_
- [~] **Transpose and key-value columnize** · v0.5 · P1 · L. Columns into rows, rows into columns, key/value pairs into columns. _OpenRefine: transpose-columns-into-rows, transpose-rows-into-columns, key-value-columnize_
- [x] **Row operations** · v0.2 · P0 · M. Remove matching rows, keep matched, add rows, remove duplicates by key columns. _OpenRefine: row-removal, row-keep-matched, row-addition, row-duplicate-removal_
- [x] **Sort and reorder rows permanently** · v0.2 · P0 · M. Multi-column sort criteria by type, then make it permanent. _OpenRefine: row-reorder_
- [~] **Edit a single cell** · v0.2 · P0 · S. Inline edit with type, recorded in history (and in exported histories where OpenRefine records it). _OpenRefine: edit-one-cell_
- [~] **Add column by fetching URLs** · v0.5 · P1 · L. Async with throttle delay, custom headers, cache, retry and progress; cancellable. _OpenRefine: column-addition-by-fetching-urls_
- [x] **onError and repeat semantics** · v0.2 · P0 · S. keep-original, set-to-blank, store-error exactly as OpenRefine once GREL can error. _OpenRefine: onError_
- [ ] **Reconciliation operations** · v1.0 · P1 · L. recon, match best, judge similar, clear similar, mark new, discard, match specific, copy across columns, use values as identifiers. _OpenRefine: recon-*_

## GREL

The full expression language: ~130 functions, controls, errors and coercion.

- [x] **Full GREL parser and evaluator** · v0.2 · P0 · L. Operators, literals, arrays, function and method syntax, variables value/cell/cells/row/record/rowIndex. _OpenRefine: grel Parser, Evaluable_
- [x] **String functions (39)** · v0.2 · P0 · L. split, smartSplit, partition, match, find, replaceChars, replaceEach, escape/unescape, encode/decode, phonetic, diff, unicode, detectLanguage and the rest. _OpenRefine: expr/functions/strings_
- [x] **Array functions (7)** · v0.2 · P0 · S. join, sort, reverse, uniques, inArray, zip, argsToArray. _OpenRefine: expr/functions/arrays_
- [x] **Math functions (38)** · v0.5 · P1 · M. round, floor, ceil, mod, pow, log, trig, combin, fact, gcd/lcm, sum, min/max, randomNumber. _OpenRefine: expr/functions/math_
- [x] **Dates** · v0.2 · P0 · M. toDate with Java-style patterns, datePart, inc, diff, now, toString with formats; time zones. _OpenRefine: ToDate, DatePart, Inc, Now_
- [x] **Controls (15)** · v0.2 · P0 · M. if, with, forEach, forEachIndex, forRange, forNonBlank, filter, isBlank, isNull, isNumeric, isError and friends. _OpenRefine: grel/controls_
- [x] **Other functions (13) and booleans (4)** · v0.2 · P0 · S. coalesce, cross, facetCount, get, hasField, jsonize, length, slice, toNumber, toString, type; and, or, not, xor. _OpenRefine: expr/functions, booleans_
- [x] **HTML/XML functions (11)** · v0.5 · P1 · M. parseHtml, parseXml, select with CSS selectors, htmlText, ownText, xmlAttr and others (scraper / roxmltree). _OpenRefine: expr/functions/html, xml_
- [x] **parseJson and JSON paths** · v0.2 · P1 · S. Field access on parsed JSON like OpenRefine. _OpenRefine: ParseJson_
- [x] **Error and type-coercion semantics** · v0.2 · P0 · M. Match OpenRefine's EvalError propagation, null handling and number formatting to the character. _OpenRefine: EvalError_
- [~] **cross() across projects** · v0.5 · P1 · M. Lookup into another open project by key column, with an index cache. _OpenRefine: Cross_
- [ ] **Python and Clojure expressions** · later · P2 · L. OpenRefine ships Jython 2.7 and Clojure; decide between embedded Python (pyo3) or clear unsupported errors. _OpenRefine: jython, clojure_

## Facets

Every facet type, including customized and expression facets.

- [~] **Numeric range facet** · v0.2 · P0 · M. Histogram, draggable range, numeric/non-numeric/blank/error toggles. _OpenRefine: range_
- [ ] **Timeline facet** · v0.5 · P1 · M. Date histogram with range selection. _OpenRefine: timerange_
- [ ] **Scatterplot facet** · later · P2 · L. Scatterplot matrix and brushed 2D selection. _OpenRefine: scatterplot_
- [~] **Custom facets by expression** · v0.2 · P0 · M. List and range facets over any GREL expression, needs the full evaluator. _OpenRefine: list facet expression_
- [~] **Customized facets menu** · v0.2 · P1 · M. Word, duplicates, text length, Unicode chars, blank, error, star and flag facets. _OpenRefine: Customized facets_
- [~] **Facet details** · v0.2 · P1 · S. Sort by name/count, include/exclude, edit a choice in place, choice limit, collapse. _OpenRefine: list-facet.js_

## Clustering

All key-collision keyers and nearest-neighbour methods.

- [~] **Phonetic keyers** · v0.5 · P1 · M. Metaphone3, Cologne phonetic, Daitch-Mokotoff, Beider-Morse. _OpenRefine: binning keyers_
- [~] **Nearest-neighbour clustering** · v0.5 · P1 · L. Levenshtein distance with n-gram blocking, radius and block size; parallel, with progress and cancel. _OpenRefine: knn: levenshtein_
- [ ] **PPM distance for nearest neighbours** · v0.5 · P2 · M. Port vicino's PPM distance: 10*((C(x+y)+C(y+x))/(C(x+x)+C(y+y)) - 1), where C is the compressed length from arithcode's PPMModel(8). Needs the arithcode-1.2 sources (OpenRefine/simile-vicino, lib-src/arithcode-1.2-sources.jar, 1.3 MB). Then drop the 2 PPM lines from tests/cluster/known_failures.txt and enable PPM in the cluster dialog. _OpenRefine: knn: ppm_
- [ ] **Cluster dialog parity** · v0.5 · P2 · S. Filter clusters by size, row count, value length and spread; export clusters as JSON. _OpenRefine: clustering-dialog_
- [ ] **Custom clustering functions** · later · P2 · M. User-defined keying and distance functions in GREL. _OpenRefine: clustering-functions-dialog_

## Reconciliation

W3C Reconciliation API: matching, judging, data extension.

- [ ] **Reconciliation API client** · v1.0 · P1 · L. W3C Reconciliation API: service manifest, batched queries, type and property constraints, service registry with Wikidata preset. _OpenRefine: StandardReconConfig_
- [ ] **Matching UI** · v1.0 · P1 · M. Candidates with scores in cells, one-click match, search-for-match, auto-match threshold. _OpenRefine: recon cell UI_
- [ ] **Reconciliation facets** · v1.0 · P1 · M. Judgment, best-candidate score, type match and other recon facets. _OpenRefine: recon facets_
- [ ] **Add columns from reconciled values** · v1.0 · P1 · M. Data extension API: fetch properties of matched entities into new columns. _OpenRefine: extend-reconciled-data_

## Desktop UI

Everything OpenRefine's UI does, in a modern, keyboard-first app.

- [~] **Home: projects and create project** · v0.2 · P0 · M. Recent projects, import sources, drag-and-drop files. _OpenRefine: index page_
- [~] **Grid parity** · v0.2 · P0 · M. Rows/records toggle, inline cell edit, star/flag column, type-aware rendering, column widths, beyond-1M-row scrolling. _OpenRefine: data-table-view_
- [~] **Expression editor** · v0.2 · P0 · M. CodeMirror 6 with GREL highlighting, autocomplete with function docs, live preview, history and starred expressions. _OpenRefine: expression-preview-dialog_
- [~] **Column menus for every operation** · v0.2 · P0 · M. Each operation reachable from the column header like OpenRefine. _OpenRefine: menu-edit-cells, menu-edit-column_
- [ ] **Command palette and shortcuts** · v0.5 · P1 · S. Ctrl+K to run any operation on the focused column. _OpenRefine: new_
- [~] **History panel parity** · v0.2 · P1 · S. Extract with step selection, apply from file, filter steps. _OpenRefine: history-panel_
- [~] **Long-running jobs** · v0.2 · P0 · M. Background processes with progress and cancel (imports, URL fetching, reconciliation). _OpenRefine: get-processes, cancel-processes_
- [~] **Themed scrollbars** · v0.2 · P1 · S. The grid scrollbars are the browser default light ones; style them with theme tokens in light and dark, wherever native scrolling is used. _OpenRefine: new_
- [ ] **Translations** · v0.5 · P2 · M. i18n framework; OpenRefine's BSD-licensed translation files can seed 40+ languages. _OpenRefine: langs/*.json_

## Compatibility & QA

Prove parity automatically against real OpenRefine.

- [x] **Operation conformance suite** · v0.2 · P0 · L. Run every operation through OpenRefine's API and ours on the same corpus and diff outputs cell by cell (extends compare.mjs). _OpenRefine: benchmarks/compare.mjs_
- [x] **GREL conformance suite** · v0.2 · P0 · M. Thousands of expressions evaluated in both engines, including errors and edge values. _OpenRefine: preview-expression_
- [~] **Messy-data test corpus** · v0.2 · P0 · M. Real-world files: encodings, ragged rows, embedded newlines, huge cells, mixed types. _OpenRefine: tests/_
- [ ] **Fuzz importers and GREL parser** · v0.5 · P2 · S. cargo-fuzz targets for every parser. _OpenRefine: new_
- [~] **CI on Windows, macOS, Linux** · v0.2 · P0 · S. GitHub Actions: build, tests, UI typecheck, conformance on a small corpus. _OpenRefine: new_

## Performance

Keep the 10x lead as features land; close the import gap.

- [~] **Parallel CSV import** · v0.2 · P0 · M. Chunked multi-threaded parsing plus a faster hasher; import is the smallest gap today (1.6x at 2M rows). _OpenRefine: benchmark_
- [ ] **History as diffs** · v0.5 · P1 · M. Store changed codes instead of full column snapshots so long histories on big files stay small. _OpenRefine: project.rs_
- [ ] **Datasets larger than RAM** · later · P2 · L. Memory-mapped column files and spill-to-disk for 50M+ rows. _OpenRefine: new_
- [ ] **Benchmark tracking** · v0.5 · P2 · S. Run the comparison on every release and publish the chart. _OpenRefine: benchmarks/_

## Release

Installers, docs and a public launch.

- [x] **Public repository** · v0.2 · P0 · S. git init, license (MIT/Apache-2.0), contributing guide, issue templates; credit OpenRefine (BSD-3). _OpenRefine: new_
- [ ] **Installers and auto-update** · v0.5 · P1 · M. Tauri bundles: msi, dmg, AppImage/deb; code signing; updater. _OpenRefine: tauri bundle_
- [ ] **Headless server mode** · v0.5 · P1 · S. ox serve with the UI embedded, for servers and Docker, like running OpenRefine remotely. _OpenRefine: refine -i 0.0.0.0_
- [ ] **OpenRefine HTTP API compatibility** · later · P2 · L. Serve OpenRefine's /command/core/* so existing clients and scripts work unchanged. _OpenRefine: openrefine-client_
- [ ] **Docs site and migration guide** · v0.5 · P1 · M. Feature map from OpenRefine, GREL reference, what is not supported yet. _OpenRefine: new_
- [ ] **Launch** · v0.5 · P1 · S. README demo GIF, benchmark chart, Show HN / Reddit r/datacleaning / OpenRefine forum post. _OpenRefine: new_

## Extensions

OpenRefine's bundled extensions: database, Wikibase, scripting.

- [ ] **Database import** · later · P2 · L. PostgreSQL, MySQL/MariaDB, SQLite with a query editor and preview. _OpenRefine: extensions/database_
- [ ] **Wikibase schema and upload** · later · P2 · XL. Schema editor, issue checks, preview and upload edits to Wikidata/Wikibase. _OpenRefine: extensions/wikibase_
- [ ] **PC-Axis import** · later · P2 · S. Statistical PX file importer. _OpenRefine: extensions/pc-axis_
