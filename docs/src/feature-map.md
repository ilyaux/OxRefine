# Where everything is

OxRefine follows OpenRefine's layout: projects on a home screen, a grid of rows, facets and history in a side panel, and each column's operations in its menu. This page lists where each OpenRefine feature lives in OxRefine. **Ctrl+K** (⌘K on a Mac) opens a command palette that finds any action by name.

## Projects

| OpenRefine | OxRefine |
|---|---|
| Create project → This computer | **Import → File…**, or a folder or archive |
| Create project → Web addresses (URLs) | **Import → Web address…** |
| Create project → Clipboard | **Import → Paste data…** |
| Create project → Database | Not yet |
| Open project | Home screen, the project list |
| Import project (`.openrefine.tar.gz`) | **Import → OpenRefine project…**; it opens with its undo history |
| Export → OpenRefine project archive | **Export → OpenRefine project…**; OpenRefine opens it with OxRefine's history |
| Rename and delete projects | Home screen, the project's buttons |
| Project tags and metadata | **About** next to the project in the list: tags, description, creator and the rest; click a tag at the top to show only its projects. OpenRefine project archives carry them both ways |
| Preferences | The gear at the top right: theme, the facet choice limit, reconciliation services, recent expressions, and every preference by name and value |

## Rows and records

| OpenRefine | OxRefine |
|---|---|
| Show as rows / records | **Rows** / **Records** at the bottom of the grid |
| Star and flag a row | The star and flag in each row |
| All → Edit rows → Star / Flag / Remove matching rows | **Rows** in the toolbar |
| Keep only matching rows | **Rows → Keep only matching rows** |
| Sort | **Sort rows by this column…** in the column menu |
| Remove duplicate rows | **Remove duplicate rows by this column** in the column menu |

## Facets and filters

| OpenRefine | OxRefine (column menu → Filter) |
|---|---|
| Facet → Text facet / Numeric facet / Timeline facet | Same names |
| Facet → Custom text facet… / Custom numeric facet… | Same names |
| Facet → Customized facets → word, duplicates, text length, Unicode, blank, error, star, flag | **Customized facets**, same entries |
| Facet → Scatterplot facet | Not yet |
| Text filter | **Text filter** |
| Reconcile → Facets | **Reconcile → Facets** |

In a list facet, click a choice to include it, and hover a choice and click the pencil to change that value in every row. **Remove all facets** clears them all.

## Editing cells

| OpenRefine (column menu → Edit cells) | OxRefine (column menu → Edit cells) |
|---|---|
| Transform… | **Transform…**: GREL with a live preview and expression history |
| Common transforms | **Common transforms**: trim, collapse whitespace, case, to number, date, text or blank |
| Fill down / Blank down | Same names |
| Split multi-valued cells… / Join multi-valued cells… | Same names |
| Cluster and edit… | **Cluster and edit…** |
| Edit one cell | Hover the cell and click the pencil; it can also change every identical cell |

## Editing columns

| OpenRefine (column menu → Edit column) | OxRefine (column menu → Edit column) |
|---|---|
| Split into several columns… | Same name |
| Add column based on this column… | **Add column based on this…** |
| Add column by fetching URLs… | Same name |
| Add columns from reconciled values… | Same name |
| Rename / Remove / Move this column | **Rename…**, **Remove column**, **Move column** |
| Transpose → Cells across columns into rows / Cells in rows into columns / Columnize by key/value columns | **Transpose**, same three entries |

## Reconciliation

| OpenRefine | OxRefine |
|---|---|
| Reconcile → Start reconciling… | **Reconcile → Start reconciling…**: services, type guessing, columns as properties |
| Reconcile → Actions (match to best candidate, create new items, match all filtered cells, discard, clear, copy across columns, use values as identifiers, add entity identifiers column) | **Reconcile → Actions**, same entries |
| Judging a cell: candidates, match this cell or all identical cells, new item, search for match | Click the mark after a reconciled cell |
| Add columns from reconciled values… | Column menu → **Edit column** |

## History and recipes

| OpenRefine | OxRefine |
|---|---|
| Undo / Redo tab | **History** tab in the side panel, plus undo and redo in the toolbar |
| Undo / Redo → Extract… | **History → Extract…**, or **Ctrl+K → Extract recipe…** |
| Undo / Redo → Apply… | **Apply recipe** in the toolbar |

## Export

| OpenRefine (Export) | OxRefine (Export) |
|---|---|
| Tab-separated / Comma-separated values | **TSV…** / **CSV…** |
| HTML table / Excel (.xlsx) / ODF spreadsheet | **HTML table…** / **Excel (.xlsx)…** / **OpenDocument (.ods)…** |
| Excel 97 (.xls) | Not yet |
| Custom tabular / SQL / Templating | **Custom tabular…** / **SQL…** / **Templating…** |
| Export project | **OpenRefine project…** |

## Expressions

| OpenRefine | OxRefine |
|---|---|
| GREL | GREL, every function except `wikibaseIssues`; see the [reference](grel.md) |
| Python / Jython, Clojure | Not yet |
| Expression history and starred expressions | History of recent expressions in the editor |

## Server and API

| OpenRefine | OxRefine |
|---|---|
| `refine -i 0.0.0.0` | `ox serve --host 0.0.0.0`, behind an access token |
| HTTP API (`/command/core/...`) | OxRefine's own JSON API; OpenRefine's API is not implemented yet |
| Batch use through the API or client libraries | `ox apply recipe.json input -o output` |
