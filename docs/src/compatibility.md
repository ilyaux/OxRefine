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
