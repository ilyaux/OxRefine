# Where everything is

OxRefine follows OpenRefine's layout: projects on a home screen, a grid of rows, facets and history in a side panel, and each column's operations in its menu. This page lists where each OpenRefine feature lives in OxRefine. **Ctrl+K** (⌘K on a Mac) opens a command palette that finds any action by name. The tools also have keys, shown next to them: <kbd>F</kbd> facet, <kbd>C</kbd> cluster, <kbd>T</kbd> transform and <kbd>R</kbd> reconcile on the column you last used, <kbd>E</kbd> export, <kbd>I</kbd> or <kbd>Ctrl O</kbd> import, <kbd>Esc</kbd> to close a view.

## Projects

| OpenRefine | OxRefine |
|---|---|
| Create project → This computer | **From a file or folder** on the home screen, or **Import → File…** in a project |
| Create project → Web addresses (URLs) | **From a web address** on the home screen, or **Import → Web address…** |
| Create project → Clipboard | **Paste from clipboard** on the home screen, or **Import → Paste data…** |
| Create project → Database | **From a database** on the home screen: saved connections to SQLite, PostgreSQL, MySQL and MariaDB, the schema, and a SQL query with a preview |
| Create project → Google Data | **Google Sheets** on the home screen: a shared link, or sheets in your Google Drive after signing in |
| Wikimedia Commons extension: import a category | **Wikimedia Commons** on the home screen: category files matched to Commons M-ids |
| Open project | Home screen, the project list |
| Import project (`.openrefine.tar.gz`) | **Open an OpenRefine project** on the home screen, or **Import → OpenRefine project…**; it opens with its undo history |
| Export → OpenRefine project archive | **Export → OpenRefine project**; OpenRefine opens it with OxRefine's history |
| Rename and delete projects | Home screen, the project's buttons |
| Project tags and metadata | **About** next to the project in the list: tags, description, creator and the rest; click a tag at the top to show only its projects. OpenRefine project archives carry them both ways |
| Preferences | The gear at the top right: theme, the facet choice limit, reconciliation services, recent expressions, extensions, and every preference by name and value |

## Rows and records

| OpenRefine | OxRefine |
|---|---|
| Show as rows / records | **Rows** / **Records** at the right of the bar above the grid |
| Star and flag a row | The star and flag in each row |
| All → Edit rows → Star / Flag / Remove matching rows | **Rows** in the top bar |
| Keep only matching rows | **Rows → Keep only matching rows** |
| Sort | **Sort rows by this column…** in the column menu |
| Remove duplicate rows | **Remove duplicate rows by this column** in the column menu |

## Facets and filters

| OpenRefine | OxRefine (column menu → Filter) |
|---|---|
| Facet → Text facet / Numeric facet / Timeline facet | Same names |
| Facet → Custom text facet… / Custom numeric facet… | Same names |
| Facet → Customized facets → word, duplicates, text length, Unicode, blank, error, star, flag | **Customized facets**, same entries |
| Facet → Scatterplot facet | **Scatterplot facet…**, linear axes only (see [differences](differences.md#scatterplot-facets)) |
| Text filter | **Text filter** |
| Reconcile → Facets | **Reconcile → Facets** |

In a list facet, click a choice to include it, and hover a choice and click the pencil to change that value in every row. Each filter shows as a chip above the grid; **Clear all** there clears every filter, and the × on a facet removes it.

## Editing cells

| OpenRefine (column menu → Edit cells) | OxRefine (column menu → Edit cells) |
|---|---|
| Transform… | **Transform…**: GREL with a live preview and expression history |
| Common transforms | **Common transforms**: trim, collapse whitespace, case, to number, date, text or blank |
| Fill down / Blank down | Same names |
| Split multi-valued cells… / Join multi-valued cells… | Same names |
| Cluster and edit… | **Cluster and edit…**, with your own keying and distance functions |
| Edit one cell | Click the cell and press Enter (or double-click it, or use the pencil); it can also change every identical cell |

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
| Undo / Redo tab | **History** tab in the side panel, undo and redo in the top bar, and **Recipe** for the full history beside its recipe |
| Undo / Redo → Extract… | **Recipe** in the top bar: tick the steps, then copy or save the JSON; also **History → Extract…** |
| Undo / Redo → Apply… | **Recipe → Apply a recipe…** |

## Export

| OpenRefine (Export) | OxRefine (Export) |
|---|---|
| Tab-separated / Comma-separated values | **TSV…** / **CSV…** |
| HTML table / Excel (.xlsx) / ODF spreadsheet | **HTML table…** / **Excel (.xlsx)…** / **OpenDocument (.ods)…** |
| Excel 97 (.xls) | **Export → Excel 97** |
| Custom tabular / SQL / Templating | **Custom tabular…** / **SQL…** / **Templating…** |
| Export project | **OpenRefine project…** |
| — (OpenRefine writes no database tables) | **PostgreSQL table**: the matching rows, or only the changes since the import, into a new `*_ox` table; see [Exporting](exporting.md#postgresql-table) |
| Google Sheets / Google Drive (the Google Data extension) | **Google Sheets** / **Google Drive**: the matching rows as a new sheet, or the project archive in your Drive |

## Expressions

| OpenRefine | OxRefine |
|---|---|
| GREL | GREL, every function; see the [reference](grel.md) |
| Python / Jython, Clojure | The language switch in the expression editor; see [Python and Clojure](transforming.md#python-and-clojure) |
| Expression history and starred expressions | History of recent expressions in the editor |

## Extensions

| OpenRefine | OxRefine |
|---|---|
| Extensions (Java modules in the `extensions` folder) | Python extensions in the `extensions` folder of the data folder; **Preferences → Extensions** lists them. OpenRefine's Java modules don't load, but their steps run in an OpenRefine you install: **Run steps in OpenRefine…**; see [Extensions](extensions.md#openrefines-java-extensions) |
| Database, Google Data, Jython, phonetic and PC-Axis extensions | Built in: the import sources above, the expression language switch, the clustering keyers and the PC-Axis importer |
| Wikibase extension: schema | A project's schema from an OpenRefine project or a recipe step (`wikidata/save-wikibase-schema`) is kept with its history, undone and redone, and written back to OpenRefine archives |
| Wikibase → Export to QuickStatements / Export schema | **Export → QuickStatements** / **Wikibase schema**, for a project with a schema |
| Wikibase → Edit Wikibase schema | **Edit Wikibase schema…** in the command palette (<kbd>Ctrl K</kbd>): items and media info with terms and statements (qualifiers, references, modes) from columns or values, property search on the Wikibase, the edits as QuickStatements and their issues |
| Wikibase → Upload edits to Wikibase | The **Upload** tab of the schema editor: log in with a bot password or an owner-only OAuth consumer, then upload; see [Wikibase and Wikidata](wikibase.md) |

## Server and API

| OpenRefine | OxRefine |
|---|---|
| `refine -i 0.0.0.0` | `ox serve --host 0.0.0.0`, behind an access token |
| HTTP API (`/command/core/...`) | OxRefine's own JSON API; OpenRefine's API is not implemented yet |
| Batch use through the API or client libraries | `ox apply recipe.json input -o output` |
