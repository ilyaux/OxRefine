# Importing data

## Sources

- **A file:** CSV and TSV (any separator), fixed-width text, Excel (`.xlsx`, `.xls`), OpenDocument (`.ods`), JSON and JSON Lines, XML, MARC (`.mrc`, `.marc`), wikitext tables (`.wiki`, `.wikitext`), PC-Axis (`.px`), or plain lines.
- **A folder or an archive:** zip, tar, gzip and bzip2 archives (`.tar.gz`, `.tar.bz2`), or a folder of files. Choose which files to import. Optional columns record the file and archive each row came from.
- **The clipboard:** **Paste from clipboard** on the home screen (or **Import → Paste data…** in a project) takes rows copied from a spreadsheet, CSV text, or plain lines.
- **A web address:** **From a web address** (or **Import → Web address…**) downloads the file and opens the same preview.
- **Wikimedia Commons:** **Wikimedia Commons** on Home or in the Import menu loads filenames from categories, with optional file-page wikitext. See the Commons workflow below.
- **An OpenRefine project:** see [Moving projects and recipes](moving-over.md).

On the home screen, drop one file onto the window to open the same import preview. The desktop app also accepts one folder. Browser folder drops and multiple files are rejected with a message; use the file/folder picker or an archive instead. Dropping an OpenRefine project archive asks for confirmation before importing it with its history. File drops while a project or another view is open do not replace the current work.

## The preview

OxRefine detects the format and opens the import view: what it detected and the project name at the top, the options on the left, and the first rows as they will be read on the right, with the time they took to parse. **Create project** (<kbd>Ctrl ↵</kbd>) imports the file; <kbd>Esc</kbd> cancels. The options match OpenRefine's:

- **Delimited text:** separator, encoding, header lines, lines to skip, quote handling, blank rows and cells, trimming, guessing cell types.
- **Fixed-width:** column widths, an editable ruler and guessed boundaries from a sample.
- **Excel and ODS:** which sheets to import, and header rows.
- **JSON, XML and MARC:** the element that holds one record, chosen from a list.
- **Wikitext:** tables, headers, spanning cells, raw templates, references and optional link reconciliation.
- **PC-Axis:** encoding, data rows to skip and row limit.

The import options follow OpenRefine's wizard. For JSON, OxRefine also suggests a record path so nested data has a useful first preview.

### JSON and JSON Lines

Pick the object or array that holds your records from the record-path list. Paths show their observed count and example fields. A bounded sample is labelled as a sample; its counts describe the part inspected, rather than a total for the file. Nested arrays can expand one selected record into several table rows. An empty preview explains that the source may be empty or the selected path may not match, and prevents creating an empty project by mistake.

The manual path accepts a JSON array of field names, so commas, spaces and underscores in names can be preserved exactly. `_` names the anonymous root, and another `_` selects array items. For `{"data":{"items":[{"sku":"A-1"}]}}`, select `["_", "data", "items", "_"]`.

JSON Lines imports decode and parse one line at a time, appending cells to the table's column dictionaries. The table and the largest current record still need memory. New imports use a path relative to each line's root, starting with `["_"]`. Older OxRefine settings retain their virtual-array path semantics: the usual `["_", "_"]` path streams, while the rare legacy paths `[]` and `["_"]` still load the entire virtual array to preserve their original row alignment.

A limited JSON Lines preview stops after the requested number of records; it does not validate the rest of the source. A later malformed line can therefore fail a full import after its preview succeeded.

### Fixed-width text

Choose **Fixed-width** for text with aligned columns. OxRefine suggests boundaries from a sample when possible; files without consistent spaces may have no suggested boundaries. Add, move or remove the ruler's boundaries to change the preview, or edit **Column widths** directly. **Guess widths** reapplies the suggested boundaries. Invalid widths show an error and prevent project creation until corrected. The remaining text after the last width forms one more column.

The guess examines up to 100 nonempty lines and 65,536 Unicode characters; the ruler displays up to 20 lines and 512 characters per line. Its display marks truncated samples. The ruler and widths count Unicode code points rather than bytes or visual text width. OpenRefine counts UTF-16 code units, so widths can differ for characters outside the basic multilingual plane, including many emoji. Check the preview when reusing those widths between the two tools.

