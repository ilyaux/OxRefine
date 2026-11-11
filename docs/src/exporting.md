# Exporting

**Export** in the toolbar writes the rows that pass your facets, or every row when no facet is set.

| Format | What you get |
|---|---|
| **CSV… / TSV…** | Comma- or tab-separated text, as OpenRefine writes it |
| **Custom tabular…** | Your choice of columns, separator, quoting, headers and blank rows |
| **HTML table…** | A standalone HTML page with the table |
| **Excel (.xlsx)…** | A worksheet named after the project, with numbers, dates and booleans typed |
| **OpenDocument (.ods)…** | The same as an OpenDocument spreadsheet |
| **Templating…** | Any text format from a prefix, a row template, a separator and a suffix, for example JSON |
| **SQL…** | `CREATE TABLE` and `INSERT` statements |
| **OpenRefine project…** | The whole project with its history, which OpenRefine opens; see [Moving projects and recipes](moving-over.md) |
| **Recipe JSON… / Copy recipe JSON** | The project's steps as an OpenRefine recipe |

The SQL exporter differs from OpenRefine 3.10 on purpose. OpenRefine leaves numbers and nulls out of `INSERT` rows, which shifts the columns, while OxRefine writes them so the dump loads. Every other exporter matches OpenRefine's output.

Exports are written straight to the file as they are produced, so even very large exports use little memory.
