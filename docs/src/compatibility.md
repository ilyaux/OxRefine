# How compatibility is checked

OxRefine's behaviour is pinned to OpenRefine 3.10.1. Nothing is matched by guesswork: each area has a test suite, and each case in it is run through a real OpenRefine 3.10.1 to record what OpenRefine does.

| Area | Cases matching OpenRefine |
|---|---|
| GREL expressions | 559 / 561 |
| Operations | 83 / 84 |
| Facets | 39 / 39 |
| Import options | 43 / 45 |
| Import from the clipboard and web addresses | 18 / 18 |
| Messy-data corpus: encodings, line endings, ragged rows, huge cells | 26 / 27 |
| Exports | 16 / 17 |
| Clustering and GREL `phonetic()` | 1,152 / 1,159 |
| Reconciliation | 49 / 49 |
| OpenRefine project archives, read, replayed and written back, metadata included | 137 / 137 |

## How a suite works

Each suite in the repository's `tests/<suite>/` folder has three files:

- **`cases.mjs`:** the cases, such as expressions, operation lists, files with import options, or projects to build.
- **`gen.mjs`:** starts OpenRefine 3.10.1, runs every case through its HTTP API, and writes `fixtures.json` with exactly what OpenRefine produced.
- **`known_failures.txt`:** the cases where OxRefine differs, each with its reason (see [Differences](differences.md)).

`cargo test` runs every case through OxRefine and compares the results. A test fails when a new difference appears, and also when a known one starts passing, so the list only shrinks.

The additional customized-facet, JSON-path and export-encoding fixtures record the official 3.10.1 release archive's SHA-256 and the hashes of the relevant JAR files. They also retain the version actually reported by its API: the checksum-verified Windows release reports `3.10-SNAPSHOT [TRUNK]`. Its release identity is verified from the artifact hashes rather than inferred from that string. These extra suites do not change the fixture total in the table above.

The additional custom-export corpus in `tests/export/custom-fixtures.json` compares ordered columns, headers, reconciliation output and workbook cell values with the official release. Requested date formatting and preserving the time of day in ODS have explicit [documented differences](differences.md#custom-export-date-settings). These fixtures are separate from the totals above.

## Reusable recipes

The additional `tests/recipes/` corpus uses small CC0 excerpts from the Portal Project Teaching Database. The data's source revision, license and selections are recorded in `sources.json`; the cleanup recipes were authored for these tests. `gen.mjs` records typed tables, history and per-step errors from OpenRefine 3.10.1.

The CLI and HTTP integration tests reuse these fixtures on the original and modified input: renamed or missing columns, blanks, invalid values, numeric types, records mode and extension dependencies. They check the state after failure or cancellation, restart the server, reopen the saved project and exercise undo/redo. Jython and Wikibase operations run in the reference but are explicitly rejected by OxRefine; this corpus does not change the compatibility totals above. See [recipe reuse and partial application](moving-over.md#reusing-a-recipe-on-another-file).

## Both directions

For project archives, the check runs both ways:

1. **Reading:** OpenRefine builds a project and exports it, and OxRefine must reproduce OpenRefine's table at every step of the undo history.
2. **Replaying:** OxRefine must redo every step itself and reach the same tables.
3. **Writing back:** OxRefine writes the project out. A script then opens it in OpenRefine and compares every undo step there.

## Reconciliation without the internet

The reconciliation suite runs a small local reconciliation service, records every query OpenRefine sends with the answer it gets, and replays those answers to OxRefine. The test therefore covers the exact queries, batching, auto-matching rules and error messages, with no network access.

## Damaged input

A fuzz test takes the suites' own GREL expressions and sample files of every format, damages them at random, and feeds them through the GREL parser and evaluator and every importer. Any crash fails the test. Each change runs a short session, and long sessions of millions of inputs run before releases. Inputs that once crashed are kept as regression cases. Expressions are limited to 128 levels of nesting or 512 chained operators, so no expression can overflow the stack.

## Speed

A separate benchmark runs the same workflow through both tools; see the [README](https://github.com/ilyaux/OxRefine#benchmark). A regression check runs on every change: it builds the previous version and the new one on the same machine, and fails if a step got clearly slower or uses clearly more memory.

The optional cardinality profile extends that same regression harness with low, mixed and high uniqueness, row-dependent transforms, merging edits and long histories. It checks every row after undo and saved-project reopening and records per-phase times and cumulative process memory. These are local measurements, separate from the people workload in CI and from the OpenRefine timing comparison. Commands and measurement limits are in [benchmarks/README.md](https://github.com/ilyaux/OxRefine/blob/main/benchmarks/README.md).
