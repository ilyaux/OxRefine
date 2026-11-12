# OxRefine roadmap: full OpenRefine parity in Rust

Generated from `roadmap/tasks.json` by `node roadmap/build.mjs`. 91 of 127 tasks done.

Milestones: **v0.2** is a daily driver for cleaning CSV/Excel/JSON (persistence, full GREL core, all common operations, parity tests). **v0.5** covers the remaining formats, clustering methods and exporters plus installers and launch. **v1.0** adds reconciliation and full UI parity. **later** lists what only a minority of OpenRefine users need.

| Milestone | Tasks | Done |
|---|---|---|
| v0.1 | 10 | 10 |
| v0.2 | 46 | 39 |
| v0.5 | 50 | 36 |
| v1.0 | 7 | 6 |
| later | 14 | 0 |

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
- [x] **Records mode** · v0.2 · P1 · L. Rows grouped into records by the key column; record-based engineConfig, record-aware facets and ops. _OpenRefine: mode: record-based_
- [x] **Row stars and flags** · v0.2 · P1 · S. Per-row star/flag bits, filterable and preserved in history. _OpenRefine: row-star, row-flag_
- [x] **Save and reopen projects** · v0.2 · P0 · L. Project files on disk (columns + history), autosave, crash-safe writes; reopening a 1M-row project in under a second. _OpenRefine: workspace, ProjectManager_
- [x] **Project library** · v0.2 · P1 · M. Open/rename/delete/tag projects, recent list, metadata like OpenRefine's project index. _OpenRefine: get-all-project-metadata_
- [x] **Open OpenRefine project archives** · v0.5 · P1 · L. Read .openrefine.tar.gz including history, so users can migrate existing projects in one step. _OpenRefine: import-project_
- [x] **Reconciliation data in the cell model** · v0.5 · P1 · M. Every cell can carry a Recon: id, service, identifierSpace, schemaSpace, judgment (none / matched / new), judgmentAction, judgmentHistoryEntry, match (id, name, types, score), candidates (ranked), features (typeMatch, nameMatch, nameLevenshtein, nameWordDistance), matchRank. Stored beside the dictionary codes, not in them. GREL sees cell.recon.* (judgment, match, best, candidates, features, service…). Recon data reads from and writes to OpenRefine project archives (the recon pool in data.txt and change files) instead of being dropped, and survives operations that move cells (column copy, move, reorder, rows remove/reorder, fill down/blank down, records). _OpenRefine: Recon, ReconCandidate, ReconStats, Pool_
- [x] **Project metadata and tags** · v0.5 · P2 · M. OpenRefine's project metadata: tags, description, creator, contributors, subject, license, homepage, image, title, version, custom metadata fields; editable from the home page, projects filtered by tag, read from and written to archive metadata.json (tags and fields are read today but not shown). _OpenRefine: ProjectMetadata, SetProjectMetadataCommand, SetProjectTagsCommand_

## Import

Every format OpenRefine imports, with its options and preview.

