# Differences and what's missing

## Known differences

OxRefine is tested against fixtures produced by a real OpenRefine 3.10.1 (see [How compatibility is checked](compatibility.md)). Of the 2,136 checks, 12 differ. Each is listed below with its reason. Some are deliberate, because OpenRefine's behaviour there looks like a bug.

| Area | What differs | Why |
|---|---|---|
| Sorting | With blanks sorted first, OpenRefine places one non-blank row before the blank rows | Looks like a comparator bug in OpenRefine; OxRefine puts all blanks first |
| SQL export | OpenRefine drops numeric and null values from `INSERT` rows, which shifts the columns | Deliberate: OxRefine writes `NULL` and unquoted numbers, so the dump loads |
| ODS import | OpenRefine adds an empty row for the filler row LibreOffice writes at the end of a sheet (2 cases) | Deliberate: OxRefine leaves it out |
| JSON import | OpenRefine can't read integers beyond 64 bits and creates no project | Deliberate: OxRefine imports them as numbers |
| Beider-Morse keys | Names with more than 20 phonetic alternatives can get a different key (7 cases) | Apache Commons Codec truncates its list while building it, and keeps a different 20 |

The other cases in that fixture total match. They cover GREL results and errors, operations, facet counts, import options, exports, clustering, reconciliation, and project archives with their full history.

### Recipe validation and partial failures

The additional recipe-reuse tests expose a separate difference: OpenRefine 3.10.1 validates missing column references and unknown expression functions before running the batch. OxRefine validates the JSON operation list first, then resolves columns and expressions as each operation runs. In the tested two-step recipes, OpenRefine applies no steps, while OxRefine keeps and saves the successful first step before reporting the second step's error. The error reports how many steps remain applied; undo them to revert the partial result. These scenarios are outside the 2,136-check total above.

### Fixed-width character positions

Fixed-width import and its ruler count Unicode code points. OpenRefine 3.10.1 counts Java UTF-16 code units. In the additional wizard-reference tests, padding after an emoji produces widths `[3, 4, 1]` in OpenRefine and `[2, 4, 1]` in OxRefine. ASCII and basic multilingual plane examples match. This difference is recorded separately in `tests/import/fixed-width-guesses.json`, outside the 2,136-check total above. Check the import preview before sharing widths for text containing supplementary characters.

### JSON Lines limits and export encoding errors

Additional JSON Lines reference cases show that OpenRefine 3.10.1 can import more sequential root values than its requested record limit. OxRefine applies a global record limit and stops reading once it is reached. Its limited preview therefore does not validate malformed data after that point; a full import still reports it. The reference outputs and declared differences are recorded separately in `tests/import/json-paths-fixtures.json`.

For legacy text export encodings, OpenRefine replaces unrepresentable characters with `?`. OxRefine rejects them by default before opening the output file; explicitly selecting replacement reproduces that behavior. The additional byte references in `tests/export/encoding-fixtures.json` are outside the 2,136-check total above, as are the JSON Lines cases.

### Custom export date settings

The additional `tests/export/custom-fixtures.json` cases show OpenRefine 3.10.1 ignoring requested custom date settings in text, HTML and workbook exports. OxRefine applies the supported requested format instead. Default workbook dates remain typed; nondefault date formatting produces text cells. These cases are separate from the 2,136-check total. See [export options](exporting.md#columns-dates-and-reconciliation) for the supported patterns and typed-cell reconciliation behavior.

The same corpus records OpenRefine's ODS exporter dropping the time of day from a date cell. OxRefine retains its timestamp, including milliseconds. The remaining cells, selected columns and ordering are still compared with the reference.

### Commons category imports

The separate CommonsExtension v0.1.5 can emit duplicate file rows from overlapping categories and repeats category traversal. OxRefine preserves direct-files-first depth-first ordering, deduplicates by file page ID and tracks visited categories with the remaining depth. It follows empty batches that still have continuation and fails the entire preparation on an API error or a limit, keeping partial data out of the project library. Loading file-page wikitext during preparation is an OxRefine addition. These behaviors and the extension mapping references are outside the 2,136-check total.

## Not in OxRefine yet

- **Import:** databases (PostgreSQL, MySQL, MariaDB, SQLite), RDF (N-Triples, Turtle, RDF/XML, JSON-LD), MARC, wikitext tables, PC-Axis, Google Sheets
- **Facets:** the scatterplot facet
- **Clustering:** custom clustering functions
- **Expressions:** Python (Jython) and Clojure; `wikibaseIssues()`
- **Export:** Google Sheets
- **Extensions:** Wikibase schemas and uploads, Commons-specific GREL functions, OpenRefine's extension modules
- **Interface:** translations (the interface is in English)
- **Server:** OpenRefine's HTTP API, which some client libraries use; OxRefine has its own JSON API, and `ox apply` covers batch use

The full plan is in [ROADMAP.md](https://github.com/ilyaux/OxRefine/blob/main/ROADMAP.md).

Additional extension data, including Wikibase schemas, is preserved as part of the original archive. Editing that data is not supported. Projects carrying it have conservative export restrictions to prevent loss or stale references; see [moving projects](moving-over.md#projects-with-additional-extension-data).

## Things that work differently on purpose

- **Speed and memory.** OxRefine keeps each distinct value of a column once and works on distinct values, so facets, clustering and most transforms take milliseconds even on millions of rows.
- **History is kept compactly.** Undo works the same way, but steps that only rename values share the previous step's data instead of copying it.
- **One program for every setup.** The desktop app, the browser app (`ox serve`), the server mode and the command line run the same engine.
- **Metadata OpenRefine doesn't keep.** OpenRefine 3.10.1 accepts a project's license, homepage and image but doesn't store them; OxRefine stores them and writes them into OpenRefine project archives, which OpenRefine then shows.
- **Errors instead of crashes.** Where OpenRefine fails with an out-of-memory or stack-overflow error, OxRefine returns a GREL error and carries on. This covers `range()` and `forRange()` past 20 million elements, `factn()` with a step of zero or less, an expression nested more than 128 levels deep or chaining more than 512 operators, and a few huge `multinomial()` and `factn()` arguments.
