# OxRefine roadmap

Full OpenRefine parity, in Rust.

Current release: [**v0.8.0**](https://github.com/ilyaux/OxRefine/releases/tag/v0.8.0), published 2026-11-23.

<img src="docs/readme/roadmap.svg" width="100%" alt="Progress by phase: tasks done, in review, in progress and planned.">

**119 of 135 tasks done.** Generated from `roadmap/tasks.json` by `node roadmap/build.mjs`; statuses follow the Kanzo board. **Done** is closed by the owner, **review** waits for the owner and doesn't count as done, **planned** is todo and backlog together. Shipped features may still be in review.

| Milestone | Tasks | Done | Review | In progress | Planned |
|---|---|---|---|---|---|
| v0.1 | 10 | 10 | 0 | 0 | 0 |
| v0.2 | 46 | 45 | 0 | 1 | 0 |
| v0.5 | 50 | 46 | 1 | 1 | 2 |
| v1.0 | 13 | 12 | 1 | 0 | 0 |
| later | 16 | 6 | 8 | 1 | 1 |
| **Total** | **135** | **119** | **10** | **3** | **3** |

Milestones are planning groups, not release checklists: **v0.2** the core cleaning workflow, **v0.5** formats, clustering, exporters and distribution, **v1.0** reconciliation and UI parity, **later** specialised workflows. Features can ship before their group; the release notes say what each version has.

## Prototype

Prove the engine model and the benchmark story. **10 of 10 done.**

<details><summary>10 done</summary>

- [x] **Dictionary-encoded column engine** — Each distinct value stored once; ops run per distinct value, rows hold u32 codes. <sub>v0.1 · P0 · L · OpenRefine: ox-core/table.rs</sub>
- [x] **CSV/TSV import and export** — Delimiter sniffing, BOM, ragged rows, duplicate headers. <sub>v0.1 · P0 · S · OpenRefine: text/line-based/*sv</sub>
- [x] **First 5 operations in OpenRefine JSON** — text-transform, mass-edit, column-addition, column-rename, column-removal with engineConfig. <sub>v0.1 · P0 · M · OpenRefine: core/*</sub>
- [x] **List and text facets** — engineConfig compatible, invert, blank selection, own-facet exclusion for counts. <sub>v0.1 · P0 · M · OpenRefine: list, text</sub>
- [x] **GREL subset** — value chains with trim, case, replace (string/regex), fingerprint; covers Common transforms. <sub>v0.1 · P0 · M · OpenRefine: grel</sub>
- [x] **Fingerprint and n-gram clustering** — Key collision over filtered rows, merge via mass-edit. <sub>v0.1 · P0 · S · OpenRefine: binning: fingerprint, ngram-fingerprint</sub>
- [x] **Undo/redo and recipe extract/apply** — Snapshot per step; recipes interchange with OpenRefine. <sub>v0.1 · P0 · S · OpenRefine: history</sub>
- [x] **ox apply / ox serve** — Headless recipe replay and a local JSON API. <sub>v0.1 · P0 · S · OpenRefine: ox-cli</sub>
- [x] **React + Tauri desktop shell** — Virtualized grid, column menus, facets, history, transform and cluster dialogs. <sub>v0.1 · P0 · L · OpenRefine: ui/</sub>
- [x] **Benchmark vs OpenRefine 3.10.1** — 100k-2M rows, identical output, 9-17x faster, 11-25x less RAM. <sub>v0.1 · P0 · M · OpenRefine: benchmarks/compare.mjs</sub>

</details>

## Core model & projects

OpenRefine's data model: typed cells, records, persistent projects. **10 of 10 done.**

<details><summary>10 done</summary>

- [x] **Typed cells** — String, number, boolean, date, null and error cells (OpenRefine's Cell model) while keeping dictionary encoding; toNumber/toDate and type guessing on import. <sub>v0.2 · P0 · L · OpenRefine: Cell, guessCellValueTypes</sub>
- [x] **Null vs empty string** — Distinguish blank (null) from "", matching storeBlankCellsAsNulls and isBlank/isEmptyString. <sub>v0.2 · P0 · S · OpenRefine: storeBlankCellsAsNulls</sub>
- [x] **Records mode** — Rows grouped into records by the key column; record-based engineConfig, record-aware facets and ops. <sub>v0.2 · P1 · L · OpenRefine: mode: record-based</sub>
- [x] **Row stars and flags** — Per-row star/flag bits, filterable and preserved in history. <sub>v0.2 · P1 · S · OpenRefine: row-star, row-flag</sub>
- [x] **Save and reopen projects** — Project files on disk (columns + history), autosave, crash-safe writes; reopening a 1M-row project in under a second. <sub>v0.2 · P0 · L · OpenRefine: workspace, ProjectManager</sub>
- [x] **Project library** — Open/rename/delete/tag projects, recent list, metadata like OpenRefine's project index. <sub>v0.2 · P1 · M · OpenRefine: get-all-project-metadata</sub>
- [x] **Open OpenRefine project archives** — Read .openrefine.tar.gz including history, so users can migrate existing projects in one step. <sub>v0.5 · P1 · L · OpenRefine: import-project</sub>
- [x] **Reconciliation data in the cell model** — Every cell can carry a Recon: id, service, identifierSpace, schemaSpace, judgment (none / matched / new), judgmentAction, judgmentHistoryEntry, match (id, name, types, score), candidates (ranked), features (typeMatch, nameMatch, nameLevenshtein, nameWordDistance), matchRank. Stored beside the dictionary codes, not in them. GREL sees cell.recon.* (judgment, match, best, candidates, features, service…). Recon data reads from and writes to OpenRefine project archives (the recon pool in data.txt and change files) instead of being dropped, and survives operations that move cells (column copy, move, reorder, rows remove/reorder, fill down/blank down, records). <sub>v0.5 · P1 · M · OpenRefine: Recon, ReconCandidate, ReconStats, Pool</sub>
- [x] **Project metadata and tags** — OpenRefine's project metadata: tags, description, creator, contributors, subject, license, homepage, image, title, version, custom metadata fields; editable from the home page, projects filtered by tag, read from and written to archive metadata.json (tags and fields are read today but not shown). <sub>v0.5 · P2 · M · OpenRefine: ProjectMetadata, SetProjectMetadataCommand, SetProjectTagsCommand</sub>
- [x] **Preserve extension data in OpenRefine project round-trips** — Inventory and preserve extension-owned project data, including overlay models such as wikibaseSchema. Define safe handling when an operation invalidates opaque data; report unsupported history and prevent silent loss on import, restart and export. Add fixtures generated with OpenRefine 3.10.1 and verify exported archives in OpenRefine. <sub>v1.0 · P1 · L · OpenRefine: orproject.rs, store.rs; forum feedback on extension compatibility</sub>

</details>

## Import

Every format OpenRefine imports, with its options and preview. **13 of 14 done.**

- [ ] **RDF (N-Triples, N3, Turtle, RDF/XML, JSON-LD)** (review) — OpenRefine's RDF importers: one row per subject, one column per predicate, multiple values as record rows. Formats: RDF/N3, N-Triples, Turtle, RDF/XML, JSON-LD. Fixtures from OpenRefine. <sub>later · P2 · L · OpenRefine: RdfTripleImporter, RdfXmlTripleImporter, RdfJsonldTripleImporter</sub>

<details><summary>13 done</summary>

- [x] **CSV options parity** — Encoding detection, quote char, ignore/skip lines, header lines, limit, trim, guess types, column names on/off. <sub>v0.2 · P0 · M · OpenRefine: text/line-based/*sv options</sub>
- [x] **Excel xlsx/xls** — Sheet selection, multiple sheets into one project, dates as dates (calamine). <sub>v0.2 · P0 · M · OpenRefine: binary/text/xml/xls/xlsx</sub>
- [x] **OpenDocument spreadsheet** — ODS through the same sheet importer. <sub>v0.5 · P2 · S · OpenRefine: text/xml/ods</sub>
- [x] **JSON and JSON Lines** — Pick the record path in a tree preview; nested fields become columns; JSONL streams. <sub>v0.2 · P0 · L · OpenRefine: text/json</sub>
- [x] **XML** — Pick the record element in a tree preview, attributes and nested elements as columns. <sub>v0.5 · P1 · L · OpenRefine: text/xml</sub>
- [x] **Fixed-width text** — Column widths with a draggable ruler in preview. <sub>v0.5 · P2 · M · OpenRefine: text/line-based/fixed-width</sub>
- [x] **Line-based text** — One line per row, lines-per-row grouping. <sub>v0.5 · P2 · S · OpenRefine: text/line-based</sub>
- [x] **MARC records** — MARC (ISO 2709) import: converted to MARCXML and read with the XML importer, as OpenRefine does. <sub>later · P2 · M · OpenRefine: MarcImporter</sub>
- [x] **Wikitext tables** — Import tables from MediaWiki markup: table choice, headers, rowspan/colspan, links as cells (optionally reconciled to Wikidata), references, blank-cell handling. Same options as OpenRefine. <sub>later · P2 · M · OpenRefine: WikitextImporter</sub>
- [x] **Import from URL and clipboard** — Download with progress and paste-as-table, like OpenRefine's Web Addresses and Clipboard tabs. <sub>v0.2 · P1 · S · OpenRefine: create-project-from-upload</sub>
- [x] **Zip/gzip and multi-file import** — Pick files inside archives, add a file-name column. <sub>v0.5 · P2 · S · OpenRefine: includeFileSources, includeArchiveFileName</sub>
- [x] **Import preview wizard** — Live preview of the first rows while options change, format auto-detect. <sub>v0.2 · P0 · M · OpenRefine: parsing-ui</sub>
- [x] **bzip2 archives** — Import .bz2 and .tar.bz2 like OpenRefine (it reads gzip, bzip2, zip and tar), in the wizard and from URLs. <sub>v0.5 · P2 · S · OpenRefine: ImportingUtilities.tryOpenAsCompressedFile</sub>

</details>

## Export

Every exporter OpenRefine ships. **9 of 9 done.**

<details><summary>9 done</summary>

- [x] **CSV/TSV/custom separator export** — Separator, quoting, line endings, BOM, filtered rows only. <sub>v0.2 · P0 · S · OpenRefine: csv, tsv, *sv</sub>
- [x] **Excel export** — xlsx with typed cells (rust_xlsxwriter); legacy xls only if users ask. <sub>v0.2 · P0 · M · OpenRefine: xlsx, xls</sub>
- [x] **ODS export** — OpenDocument spreadsheet writer. <sub>v0.5 · P2 · M · OpenRefine: ods</sub>
- [x] **HTML table export** — Plain HTML table of the filtered rows. <sub>v0.5 · P2 · S · OpenRefine: html</sub>
- [x] **Templating exporter** — Prefix / row template / separator / suffix with GREL inside {{ }}; the usual way to produce JSON. <sub>v0.5 · P1 · M · OpenRefine: template</sub>
- [x] **SQL exporter** — CREATE TABLE + INSERT statements with column types and options. <sub>v0.5 · P2 · M · OpenRefine: sql</sub>
- [x] **Custom tabular exporter** — Choose and order columns, format dates and recon values, save as a reusable preset. <sub>v0.5 · P1 · M · OpenRefine: custom-tabular-exporter</sub>
- [x] **Export project archive** — Write .openrefine.tar.gz that OpenRefine can open, so moving back is possible. <sub>v0.5 · P2 · M · OpenRefine: export-project</sub>
- [x] **Excel 97 (.xls) export** — OpenRefine exports both .xls (BIFF8) and .xlsx. Write .xls with typed cells, the sheet named after the project, OpenRefine's cell limits (32767 chars, 65536 rows). <sub>v0.5 · P2 · M · OpenRefine: XlsExporter (xlsx=false)</sub>

</details>

## Operations

All 37 OpenRefine operations, same JSON, same results. **11 of 11 done.**

<details><summary>11 done</summary>

- [x] **Column move, reorder, multi-removal** — column-move(-left/-right/-first/-last), column-reorder, column-multi-removal. <sub>v0.2 · P0 · S · OpenRefine: column-move*, column-reorder, column-multi-removal</sub>
- [x] **Split column** — By separator, regex or field lengths; guess types; remove original. <sub>v0.2 · P0 · M · OpenRefine: column-split</sub>
- [x] **Split / join multi-valued cells** — Explode cells into rows of a record and back. <sub>v0.2 · P0 · M · OpenRefine: multivalued-cell-split, multivalued-cell-join</sub>
- [x] **Fill down / blank down** — Row- and record-aware. <sub>v0.2 · P0 · S · OpenRefine: fill-down, blank-down</sub>
- [x] **Transpose and key-value columnize** — Columns into rows, rows into columns, key/value pairs into columns. <sub>v0.5 · P1 · L · OpenRefine: transpose-columns-into-rows, transpose-rows-into-columns, key-value-columnize</sub>
- [x] **Row operations** — Remove matching rows, keep matched, add rows, remove duplicates by key columns. <sub>v0.2 · P0 · M · OpenRefine: row-removal, row-keep-matched, row-addition, row-duplicate-removal</sub>
- [x] **Sort and reorder rows permanently** — Multi-column sort criteria by type, then make it permanent. <sub>v0.2 · P0 · M · OpenRefine: row-reorder</sub>
- [x] **Edit a single cell** — Inline edit with type, recorded in history (and in exported histories where OpenRefine records it). <sub>v0.2 · P0 · S · OpenRefine: edit-one-cell</sub>
- [x] **Add column by fetching URLs** — Async with throttle delay, custom headers, cache, retry and progress; cancellable. <sub>v0.5 · P1 · L · OpenRefine: column-addition-by-fetching-urls</sub>
- [x] **onError and repeat semantics** — keep-original, set-to-blank, store-error exactly as OpenRefine once GREL can error. <sub>v0.2 · P0 · S · OpenRefine: onError</sub>
- [x] **Reconciliation operations** — The operations around reconciliation: match each cell to its best candidate, create a new item for each cell, match all filtered cells to a chosen item, discard judgments, clear reconciliation data, copy reconciliation data to other columns, use values as identifiers, add a column of matched entity identifiers/names. Same operation JSON and results as OpenRefine 3.10.1, checked by fixtures. <sub>v1.0 · P1 · M · OpenRefine: ReconMatchBestCandidatesOperation, ReconMarkNewTopicsOperation, ReconDiscardJudgmentsOperation, ReconClearSimilarCellsOperation, ReconCopyAcrossColumnsOperation, ReconUseValuesAsIdentifiersOperation, ReconMatchSpecificTopicOperation</sub>

</details>

## GREL

The full expression language: ~130 functions, controls, errors and coercion. **13 of 14 done.**

- [ ] **Python and Clojure expressions** (review) — Expressions in Jython (Python 2.7 semantics) and Clojure with value, cell, cells, row, rowIndex bound as in OpenRefine; selectable in the expression editor and recipes ('jython:', 'clojure:' prefixes). Python runs in embedded RustPython with a Python 2 compatibility layer, Clojure in OxRefine's own interpreter; tests/langs pins 337 expressions to OpenRefine 3.10.1. <sub>later · P2 · L · OpenRefine: JythonEvaluable, ClojureEvaluable</sub>

<details><summary>13 done</summary>

- [x] **Full GREL parser and evaluator** — Operators, literals, arrays, function and method syntax, variables value/cell/cells/row/record/rowIndex. <sub>v0.2 · P0 · L · OpenRefine: grel Parser, Evaluable</sub>
- [x] **String functions (39)** — split, smartSplit, partition, match, find, replaceChars, replaceEach, escape/unescape, encode/decode, phonetic, diff, unicode, detectLanguage and the rest. <sub>v0.2 · P0 · L · OpenRefine: expr/functions/strings</sub>
- [x] **Array functions (7)** — join, sort, reverse, uniques, inArray, zip, argsToArray. <sub>v0.2 · P0 · S · OpenRefine: expr/functions/arrays</sub>
- [x] **Math functions (38)** — round, floor, ceil, mod, pow, log, trig, combin, fact, gcd/lcm, sum, min/max, randomNumber. <sub>v0.5 · P1 · M · OpenRefine: expr/functions/math</sub>
- [x] **Dates** — toDate with Java-style patterns, datePart, inc, diff, now, toString with formats; time zones. <sub>v0.2 · P0 · M · OpenRefine: ToDate, DatePart, Inc, Now</sub>
- [x] **Controls (15)** — if, with, forEach, forEachIndex, forRange, forNonBlank, filter, isBlank, isNull, isNumeric, isError and friends. <sub>v0.2 · P0 · M · OpenRefine: grel/controls</sub>
- [x] **Other functions (13) and booleans (4)** — coalesce, cross, facetCount, get, hasField, jsonize, length, slice, toNumber, toString, type; and, or, not, xor. <sub>v0.2 · P0 · S · OpenRefine: expr/functions, booleans</sub>
- [x] **HTML/XML functions (11)** — parseHtml, parseXml, select with CSS selectors, htmlText, ownText, xmlAttr and others (scraper / roxmltree). <sub>v0.5 · P1 · M · OpenRefine: expr/functions/html, xml</sub>
- [x] **parseJson and JSON paths** — Field access on parsed JSON like OpenRefine. <sub>v0.2 · P1 · S · OpenRefine: ParseJson</sub>
- [x] **Error and type-coercion semantics** — Match OpenRefine's EvalError propagation, null handling and number formatting to the character. <sub>v0.2 · P0 · M · OpenRefine: EvalError</sub>
- [x] **cross() across projects** — Lookup into another open project by key column, with an index cache. <sub>v0.5 · P1 · M · OpenRefine: Cross</sub>
- [x] **detectLanguage()** — GREL detectLanguage(s): ISO 639-1 code from OpenRefine's language-detector n-gram profiles, the same answers on the fixture texts (currently 2 known failures). <sub>v0.5 · P2 · M · OpenRefine: DetectLanguage, optimaize language-detector</sub>
- [x] **GREL record variable** — In records mode OpenRefine's expressions see the current record: record.index, record.fromRowIndex, record.toRowIndex, record.rowCount, record.cells["Name"].value (an array of the record's values). OxRefine's GREL returns null for record today (found while writing the GREL reference). Fixtures from OpenRefine for each field, in transforms, new columns and facets. <sub>v0.5 · P2 · S · OpenRefine: WrappedRecord, RecordModel</sub>

</details>

## Facets

Every facet type, including customized and expression facets. **5 of 6 done.**

- [ ] **Scatterplot facet** (review) — Facet → Scatterplot matrix of numeric columns, then a scatterplot facet on a pair: linear/log scales, rotation, dot size, rectangle selection that filters rows; facet JSON (scatterplot, from_x/to_x/from_y/to_y) as in OpenRefine. <sub>later · P2 · M · OpenRefine: ScatterplotFacet, GetScatterplotCommand</sub>

<details><summary>5 done</summary>

- [x] **Numeric range facet** — Histogram, draggable range, numeric/non-numeric/blank/error toggles. <sub>v0.2 · P0 · M · OpenRefine: range</sub>
- [x] **Timeline facet** — Date histogram with range selection. <sub>v0.5 · P1 · M · OpenRefine: timerange</sub>
- [x] **Custom facets by expression** — List and range facets over any GREL expression, needs the full evaluator. <sub>v0.2 · P0 · M · OpenRefine: list facet expression</sub>
- [x] **Customized facets menu** — Word, duplicates, text length, Unicode chars, blank, error, star and flag facets. <sub>v0.2 · P1 · M · OpenRefine: Customized facets</sub>
- [x] **Facet details** — Sort by name/count, include/exclude, edit a choice in place, choice limit, collapse. <sub>v0.2 · P1 · S · OpenRefine: list-facet.js</sub>

</details>

## Clustering

All key-collision keyers and nearest-neighbour methods. **4 of 5 done.**

- [ ] **Custom clustering functions** (review) — OpenRefine 3.8+ lets users add key-collision keyers and kNN distances written in GREL/Python, saved in preferences and listed in the cluster dialog. <sub>later · P2 · M · OpenRefine: ClusteringFunctionsCommand, preferences: ui.clustering.customKeyingFunctions</sub>

<details><summary>4 done</summary>

- [x] **Phonetic keyers** — Metaphone3, Cologne phonetic, Daitch-Mokotoff, Beider-Morse. <sub>v0.5 · P1 · M · OpenRefine: binning keyers</sub>
- [x] **Nearest-neighbour clustering** — Levenshtein distance with n-gram blocking, radius and block size; parallel, with progress and cancel. <sub>v0.5 · P1 · L · OpenRefine: knn: levenshtein</sub>
- [x] **PPM distance for nearest neighbours** — Prediction by Partial Matching distance, as OpenRefine computes it with its bundled arithmetic coder (needs permission to download the reference code). 2 known failures today. <sub>v0.5 · P2 · S · OpenRefine: PPMDistance, arithcode</sub>
- [x] **Cluster dialog parity** — Filter clusters by size, row count, value length and spread; export clusters as JSON. <sub>v0.5 · P2 · S · OpenRefine: clustering-dialog</sub>

</details>

## Reconciliation

W3C Reconciliation API: matching, judging, data extension. **6 of 6 done.**

<details><summary>6 done</summary>

- [x] **Reconciliation API client and the reconcile operation** — W3C Reconciliation API client: fetch a service manifest, batched queries (OpenRefine's batch size), type constraint, property constraints from other columns, limit, auto-match (OpenRefine's rule: top candidate matched when its score and the gap to the second qualify). The core/recon operation with StandardReconConfig JSON, same output cells as OpenRefine; long-running job with progress and cancel. Service list kept in preferences, with Wikidata as a preset; 'guess types of column'. The reconcile dialog: service, type, columns as properties, auto-match toggle. Tests run against a local mock service with fixtures from OpenRefine 3.10.1. <sub>v1.0 · P1 · L · OpenRefine: StandardReconConfig, ReconOperation, GuessTypesOfColumnCommand</sub>
- [x] **Matching UI** — Per-cell reconciliation UI as in OpenRefine: the candidate list with scores, 'match this cell', 'match all identical cells', 'create new item' and 'new for identical cells', matching to another item by id, discard and clear; matched cells link to the entity view URL. Backed by recon-judge-one-cell and recon-judge-similar-cells, with the same history entries. (Searching with the suggest API and the preview flyout: rc-suggest.) <sub>v1.0 · P1 · M · OpenRefine: ReconJudgeOneCellCommand, ReconJudgeSimilarCellsOperation</sub>
- [x] **Reconciliation facets** — Reconcile → Facets menu: by judgment, best candidate's score (range), best candidate's type match, best candidate's name edit distance, best candidate's name word similarity, best candidate's types; by judgment action and judgment timestamp. Same facet JSON in engineConfig, so recipes with these facets replay. <sub>v1.0 · P1 · M · OpenRefine: ReconJudgmentFacet, cell.recon.* expressions</sub>
- [x] **Add columns from reconciled values** — Data extension: propose properties from the service, preview, fetch in batches, add one column per property (extend-reconciled-data), multi-valued results as extra record rows, settings (limit, rank, references, count), values that are entities come back reconciled. Fixtures from OpenRefine against the mock service. <sub>v1.0 · P1 · L · OpenRefine: ExtendDataOperation, ReconciledDataExtensionJob</sub>
- [x] **Recons through every operation** — Fill down copies the Recon with the value (OpenRefine copies the whole cell); records-mode reconciling takes property values from the record's other rows (cellDependencies); check split, join, transpose and star/flag steps against OpenRefine fixtures. <sub>v0.5 · P2 · S · OpenRefine: FillDownOperation, RecordModel.CellDependency</sub>
- [x] **Search for match and entity preview** — In the matching panel: search the service with its suggest API (entity, type, property) instead of typing an id; the preview service in a flyout; the same suggest boxes in the reconcile dialog for types and properties. <sub>v1.0 · P2 · M · OpenRefine: suggest API, preview service, reconciliation manifest</sub>

</details>

## Desktop UI

Everything OpenRefine's UI does, in a modern, keyboard-first app. **16 of 18 done.**

- [ ] **Column menus for every operation** *(in progress)* — Each operation reachable from the column header like OpenRefine. <sub>v0.2 · P0 · M · OpenRefine: menu-edit-cells, menu-edit-column</sub>
- [ ] **Translations** — UI strings in message files, language picker, OpenRefine's translations (40+ languages from its Weblate project) mapped onto ours where the meaning matches, Russian checked by hand. <sub>v0.5 · P2 · M · OpenRefine: main/webapp/modules/core/langs</sub>

<details><summary>16 done</summary>

- [x] **Home: projects and create project** — Recent projects, import sources, drag-and-drop files. <sub>v0.2 · P0 · M · OpenRefine: index page</sub>
- [x] **Grid parity** — Rows/records toggle, inline cell edit, star/flag column, type-aware rendering, column widths, beyond-1M-row scrolling. <sub>v0.2 · P0 · M · OpenRefine: data-table-view</sub>
- [x] **Expression editor** — CodeMirror 6 with GREL highlighting, autocomplete with function docs, live preview, history and starred expressions. <sub>v0.2 · P0 · M · OpenRefine: expression-preview-dialog</sub>
- [x] **Command palette and shortcuts** — Ctrl+K to run any operation on the focused column. <sub>v0.5 · P1 · S · OpenRefine: new</sub>
- [x] **History panel parity** — Extract with step selection, apply from file, filter steps. <sub>v0.2 · P1 · S · OpenRefine: history-panel</sub>
- [x] **Long-running jobs** — Background processes with progress and cancel (imports, URL fetching, reconciliation). <sub>v0.2 · P0 · M · OpenRefine: get-processes, cancel-processes</sub>
- [x] **Themed scrollbars** — The grid scrollbars are the browser default light ones; style them with theme tokens in light and dark, wherever native scrolling is used. <sub>v0.2 · P1 · S · OpenRefine: new</sub>
- [x] **Preferences** — A preferences page like OpenRefine's: facet choice limit (ui.browsing.listFacet.limit), rows per page, reconciliation services, starred/history expressions, custom clustering functions, language; plus raw key/value editing for the rest. <sub>v0.5 · P2 · M · OpenRefine: PreferenceStore, preferences.vt</sub>
- [x] **Instrument UI design** — The Instrument design (design/, not committed): ruled sheet instead of cards, tokens in index.css, bars as cells, ink primary action, Geist Mono values; Cluster, Transform, Reconcile, History, Import and Export as full-window views with a breadcrumb and Esc; Palette and Jobs as overlays. Logic and API unchanged; data the engine lacks is left out and listed. <sub>v0.5 · P1 · L · OpenRefine: design/README.md</sub>
- [x] **Column profiles** — Per column: distinct and empty counts, the type most cells have, and a small distribution (top values for text, a histogram for numbers and dates, a marker for near-unique columns), for the grid headers of the Instrument design. <sub>v0.5 · P1 · M · OpenRefine: design/README.md</sub>
- [x] **Values that cluster, per column** — For each text column, how many values fall into clusters with the default keyer ("37 variants of 9 places"), and which cells, so the header links to Cluster and those cells get a dotted amber underline. <sub>v0.5 · P2 · M · OpenRefine: design/README.md</sub>
- [x] **Memory in the status bar** — The process's current and peak memory, for the status bar and the jobs panel. <sub>v0.5 · P2 · S · OpenRefine: design/README.md</sub>
- [x] **Job stages** — Jobs report their stages (read, parse, detect types, save) with their state and time, for the jobs panel. <sub>v0.5 · P2 · S · OpenRefine: design/README.md</sub>
- [x] **Transform preview by distinct value** — The transform preview over distinct values with their row counts: how many distinct values change and how many remain after. <sub>v0.5 · P2 · M · OpenRefine: design/README.md</sub>
- [x] **Reconciliation results by value** — One row per distinct value with its candidates and scores, and Match cell / Match all, for the Reconcile view. <sub>v0.5 · P2 · L · OpenRefine: design/README.md</sub>
- [x] **Project size and steps on Home** — Each project's history length and size on disk, for the project list. <sub>v0.5 · P2 · S · OpenRefine: design/README.md</sub>

</details>

## Compatibility & QA

Prove parity automatically against real OpenRefine. **7 of 7 done.**

<details><summary>7 done</summary>

- [x] **Operation conformance suite** — Run every operation through OpenRefine's API and ours on the same corpus and diff outputs cell by cell (extends compare.mjs). <sub>v0.2 · P0 · L · OpenRefine: benchmarks/compare.mjs</sub>
- [x] **GREL conformance suite** — Thousands of expressions evaluated in both engines, including errors and edge values. <sub>v0.2 · P0 · M · OpenRefine: preview-expression</sub>
- [x] **Messy-data test corpus** — Real-world files: encodings, ragged rows, embedded newlines, huge cells, mixed types. <sub>v0.2 · P0 · M · OpenRefine: tests/</sub>
- [x] **Fuzz importers and GREL parser** — Deterministic mutation tests damage real GREL expressions and importer inputs, record panics and slow cases, and run under cargo test. FUZZ_ITERS and FUZZ_SEED control longer reproducible runs; these are not cargo-fuzz targets. <sub>v0.5 · P2 · S · OpenRefine: crates/ox-core/tests/fuzz.rs</sub>
- [x] **CI on Windows, macOS, Linux** — GitHub Actions: build, tests, UI typecheck, conformance on a small corpus. <sub>v0.2 · P0 · S · OpenRefine: new</sub>
- [x] **Follow HTTP redirects** — Reconciliation services, fetch-URL steps and import from a web address follow redirects as OpenRefine's HttpClient does (301/302/303/307/308, POST kept on 307/308). Found on Nov 11: wikidata.reconci.link answers 307 and ureq refused it, so the Wikidata preset had to point at the final host. <sub>v0.5 · P1 · S · OpenRefine: HttpClient</sub>
- [x] **Real-project recipe conformance and failure reporting** — Expand the existing conformance suites with shareable real-world projects and reusable recipes. Check changed inputs, missing columns, types, records and extension dependencies; verify errors, partial application, saved state and restart behavior against OpenRefine 3.10.1 where applicable. <sub>v1.0 · P1 · M · OpenRefine: tests/ops, tests/orproject, project.rs; Tom's forum feedback</sub>

</details>

## Performance

Keep the 10x lead as features land; close the import gap. **6 of 6 done.**

<details><summary>6 done</summary>

- [x] **Parallel CSV import** — Chunked multi-threaded parsing plus a faster hasher; import is the smallest gap today (1.6x at 2M rows). <sub>v0.2 · P0 · M · OpenRefine: benchmark</sub>
- [x] **Shared history snapshots** — Share unchanged columns and row codes across history steps; one-to-one value renames reuse codes with a reordered dictionary. Steps that merge values or otherwise change codes still retain full column code arrays. Sparse code diffs are not implemented; measure long histories in perf-cardinality-history. <sub>v0.5 · P1 · M · OpenRefine: project.rs</sub>
- [x] **Datasets larger than RAM** — Memory-mapped column files and spill-to-disk for 50M+ rows. <sub>later · P2 · L · OpenRefine: new</sub>
- [x] **Benchmark tracking** — Run the comparison on every release and publish the chart, and catch regressions: on Nov 11 a hand check against the first prototype found the list facet 6x slower (per-row string keys), peak RAM up 70% from saving (a JSON tree of the whole table) and export (the whole CSV in memory). Keep a baseline results.json, an OxRefine-only fast mode for CI, and fail on per-step slowdowns and memory growth beyond noise. <sub>v0.5 · P1 · S · OpenRefine: benchmarks/compare.mjs, benchmarks/report.mjs</sub>
- [x] **Import memory** — After the Nov 11 fixes peak RAM at 1M rows is 141 MB against the prototype's 122; the remainder is probably the parallel import's batches and per-worker dictionaries. Measure per step, trim without losing the 4x import speed-up. <sub>v0.5 · P2 · S · OpenRefine: benchmarks/compare.mjs</sub>
- [x] **Benchmark unique values and long editing histories** — Extend performance coverage beyond the repeated-value benchmark: low, mixed and high cardinality, row-dependent GREL, value-merging edits and long histories. Measure time and peak memory for import, transforms, undo/redo, save/reopen and export; publish dataset-specific claims and reproducible baselines. <sub>v1.0 · P2 · M · OpenRefine: benchmarks/gen.mjs, benchmarks/check.mjs, project.rs</sub>

</details>

## Release

Installers, docs and a public launch. **5 of 9 done.**

- [ ] **Installers and auto-update** *(in progress)* — Tauri bundles: msi, dmg, AppImage/deb; code signing; updater. <sub>v0.5 · P1 · M · OpenRefine: tauri bundle</sub>
- [ ] **OpenRefine HTTP API compatibility** *(in progress)* — Serve OpenRefine's /command/core/* (create-project-from-upload, apply-operations, export-rows, get-models, get-rows, CSRF token, …) so existing clients (openrefine-client, refine-python, R and Ruby wrappers) work unchanged. <sub>later · P2 · L · OpenRefine: openrefine-client, CommandServlet</sub>
- [ ] **Launch** (review) — README demo GIF, benchmark chart, Show HN / Reddit r/datacleaning / OpenRefine forum post. <sub>v0.5 · P1 · S · OpenRefine: new</sub>
- [ ] **Check the Docker image** — The Dockerfile from rel-server was never built (Docker Desktop wasn't running). Build and run it, check the token link, uploads and downloads, then build it in CI. <sub>v0.5 · P2 · S · OpenRefine: Dockerfile</sub>

<details><summary>5 done</summary>

- [x] **Public repository** — git init, license (MIT/Apache-2.0), contributing guide, issue templates; credit OpenRefine (BSD-3). <sub>v0.2 · P0 · S · OpenRefine: new</sub>
- [x] **Headless server mode** — ox serve with the UI embedded, for servers and Docker, like running OpenRefine remotely. <sub>v0.5 · P1 · S · OpenRefine: refine -i 0.0.0.0</sub>
- [x] **Docs site and migration guide** — Feature map from OpenRefine, GREL reference, what is not supported yet. <sub>v0.5 · P1 · M · OpenRefine: new</sub>
- [x] **Browser sessions and safe handover between computers** — Support the browser workflow requested by Carlo: projects hosted on Linux, reopened from another computer, with independent active-project selection and protection against stale edits. Define shared-project refresh/conflict behavior and verify with two browser contexts. Document the trusted-token access model. <sub>v1.0 · P1 · L · OpenRefine: ox-cli/main.rs, ui/src/api.ts, ui/src/Grid.tsx; Carlo's forum feedback</sub>
- [x] **Reconcile Kanzo and the generated roadmap safely** — Define and verify synchronization between Kanzo, roadmap/tasks.json and ROADMAP.md, including direct board edits, newly created cards and stable IDs. Preview conflicts and preserve owner-only done decisions; document the authority and supported workflow instead of overwriting differences silently. <sub>v1.0 · P2 · S · OpenRefine: tools/kanzo.mjs, roadmap/tasks.json, roadmap/build.mjs, CLAUDE.md</sub>

</details>

## Extensions

OpenRefine's bundled extensions: database, Wikibase, scripting. **4 of 10 done.**

- [ ] **Wikibase schema and upload** (review) — Wikidata/Wikibase extension: manifests, schema editor (items, statements, qualifiers, references, terms), issues panel, preview, upload edits with OAuth or bot password, export to QuickStatements, schema saved in the project. Needs reconciliation. <sub>later · P2 · L · OpenRefine: extensions/wikibase</sub>
- [ ] **Google Sheets and Drive** (review) — OpenRefine's gdata extension (no longer bundled with 3.10.1): import from Google Sheets (by URL or from the signed-in Drive), export to a new Google Sheet, upload the project archive to Drive; OAuth sign-in with a client the user registers (Google issues clients per app). <sub>later · P2 · L · OpenRefine: extensions/gdata</sub>
- [ ] **Extensions** (review) — A way to add GREL functions, importers, exporters and operations without rebuilding OxRefine (OpenRefine uses Butterfly modules): Python extensions in <data>/extensions, a documented API (docs/src/extensions.md) and one example extension (docs/extensions/text-tools). <sub>later · P2 · L · OpenRefine: Butterfly modules, ClientSideResourceManager</sub>
- [ ] **Run OpenRefine Java extension steps in an installed OpenRefine** (review) — OpenRefine extensions are Java modules OxRefine cannot load. Bridge: the project goes as an OpenRefine archive to a headless OpenRefine from the folder in Preferences (openrefine.home), the steps run there with its extensions, and the result comes back as a new project with OpenRefine history. Command palette: Run steps in OpenRefine…; test against a local OpenRefine 3.10.1 (skipped without one). <sub>later · P2 · M · OpenRefine: ox-core/src/openrefine.rs, tools/openrefine.mjs; card ext-plugins</sub>
- [ ] **MCP without ox serve, over HTTP and with more tools** (review) — `ox mcp` starts its own host when no server runs for its data folder, and finds a running `ox serve` for that folder without --url; one data folder has one host. `ox serve` answers MCP Streamable HTTP at /mcp. New tools: rename and delete projects, import from a web address or pasted text, preview a file import, guess reconciliation types, list GREL functions and installed extensions; Python and Clojure in expression previews. <sub>v1.0 · P1 · L · OpenRefine: ox-cli/src/mcp.rs, ox-cli/src/main.rs; modelcontextprotocol.io Streamable HTTP</sub>
- [ ] **Investigate database working tables shared with SQL tools** — Study Carlo's proposal for a separate database working table used by OxRefine and SQL tools while leaving source tables untouched. Define external-edit detection, transactions, conflict handling and the effect on undo/redo; produce a design and a bounded feasibility experiment before committing to implementation. <sub>later · P2 · M · OpenRefine: extensions/database; Carlo's forum feedback</sub>

<details><summary>4 done</summary>

- [x] **Database import** — OpenRefine's database extension: PostgreSQL, MySQL, MariaDB and SQLite connections (saved, with test), schema browser, SQL query editor, preview, import of the query result. <sub>later · P2 · L · OpenRefine: extensions/database</sub>
- [x] **PC-Axis import** — Statistical PC-Axis (.px) files: one row per data point with its dimension values, as OpenRefine's pc-axis extension. <sub>later · P2 · S · OpenRefine: extensions/pc-axis</sub>
- [x] **Wikimedia Commons import** — OpenRefine's commons extension: import files from Commons categories (with depth), file names reconciled to M-ids, the wikitext of each file page as an option. <sub>later · P2 · M · OpenRefine: extensions/commons</sub>
- [x] **MCP server for AI assistants** — `ox mcp`: a Model Context Protocol server (stdio) over the same commands as `ox serve`, so AI assistants can list, import and open projects, read column profiles, rows and facet counts, cluster, preview GREL, apply operations or recipes, undo and export. Every change goes into the project history; file access stays inside the data folder and the given paths. Docs for Claude Desktop and Claude Code. <sub>v1.0 · P1 · L · OpenRefine: no counterpart; the protocol is at modelcontextprotocol.io</sub>

</details>
