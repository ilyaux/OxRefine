# Exporting

**Export** in the top bar (or <kbd>E</kbd>) opens the export view. Pick a format on the left and set its options; the preview on the right shows the first matching rows. **Save** (<kbd>Ctrl S</kbd>) writes the rows that pass your facets, or every row when no facet is set. The view shows how many rows match and how many there are in all; <kbd>Esc</kbd> closes it.

| Format | What you get |
|---|---|
| **CSV / TSV** | Comma- or tab-separated text, as OpenRefine writes it |
| **Custom tabular** | Your choice of ordered columns, separator, line endings, quoting, headers and blank rows |
| **HTML table** | A standalone HTML page with the table |
| **Excel** (`.xlsx`) | A worksheet named after the project, with numbers, dates and booleans typed |
| **Excel 97** (`.xls`) | The same in the older Excel format, for software that needs it: at most 65,536 rows and 256 columns, and text cut at 32,767 characters a cell, as OpenRefine does |
| **OpenDocument** (`.ods`) | The same as an OpenDocument spreadsheet |
| **Templating** | Any text format from a prefix, a row template, a separator and a suffix, for example JSON |
| **SQL** | `CREATE TABLE` and `INSERT` statements |
| **PostgreSQL table** | A new table in a PostgreSQL database: the matching rows, or only what changed since the import, for a correlated `UPDATE`; see [below](#postgresql-table) |
| **OpenRefine project** | The whole project with its history, which OpenRefine opens; see [Moving projects and recipes](moving-over.md) |
| **Recipe JSON** | The project's steps as an OpenRefine recipe; **Recipe** in the top bar picks steps and copies the JSON |
| **Google Sheets** | The matching rows as a new spreadsheet in your Google Drive, which opens when it's ready ([sign-in](importing.md#google-sheets-and-drive)) |
| **Google Drive** | The project as an OpenRefine archive in your Google Drive |
| **QuickStatements** | For a project with a Wikibase schema: the schema applied to the matching rows, as commands for the QuickStatements tool, as OpenRefine's Wikibase extension writes them |
| **Wikibase schema** | The project's Wikibase schema as JSON, which OpenRefine's **Wikibase → Import schema** reads |

Formats from installed [extensions](extensions.md) follow these in the list.

The SQL exporter differs from OpenRefine 3.10 on purpose. OpenRefine leaves numbers and nulls out of `INSERT` rows, which shifts the columns, while OxRefine writes them so the dump loads. See [compatibility checks](compatibility.md) for format coverage, and the encoding policy below for another deliberate difference.

CSV and TSV skip rows containing only null cells by default. Empty text and whitespace still count as values. Custom tabular's **Blank rows** option lets you retain null-only rows. A single selected column that renders as an empty physical line is suppressed, as in OpenRefine; **Quote every cell** can preserve it as quoted empty text. In the API, omitting `options` skips null-only rows; providing an options object defaults `outputBlankRows` to true unless explicitly set.

## PostgreSQL table

**PostgreSQL table** writes into a new table in a PostgreSQL database, over a connection saved in **New project → Database** (see [database queries](importing.md#database-queries)). Enter the password there; it's used for this export only and never saved.

- **Only `_ox` tables.** OxRefine writes only tables whose name ends in `_ox`: lowercase letters, digits and `_`, optionally after a schema (`staging.people_ox`). It never changes any other table. An existing `_ox` table of that name is replaced only when **Replace the table if it exists** is ticked.
- **One transaction.** If anything fails, the database is left as it was.
- **A preview first.** The preview shows the `CREATE TABLE` and the first rows before anything is written.

There are two shapes:

- **Changes only.** Pick the key column: one that came with the import and identifies each row. The table gets the key, the columns that differ from the imported values in some matching row, and only those rows. That's what a correlated `UPDATE` needs:

  ```sql
  UPDATE people SET name = c.name
  FROM people_ox c
  WHERE people.id = c.id;
  ```

  The key must keep its imported values, and each key can be on only one row; otherwise nothing is written and the message says which row.
- **Whole table.** Every column of the rows that pass your facets.

Each column gets one type:

| Values in the column | PostgreSQL type |
|---|---|
| Whole numbers | `bigint` |
| Other numbers | `double precision` |
| Booleans | `boolean` |
| Dates | `timestamptz` |
| Anything else, or a mix | `text` |

Null stays `NULL`, and empty text stays empty text, as on import. MySQL, MariaDB and SQLite can come later; tell us in the [issue](https://github.com/ilyaux/OxRefine/issues/7) if you need one.

## Columns, dates and reconciliation

CSV, TSV, Custom tabular, HTML, Excel and OpenDocument share the column controls. Select the columns to include and use their up/down buttons to set the output order. **Column options** edits each selected column's date format and whether reconciled cells use the matched entity name, its ID or the original cell content. **Blank unmatched cells** replaces unmatched reconciliation results with blanks.

Dates offer ISO 8601, English short/medium/long/full formats and a custom pattern. Supported patterns use `yy`/`yyyy`, `M` through `MMMM`, `d`/`dd`, `E` through `EEEE`, `H`/`HH`, `h`/`hh`, `m`/`mm`, `s`/`ss`, `SSS`, `a` and simple quoted text. Unsupported tokens, timezone tokens and escaped apostrophes produce an error before the destination file is opened. **Use server local time** uses the machine running the engine, which can differ from the browser's timezone. **Omit time** exports just the date for the built-in formats.

Workbooks retain original numbers, booleans and default dates, including reconciled cells, as OpenRefine does. Choose CSV or HTML when you need matched entity text from those typed cells. Choosing a custom date format, omitted time or local time exports workbook dates as text. HTML can link matched entities using the service's saved page template, falling back to the identifier URI when no template is known. Workbook hyperlinks are not offered.

OxRefine applies the requested date settings. The tested OpenRefine 3.10.1 custom exporter ignores some of its date controls and writes the original date instead; this difference is [recorded separately](differences.md#custom-export-date-settings).

## Reusing export settings

Enter an **Export preset name** and choose **Save preset**. **Load saved settings** restores the format, column order, per-column formatting and encoding options; **Update preset** replaces the named preset and **Remove preset** deletes it. Presets belong to the current engine's preferences and can be reused on other projects. They store settings and column names, without row values or filters. Missing columns are reported when applying a preset, and an empty selection prevents saving a file.

## Text encoding and BOM

CSV, TSV and Custom tabular offer UTF-8, UTF-16LE/BE, Windows-1251/1252, ISO-8859-1 and US-ASCII. UTF-8 without a byte-order mark (BOM) remains the default. For Unicode encodings, **Include byte-order mark (BOM)** adds the mark once at the start of the file. The preview displays the decoded text with that transport mark hidden.

Legacy character sets cannot represent every character. By default an export rejects unrepresentable characters before opening the destination file. **Replace unsupported characters with ?** explicitly permits replacement, as OpenRefine does. The preview checks its first ten matching rows; saving validates all exported rows, so a character outside the preview can still prevent export. Save waits for the current preview after an option or filter change.

In the JSON API and MCP export parameters, `encoding`, `bom` and `encodingErrors` are top-level fields beside the existing `options`. `encodingErrors` accepts `"error"` (the default) or `"replace"`. The API also accepts `"UTF-16"` for big-endian UTF-16 with Java's automatic BOM; an explicit `bom` setting overrides it. BOM settings apply to Unicode encodings.

Delimited text is written to the file as it is produced. Workbook exports materialize their output and selected custom columns in memory.
