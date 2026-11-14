# Reusing ecological recipes

These tests use the header and first eight rows of two real Portal Project Teaching Database files at the revision recorded in [sources.json](sources.json). The data is CC0. The small scientific-name and measurement recipes were authored for these tests; they are representative examples, not histories donated by users. Schema changes, blanks, invalid measurements and record continuations are deliberate variants in `cases.mjs`.

Regenerate the expected typed tables and history with an installed OpenRefine **3.10.1**:

```sh
node tests/recipes/gen.mjs
```

This uses the existing `tools/openrefine.mjs` client with an isolated workspace on port **3339**. No reconciliation service or Wikidata write is called. The Wikibase case only saves an empty local schema. `typed-input.openrefine.tar.gz` is the input with OpenRefine's type guessing enabled.

Run the integration checks without Java:

```sh
cargo test --locked -p ox-cli --test recipes
```

The same reference fixtures are used through `ox apply` and the HTTP dispatcher. Cells retain their types, row order, flags and stars; as in the operations suite, error presence is compared rather than Java/Rust error wording. Tests restart the server, reopen saved projects, and undo/redo completed steps. A local blocked URL request makes cancellation deterministic.

Recipe JSON is validated before any step runs. Unsupported operations, including the bundled Wikibase extension's operation, reject the whole recipe with a step number. A supported operation that fails while executing leaves earlier completed steps in the server project and saves them. Jython is rejected explicitly; its output is not compared as compatible. CLI failures leave the input and any existing output untouched and do not export a partial result.

The OpenRefine generator records each operation separately so that intermediate states and the first failing step are observable, and also records the whole-batch result. OpenRefine rejects the missing/renamed-column and unknown-function batches before applying any steps; OxRefine keeps the earlier completed step. [known_failures.txt](known_failures.txt) records these three differences. This corpus complements the existing conformance suites; it is not broad validation of arbitrary production recipes or legacy extensions.
