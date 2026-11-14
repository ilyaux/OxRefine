# Differences and what's missing

## Known differences

OxRefine is tested against fixtures produced by a real OpenRefine 3.10.1 (see [How compatibility is checked](compatibility.md)). Of the 2,136 checks, 14 differ. Each is listed below with its reason. Some are deliberate, because OpenRefine's behaviour there looks like a bug.

| Area | What differs | Why |
|---|---|---|
| Sorting | With blanks sorted first, OpenRefine places one non-blank row before the blank rows | Looks like a comparator bug in OpenRefine; OxRefine puts all blanks first |
| SQL export | OpenRefine drops numeric and null values from `INSERT` rows, which shifts the columns | Deliberate: OxRefine writes `NULL` and unquoted numbers, so the dump loads |
| ODS import | OpenRefine adds an empty row for the filler row LibreOffice writes at the end of a sheet (2 cases) | Deliberate: OxRefine leaves it out |
| JSON import | OpenRefine can't read integers beyond 64 bits and creates no project | Deliberate: OxRefine imports them as numbers |
| `detectLanguage()` | OxRefine doesn't identify languages yet (2 cases) | Needs a language-identification model |
| Beider-Morse keys | Names with more than 20 phonetic alternatives can get a different key (7 cases) | Apache Commons Codec truncates its list while building it, and keeps a different 20 |

Everything else checked matches exactly. That covers GREL results and errors, operations, facet counts, import options, exports, clustering, reconciliation, and project archives with their full history.

## Not in OxRefine yet

- **Import:** databases (PostgreSQL, MySQL, MariaDB, SQLite), RDF (N-Triples, Turtle, RDF/XML, JSON-LD), MARC, wikitext tables, PC-Axis, Google Sheets
- **Facets:** the scatterplot facet
- **Clustering:** custom clustering functions
- **Expressions:** Python (Jython) and Clojure; `detectLanguage()`; `wikibaseIssues()`
- **Export:** Google Sheets
- **Extensions:** Wikibase schemas and uploads, Wikimedia Commons, OpenRefine's extension modules
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