- [x] **CSV options parity** · v0.2 · P0 · M. Encoding detection, quote char, ignore/skip lines, header lines, limit, trim, guess types, column names on/off. _OpenRefine: text/line-based/*sv options_
- [x] **Excel xlsx/xls** · v0.2 · P0 · M. Sheet selection, multiple sheets into one project, dates as dates (calamine). _OpenRefine: binary/text/xml/xls/xlsx_
- [x] **OpenDocument spreadsheet** · v0.5 · P2 · S. ODS through the same sheet importer. _OpenRefine: text/xml/ods_
- [ ] **JSON and JSON Lines** · v0.2 · P0 · L. Pick the record path in a tree preview; nested fields become columns; JSONL streams. _OpenRefine: text/json_
- [x] **XML** · v0.5 · P1 · L. Pick the record element in a tree preview, attributes and nested elements as columns. _OpenRefine: text/xml_
- [ ] **Fixed-width text** · v0.5 · P2 · M. Column widths with a draggable ruler in preview. _OpenRefine: text/line-based/fixed-width_
- [x] **Line-based text** · v0.5 · P2 · S. One line per row, lines-per-row grouping. _OpenRefine: text/line-based_
- [ ] **RDF (N-Triples, N3, Turtle, RDF/XML, JSON-LD)** · later · P2 · L. OpenRefine's RDF importers: one row per subject, one column per predicate, multiple values as record rows. Formats: RDF/N3, N-Triples, Turtle, RDF/XML, JSON-LD. Fixtures from OpenRefine. _OpenRefine: RdfTripleImporter, RdfXmlTripleImporter, RdfJsonldTripleImporter_
- [ ] **MARC records** · later · P2 · M. MARC (ISO 2709) import: converted to MARCXML and read with the XML importer, as OpenRefine does. _OpenRefine: MarcImporter_
- [ ] **Wikitext tables** · later · P2 · M. Import tables from MediaWiki markup: table choice, headers, rowspan/colspan, links as cells (optionally reconciled to Wikidata), references, blank-cell handling. Same options as OpenRefine. _OpenRefine: WikitextImporter_
- [x] **Import from URL and clipboard** · v0.2 · P1 · S. Download with progress and paste-as-table, like OpenRefine's Web Addresses and Clipboard tabs. _OpenRefine: create-project-from-upload_
- [x] **Zip/gzip and multi-file import** · v0.5 · P2 · S. Pick files inside archives, add a file-name column. _OpenRefine: includeFileSources, includeArchiveFileName_
- [x] **Import preview wizard** · v0.2 · P0 · M. Live preview of the first rows while options change, format auto-detect. _OpenRefine: parsing-ui_
- [x] **bzip2 archives** · v0.5 · P2 · S. Import .bz2 and .tar.bz2 like OpenRefine (it reads gzip, bzip2, zip and tar), in the wizard and from URLs. _OpenRefine: ImportingUtilities.tryOpenAsCompressedFile_

## Export

Every exporter OpenRefine ships.

- [ ] **CSV/TSV/custom separator export** · v0.2 · P0 · S. Separator, quoting, line endings, BOM, filtered rows only. _OpenRefine: csv, tsv, *sv_
- [x] **Excel export** · v0.2 · P0 · M. xlsx with typed cells (rust_xlsxwriter); legacy xls only if users ask. _OpenRefine: xlsx, xls_
- [x] **ODS export** · v0.5 · P2 · M. OpenDocument spreadsheet writer. _OpenRefine: ods_
- [x] **HTML table export** · v0.5 · P2 · S. Plain HTML table of the filtered rows. _OpenRefine: html_
- [x] **Templating exporter** · v0.5 · P1 · M. Prefix / row template / separator / suffix with GREL inside {{ }}; the usual way to produce JSON. _OpenRefine: template_
- [x] **SQL exporter** · v0.5 · P2 · M. CREATE TABLE + INSERT statements with column types and options. _OpenRefine: sql_
- [ ] **Custom tabular exporter** · v0.5 · P1 · M. Choose and order columns, format dates and recon values, save as a reusable preset. _OpenRefine: custom-tabular-exporter_
- [x] **Export project archive** · v0.5 · P2 · M. Write .openrefine.tar.gz that OpenRefine can open, so moving back is possible. _OpenRefine: export-project_
- [x] **Excel 97 (.xls) export** · v0.5 · P2 · M. OpenRefine exports both .xls (BIFF8) and .xlsx. Write .xls with typed cells, the sheet named after the project, OpenRefine's cell limits (32767 chars, 65536 rows). _OpenRefine: XlsExporter (xlsx=false)_

## Operations

All 37 OpenRefine operations, same JSON, same results.

- [x] **Column move, reorder, multi-removal** · v0.2 · P0 · S. column-move(-left/-right/-first/-last), column-reorder, column-multi-removal. _OpenRefine: column-move*, column-reorder, column-multi-removal_
- [x] **Split column** · v0.2 · P0 · M. By separator, regex or field lengths; guess types; remove original. _OpenRefine: column-split_
- [x] **Split / join multi-valued cells** · v0.2 · P0 · M. Explode cells into rows of a record and back. _OpenRefine: multivalued-cell-split, multivalued-cell-join_
- [x] **Fill down / blank down** · v0.2 · P0 · S. Row- and record-aware. _OpenRefine: fill-down, blank-down_
- [x] **Transpose and key-value columnize** · v0.5 · P1 · L. Columns into rows, rows into columns, key/value pairs into columns. _OpenRefine: transpose-columns-into-rows, transpose-rows-into-columns, key-value-columnize_
- [x] **Row operations** · v0.2 · P0 · M. Remove matching rows, keep matched, add rows, remove duplicates by key columns. _OpenRefine: row-removal, row-keep-matched, row-addition, row-duplicate-removal_
- [x] **Sort and reorder rows permanently** · v0.2 · P0 · M. Multi-column sort criteria by type, then make it permanent. _OpenRefine: row-reorder_
- [x] **Edit a single cell** · v0.2 · P0 · S. Inline edit with type, recorded in history (and in exported histories where OpenRefine records it). _OpenRefine: edit-one-cell_
- [x] **Add column by fetching URLs** · v0.5 · P1 · L. Async with throttle delay, custom headers, cache, retry and progress; cancellable. _OpenRefine: column-addition-by-fetching-urls_
- [x] **onError and repeat semantics** · v0.2 · P0 · S. keep-original, set-to-blank, store-error exactly as OpenRefine once GREL can error. _OpenRefine: onError_
- [x] **Reconciliation operations** · v1.0 · P1 · M. The operations around reconciliation: match each cell to its best candidate, create a new item for each cell, match all filtered cells to a chosen item, discard judgments, clear reconciliation data, copy reconciliation data to other columns, use values as identifiers, add a column of matched entity identifiers/names. Same operation JSON and results as OpenRefine 3.10.1, checked by fixtures. _OpenRefine: ReconMatchBestCandidatesOperation, ReconMarkNewTopicsOperation, ReconDiscardJudgmentsOperation, ReconClearSimilarCellsOperation, ReconCopyAcrossColumnsOperation, ReconUseValuesAsIdentifiersOperation, ReconMatchSpecificTopicOperation_

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
- [x] **cross() across projects** · v0.5 · P1 · M. Lookup into another open project by key column, with an index cache. _OpenRefine: Cross_
- [ ] **Python and Clojure expressions** · later · P2 · L. Expressions in Jython (Python 2.7 semantics) and Clojure with value, cell, cells, row, rowIndex bound as in OpenRefine; selectable in the expression editor and recipes ('jython:', 'clojure:' prefixes). _OpenRefine: JythonEvaluable, ClojureEvaluable_
- [ ] **detectLanguage()** · v0.5 · P2 · M. GREL detectLanguage(s): ISO 639-1 code from OpenRefine's language-detector n-gram profiles, the same answers on the fixture texts (currently 2 known failures). _OpenRefine: DetectLanguage, optimaize language-detector_
- [x] **GREL record variable** · v0.5 · P2 · S. In records mode OpenRefine's expressions see the current record: record.index, record.fromRowIndex, record.toRowIndex, record.rowCount, record.cells["Name"].value (an array of the record's values). OxRefine's GREL returns null for record today (found while writing the GREL reference). Fixtures from OpenRefine for each field, in transforms, new columns and facets. _OpenRefine: WrappedRecord, RecordModel_

## Facets

Every facet type, including customized and expression facets.

- [ ] **Numeric range facet** · v0.2 · P0 · M. Histogram, draggable range, numeric/non-numeric/blank/error toggles. _OpenRefine: range_
- [ ] **Timeline facet** · v0.5 · P1 · M. Date histogram with range selection. _OpenRefine: timerange_
- [ ] **Scatterplot facet** · later · P2 · M. Facet → Scatterplot matrix of numeric columns, then a scatterplot facet on a pair: linear/log scales, rotation, dot size, rectangle selection that filters rows; facet JSON (scatterplot, from_x/to_x/from_y/to_y) as in OpenRefine. _OpenRefine: ScatterplotFacet, GetScatterplotCommand_
- [x] **Custom facets by expression** · v0.2 · P0 · M. List and range facets over any GREL expression, needs the full evaluator. _OpenRefine: list facet expression_
- [ ] **Customized facets menu** · v0.2 · P1 · M. Word, duplicates, text length, Unicode chars, blank, error, star and flag facets. _OpenRefine: Customized facets_
- [ ] **Facet details** · v0.2 · P1 · S. Sort by name/count, include/exclude, edit a choice in place, choice limit, collapse. _OpenRefine: list-facet.js_

## Clustering

All key-collision keyers and nearest-neighbour methods.

- [x] **Phonetic keyers** · v0.5 · P1 · M. Metaphone3, Cologne phonetic, Daitch-Mokotoff, Beider-Morse. _OpenRefine: binning keyers_
- [x] **Nearest-neighbour clustering** · v0.5 · P1 · L. Levenshtein distance with n-gram blocking, radius and block size; parallel, with progress and cancel. _OpenRefine: knn: levenshtein_
- [x] **PPM distance for nearest neighbours** · v0.5 · P2 · S. Prediction by Partial Matching distance, as OpenRefine computes it with its bundled arithmetic coder (needs permission to download the reference code). 2 known failures today. _OpenRefine: PPMDistance, arithcode_
- [x] **Cluster dialog parity** · v0.5 · P2 · S. Filter clusters by size, row count, value length and spread; export clusters as JSON. _OpenRefine: clustering-dialog_
- [ ] **Custom clustering functions** · later · P2 · M. OpenRefine 3.8+ lets users add key-collision keyers and kNN distances written in GREL/Python, saved in preferences and listed in the cluster dialog. _OpenRefine: ClusteringFunctionsCommand, preferences: ui.clustering.customKeyingFunctions_

## Reconciliation

W3C Reconciliation API: matching, judging, data extension.

- [x] **Reconciliation API client and the reconcile operation** · v1.0 · P1 · L. W3C Reconciliation API client: fetch a service manifest, batched queries (OpenRefine's batch size), type constraint, property constraints from other columns, limit, auto-match (OpenRefine's rule: top candidate matched when its score and the gap to the second qualify). The core/recon operation with StandardReconConfig JSON, same output cells as OpenRefine; long-running job with progress and cancel. Service list kept in preferences, with Wikidata as a preset; 'guess types of column'. The reconcile dialog: service, type, columns as properties, auto-match toggle. Tests run against a local mock service with fixtures from OpenRefine 3.10.1. _OpenRefine: StandardReconConfig, ReconOperation, GuessTypesOfColumnCommand_
- [x] **Matching UI** · v1.0 · P1 · M. Per-cell reconciliation UI as in OpenRefine: the candidate list with scores, 'match this cell', 'match all identical cells', 'create new item' and 'new for identical cells', matching to another item by id, discard and clear; matched cells link to the entity view URL. Backed by recon-judge-one-cell and recon-judge-similar-cells, with the same history entries. (Searching with the suggest API and the preview flyout: rc-suggest.) _OpenRefine: ReconJudgeOneCellCommand, ReconJudgeSimilarCellsOperation_
- [x] **Reconciliation facets** · v1.0 · P1 · M. Reconcile → Facets menu: by judgment, best candidate's score (range), best candidate's type match, best candidate's name edit distance, best candidate's name word similarity, best candidate's types; by judgment action and judgment timestamp. Same facet JSON in engineConfig, so recipes with these facets replay. _OpenRefine: ReconJudgmentFacet, cell.recon.* expressions_
- [x] **Add columns from reconciled values** · v1.0 · P1 · L. Data extension: propose properties from the service, preview, fetch in batches, add one column per property (extend-reconciled-data), multi-valued results as extra record rows, settings (limit, rank, references, count), values that are entities come back reconciled. Fixtures from OpenRefine against the mock service. _OpenRefine: ExtendDataOperation, ReconciledDataExtensionJob_
- [x] **Recons through every operation** · v0.5 · P2 · S. Fill down copies the Recon with the value (OpenRefine copies the whole cell); records-mode reconciling takes property values from the record's other rows (cellDependencies); check split, join, transpose and star/flag steps against OpenRefine fixtures. _OpenRefine: FillDownOperation, RecordModel.CellDependency_
- [x] **Search for match and entity preview** · v1.0 · P2 · M. In the matching panel: search the service with its suggest API (entity, type, property) instead of typing an id; the preview service in a flyout; the same suggest boxes in the reconcile dialog for types and properties. _OpenRefine: suggest API, preview service, reconciliation manifest_

## Desktop UI

Everything OpenRefine's UI does, in a modern, keyboard-first app.

- [ ] **Home: projects and create project** · v0.2 · P0 · M. Recent projects, import sources, drag-and-drop files. _OpenRefine: index page_
- [x] **Grid parity** · v0.2 · P0 · M. Rows/records toggle, inline cell edit, star/flag column, type-aware rendering, column widths, beyond-1M-row scrolling. _OpenRefine: data-table-view_
- [x] **Expression editor** · v0.2 · P0 · M. CodeMirror 6 with GREL highlighting, autocomplete with function docs, live preview, history and starred expressions. _OpenRefine: expression-preview-dialog_
- [~] **Column menus for every operation** · v0.2 · P0 · M. Each operation reachable from the column header like OpenRefine. _OpenRefine: menu-edit-cells, menu-edit-column_
- [x] **Command palette and shortcuts** · v0.5 · P1 · S. Ctrl+K to run any operation on the focused column. _OpenRefine: new_
- [x] **History panel parity** · v0.2 · P1 · S. Extract with step selection, apply from file, filter steps. _OpenRefine: history-panel_
- [x] **Long-running jobs** · v0.2 · P0 · M. Background processes with progress and cancel (imports, URL fetching, reconciliation). _OpenRefine: get-processes, cancel-processes_
- [x] **Themed scrollbars** · v0.2 · P1 · S. The grid scrollbars are the browser default light ones; style them with theme tokens in light and dark, wherever native scrolling is used. _OpenRefine: new_
- [ ] **Translations** · v0.5 · P2 · M. UI strings in message files, language picker, OpenRefine's translations (40+ languages from its Weblate project) mapped onto ours where the meaning matches, Russian checked by hand. _OpenRefine: main/webapp/modules/core/langs_
- [x] **Preferences** · v0.5 · P2 · M. A preferences page like OpenRefine's: facet choice limit (ui.browsing.listFacet.limit), rows per page, reconciliation services, starred/history expressions, custom clustering functions, language; plus raw key/value editing for the rest. _OpenRefine: PreferenceStore, preferences.vt_
- [x] **Instrument UI design** · v0.5 · P1 · L. The Instrument design (design/, not committed): ruled sheet instead of cards, tokens in index.css, bars as cells, ink primary action, Geist Mono values; Cluster, Transform, Reconcile, History, Import and Export as full-window views with a breadcrumb and Esc; Palette and Jobs as overlays. Logic and API unchanged; data the engine lacks is left out and listed. _OpenRefine: design/README.md_
- [x] **Column profiles** · v0.5 · P1 · M. Per column: distinct and empty counts, the type most cells have, and a small distribution (top values for text, a histogram for numbers and dates, a marker for near-unique columns), for the grid headers of the Instrument design. _OpenRefine: design/README.md_
- [ ] **Values that cluster, per column** · v0.5 · P2 · M. For each text column, how many values fall into clusters with the default keyer ("37 variants of 9 places"), and which cells, so the header links to Cluster and those cells get a dotted amber underline. _OpenRefine: design/README.md_
- [ ] **Memory in the status bar** · v0.5 · P2 · S. The process's current and peak memory, for the status bar and the jobs panel. _OpenRefine: design/README.md_
- [ ] **Job stages** · v0.5 · P2 · S. Jobs report their stages (read, parse, detect types, save) with their state and time, for the jobs panel. _OpenRefine: design/README.md_
- [ ] **Transform preview by distinct value** · v0.5 · P2 · M. The transform preview over distinct values with their row counts: how many distinct values change and how many remain after. _OpenRefine: design/README.md_
- [ ] **Reconciliation results by value** · v0.5 · P2 · L. One row per distinct value with its candidates and scores, and Match cell / Match all, for the Reconcile view. _OpenRefine: design/README.md_
- [ ] **Project size and steps on Home** · v0.5 · P2 · S. Each project's history length and size on disk, for the project list. _OpenRefine: design/README.md_

## Compatibility & QA

Prove parity automatically against real OpenRefine.

- [x] **Operation conformance suite** · v0.2 · P0 · L. Run every operation through OpenRefine's API and ours on the same corpus and diff outputs cell by cell (extends compare.mjs). _OpenRefine: benchmarks/compare.mjs_
- [x] **GREL conformance suite** · v0.2 · P0 · M. Thousands of expressions evaluated in both engines, including errors and edge values. _OpenRefine: preview-expression_
- [x] **Messy-data test corpus** · v0.2 · P0 · M. Real-world files: encodings, ragged rows, embedded newlines, huge cells, mixed types. _OpenRefine: tests/_
- [x] **Fuzz importers and GREL parser** · v0.5 · P2 · S. cargo-fuzz targets for every parser. _OpenRefine: new_
- [x] **CI on Windows, macOS, Linux** · v0.2 · P0 · S. GitHub Actions: build, tests, UI typecheck, conformance on a small corpus. _OpenRefine: new_
- [x] **Follow HTTP redirects** · v0.5 · P1 · S. Reconciliation services, fetch-URL steps and import from a web address follow redirects as OpenRefine's HttpClient does (301/302/303/307/308, POST kept on 307/308). Found on Nov 11: wikidata.reconci.link answers 307 and ureq refused it, so the Wikidata preset had to point at the final host. _OpenRefine: HttpClient_

## Performance

Keep the 10x lead as features land; close the import gap.

- [x] **Parallel CSV import** · v0.2 · P0 · M. Chunked multi-threaded parsing plus a faster hasher; import is the smallest gap today (1.6x at 2M rows). _OpenRefine: benchmark_
- [x] **History as diffs** · v0.5 · P1 · M. Store changed codes instead of full column snapshots so long histories on big files stay small. _OpenRefine: project.rs_
- [ ] **Datasets larger than RAM** · later · P2 · L. Memory-mapped column files and spill-to-disk for 50M+ rows. _OpenRefine: new_
- [x] **Benchmark tracking** · v0.5 · P1 · S. Run the comparison on every release and publish the chart, and catch regressions: on Nov 11 a hand check against the first prototype found the list facet 6x slower (per-row string keys), peak RAM up 70% from saving (a JSON tree of the whole table) and export (the whole CSV in memory). Keep a baseline results.json, an OxRefine-only fast mode for CI, and fail on per-step slowdowns and memory growth beyond noise. _OpenRefine: benchmarks/compare.mjs, benchmarks/report.mjs_
- [x] **Import memory** · v0.5 · P2 · S. After the Nov 11 fixes peak RAM at 1M rows is 141 MB against the prototype's 122; the remainder is probably the parallel import's batches and per-worker dictionaries. Measure per step, trim without losing the 4x import speed-up. _OpenRefine: benchmarks/compare.mjs_

## Release

Installers, docs and a public launch.

- [x] **Public repository** · v0.2 · P0 · S. git init, license (MIT/Apache-2.0), contributing guide, issue templates; credit OpenRefine (BSD-3). _OpenRefine: new_
- [ ] **Installers and auto-update** · v0.5 · P1 · M. Tauri bundles: msi, dmg, AppImage/deb; code signing; updater. _OpenRefine: tauri bundle_
- [x] **Headless server mode** · v0.5 · P1 · S. ox serve with the UI embedded, for servers and Docker, like running OpenRefine remotely. _OpenRefine: refine -i 0.0.0.0_
- [ ] **OpenRefine HTTP API compatibility** · later · P2 · L. Serve OpenRefine's /command/core/* (create-project-from-upload, apply-operations, export-rows, get-models, get-rows, CSRF token, …) so existing clients (openrefine-client, refine-python, R and Ruby wrappers) work unchanged. _OpenRefine: openrefine-client, CommandServlet_
- [x] **Docs site and migration guide** · v0.5 · P1 · M. Feature map from OpenRefine, GREL reference, what is not supported yet. _OpenRefine: new_
- [~] **Launch** · v0.5 · P1 · S. README demo GIF, benchmark chart, Show HN / Reddit r/datacleaning / OpenRefine forum post. _OpenRefine: new_
- [ ] **Check the Docker image** · v0.5 · P2 · S. The Dockerfile from rel-server was never built (Docker Desktop wasn't running). Build and run it, check the token link, uploads and downloads, then build it in CI. _OpenRefine: Dockerfile_

## Extensions

OpenRefine's bundled extensions: database, Wikibase, scripting.

- [ ] **Database import** · later · P2 · L. OpenRefine's database extension: PostgreSQL, MySQL, MariaDB and SQLite connections (saved, with test), schema browser, SQL query editor, preview, import of the query result. _OpenRefine: extensions/database_
- [ ] **Wikibase schema and upload** · later · P2 · L. Wikidata/Wikibase extension: manifests, schema editor (items, statements, qualifiers, references, terms), issues panel, preview, upload edits with OAuth or bot password, export to QuickStatements, schema saved in the project. Needs reconciliation. _OpenRefine: extensions/wikibase_
- [ ] **PC-Axis import** · later · P2 · S. Statistical PC-Axis (.px) files: one row per data point with its dimension values, as OpenRefine's pc-axis extension. _OpenRefine: extensions/pc-axis_
- [ ] **Wikimedia Commons import** · later · P2 · M. OpenRefine's commons extension: import files from Commons categories (with depth), file names reconciled to M-ids, the wikitext of each file page as an option. _OpenRefine: extensions/commons_
- [ ] **Google Sheets and Drive** · later · P2 · L. OpenRefine's gdata extension: import from Google Sheets (by URL or from the signed-in Drive), export to a new Google Sheet, upload the project archive to Drive; OAuth sign-in. _OpenRefine: extensions/gdata_
- [ ] **Extensions** · later · P2 · L. A way to add GREL functions, importers, exporters and operations without rebuilding OxRefine (OpenRefine uses Butterfly modules); a documented API and one example extension. _OpenRefine: Butterfly modules, ClientSideResourceManager_
- [ ] **MCP server for AI assistants** · v1.0 · P1 · L. `ox mcp`: a Model Context Protocol server (stdio) over the same commands as `ox serve`, so AI assistants can list, import and open projects, read column profiles, rows and facet counts, cluster, preview GREL, apply operations or recipes, undo and export. Every change goes into the project history; file access stays inside the data folder and the given paths. Docs for Claude Desktop and Claude Code. _OpenRefine: no counterpart; the protocol is at modelcontextprotocol.io_