### MARC records

MARC files are converted from ISO 2709 to MARCXML and passed through the XML importer. The preview suggests the record element; its attributes, control fields, data fields and repeated subfields become columns and expanded rows. The record leader and MARC-8 escape designations select character decoding automatically. Conversion and XML parsing keep the document in memory.

Malformed records fail with an error before a project is created. OpenRefine's permissive Marc4j reader can repair some damaged records and guess additional legacy encodings; those repairs are not a promise of this importer. See [differences](differences.md) for the tested scope.

### Wikitext tables

Wikitext imports the table markup itself; it does not render a wiki page or expand templates. Choose all tables or a single table from **Tables to import**. All tables is the reference behavior; choosing a table is an OxRefine addition. The JSON API accepts multiple zero-based `tableIndexes`; an empty list means all tables.

Header lines, spanning-cell handling, raw template text, reference extraction, blank rows and blank columns follow the importer options. Values remain strings, as in OpenRefine's wikitext importer. A span can leave its covered cells blank or repeat its value. Nested tables are rejected. Deep list/template nesting and unbalanced template syntax are rejected before parsing to prevent stack or memory exhaustion on malformed input. The parser uses a different syntax engine from OpenRefine, so inspect the preview when importing complex markup.

**Resolve wiki links to Wikidata** is off initially. Turning it on sends links with the chosen wiki page prefix to a reconciliation service; only a cell with exactly one wiki link is eligible. Preparation of the table list makes no network requests. Reconciliation requests have time and response-size limits, and errors stop the import before saving a project. The resulting cell reconciliation is retained through saving, reopening and combining files with source columns.

Use the preview to check references and labels. The reference importer has particular text rules: bare URLs and entities can disappear, link suffixes can be omitted, and ordinary text after a reference can be suppressed. Raw template text preserves arguments without expanding them. These are importer semantics, rather than a rendering of the page's appearance.

Wikitext parsing keeps the source and its table structure in memory. It accepts input up to 32 MiB, expanded cell text up to 64 MiB, 10,000 columns and 5,000,000 cells after padding short rows. Reaching a limit stops the import. Link resolution accepts up to 100,000 linked cells and sends at most 50 per request; cancellation can wait for an in-flight request's 30-second timeout.

### PC-Axis

PC-Axis uses `VALUES` labels as dimension columns, ordered by `STUB` followed by `HEADING`, and a final `value` column. The last dimension changes fastest. Missing-value markers such as `.` and `..` remain text. `CODES`, localized metadata and notes do not become extra columns. Select the character encoding if detection needs correcting; the file's `CHARSET` metadata does not override that choice.

This follows OpenRefine's line-sensitive importer, rather than supporting every PC-Axis dialect. Like the reference, it can return fewer rows for short `DATA`, and cycles the dimension labels if extra values are present. A semicolon discards the rest of its line; later data lines are still read. Header and blank-cell settings are fixed by this importer, while **Skip data lines** and **Limit** remain available.

## Database queries

Choose **Database** on Home or in Import. SQLite reads an existing local database file; PostgreSQL, MySQL and MariaDB use a host, port, database and username. Saved connection profiles keep these settings without a password. Enter the password for the current dialog; switching profiles or closing the dialog clears it. The test and schema buttons perform explicit reads.

Use an account granted only the database read permissions you need. Queries run in read-only transactions, and the query editor accepts a single `SELECT` or `WITH` query with additional guards against known side effects. This is not a sandbox for privileged server functions or custom functions. SQLite network paths and connection URIs are rejected.

Select a table from the schema browser or enter SQL, then choose **Load result**. Review the rows before choosing **Create project**. Loading does not save or replace a project. Creation uses the complete prepared result without running SQL again, so later database changes do not alter the snapshot. A source database is not required to reopen the saved project. Neither SQL nor connection credentials are saved in project metadata.

The default result limit is 100,000 rows; the maximum is 1,000,000. An estimated 128 MiB result budget, 512-column limit and query timeout also apply. Exceeding a limit stops the load rather than saving partial results. Up to four prepared results share the cache budget. They expire after 30 minutes, can be evicted earlier and do not survive a server restart. Stop cancels the owned request; connection setup and server-side cleanup can take longer than a local loop iteration.

SQL integers retain their exact values. Decimal values, dates, times and boolean representations remain text; SQL `NULL` and empty text remain distinct. Binary values use `base64:` followed by Base64 so their bytes survive. Long text is shortened only in the labelled preview. Some representations differ from OpenRefine's JDBC import, including lossless binary data and canonical decimal/time formatting; see [differences](differences.md).

Changing profile settings, SQL or query limits requires a new load. Renaming a project does not rerun a prepared query. Editing a saved profile uses its revision, so an older dialog cannot silently overwrite a newer edit.

## Wikimedia Commons
Enter one category title per line, with or without `Category:`, and choose a subcategory depth for all listed categories. Depth 0 reads the listed categories themselves; depth 1 also reads their direct subcategories. **Browse and preview** fetches metadata anonymously from the official Commons API. It does not download images, upload anything or create a project.

The **File** column retains full `File:` titles and is already reconciled to the corresponding Commons MediaInfo `M` identifier. These are Commons identifiers, not Wikidata `Q` identifiers. **Include file-page wikitext** adds a **Wikitext** column containing the page's main-slot text. This option is an OxRefine addition; the separate OpenRefine CommonsExtension normally retrieves wikitext in a later data-extension step.

Review the preview and enter a project name, then choose **Create project**. Creation uses the prepared snapshot and makes no second Commons request. The existing project remains selected until the new one is successfully saved. The snapshot is temporary: it expires after 30 minutes, can be evicted by later previews and does not survive a server restart. If it expires, browse again. Changing categories, depth or the wikitext option also requires a new browse.

Use **Stop loading**, **Cancel** or the matching job in Jobs to stop a fetch. Cancellation is cooperative: an in-flight request may run until its timeout. Closing a preview does not create a project, and late replies do not replace another view. Saving is the commit point; a cancellation request cannot undo a save that has already completed.

The UI accepts up to 20 category roots, depth 10 and 10,000 unique files. The importer also bounds visited categories, request count, response sizes and estimated metadata size; reaching a limit or an API error fails the load rather than creating a partial project. It follows continuation responses, waits for bounded rate-limit backoff and visits subcategories depth first. Overlapping categories produce one row per file, and visited-category tracking prevents repeated cyclic traversal. This deduplication is an intentional difference from CommonsExtension. Long wikitext is shortened only in the labelled preview; the created project retains the full retrieved text.

The JSON API exposes `commons_prepare` with `params.categories` (`[{"name":"Category:OpenRefine","depth":0}]`), `params.includeWikitext` and a unique `params.requestId`. Use the returned snapshot `id` with `commons_preview` (`start`, `limit` up to 100) or `commons_create` (`name`). Snapshots preserve reconciliation metadata through project creation, saving and reopening.

## Big files

In the default in-memory mode, delimited files of 8 MB or more use parallel column workers. Long imports report progress and can be cancelled. Current measured workloads and their limits are in the README benchmarks.

Experimental row-code spilling is opt-in. Set `OXREFINE_CODE_BUDGET_MIB` to a nonnegative integer before starting OxRefine; `0` forces all nonempty row-code buffers to disk. `OXREFINE_SPILL_DIR` optionally selects the temporary storage directory. With a finite code budget, CSV currently uses the serial streaming parser. Private spill files are shared by history snapshots and removed after their last owner closes. Saved OXT row codes load as a stream rather than a second full-file buffer.

This budget covers the tracked row-code arrays. Dictionaries of distinct values, flags, row indexes, reconciliation metadata, document trees and operation scratch still use RAM; mapped pages are managed by the OS and can contribute to RSS. This is not a total process-memory limit. High-cardinality data can still require substantial memory. `storage_stats` reports tracked code, writer-buffer and mapping counters separately. The 50M-row validation report identifies the exact workload and operating-system limit used.
