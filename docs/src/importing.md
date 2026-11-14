# Importing data

## Sources

- **A file:** CSV and TSV (any separator), fixed-width text, Excel (`.xlsx`, `.xls`), OpenDocument (`.ods`), JSON and JSON Lines, XML, or plain lines.
- **A folder or an archive:** zip, tar, gzip and bzip2 archives (`.tar.gz`, `.tar.bz2`), or a folder of files. Choose which files to import. Optional columns record the file and archive each row came from.
- **The clipboard:** **Paste from clipboard** on the home screen (or **Import → Paste data…** in a project) takes rows copied from a spreadsheet, CSV text, or plain lines.
- **A web address:** **From a web address** (or **Import → Web address…**) downloads the file and opens the same preview.
- **An OpenRefine project:** see [Moving projects and recipes](moving-over.md).

On the home screen, drop one file onto the window to open the same import preview. The desktop app also accepts one folder. Browser folder drops and multiple files are rejected with a message; use the file/folder picker or an archive instead. Dropping an OpenRefine project archive asks for confirmation before importing it with its history. File drops while a project or another view is open do not replace the current work.

## The preview

OxRefine detects the format and opens the import view: what it detected and the project name at the top, the options on the left, and the first rows as they will be read on the right, with the time they took to parse. **Create project** (<kbd>Ctrl ↵</kbd>) imports the file; <kbd>Esc</kbd> cancels. The options match OpenRefine's:

- **Delimited text:** separator, encoding, header lines, lines to skip, quote handling, blank rows and cells, trimming, guessing cell types.
- **Fixed-width:** column widths, an editable ruler and guessed boundaries from a sample.
- **Excel and ODS:** which sheets to import, and header rows.
- **JSON and XML:** the element that holds one record, chosen from a list.

The import options follow OpenRefine's wizard. For JSON, OxRefine also suggests a record path so nested data has a useful first preview.

### JSON and JSON Lines

Pick the object or array that holds your records from the record-path list. Paths show their observed count and example fields. A bounded sample is labelled as a sample; its counts describe the part inspected, rather than a total for the file. Nested arrays can expand one selected record into several table rows. An empty preview explains that the source may be empty or the selected path may not match, and prevents creating an empty project by mistake.

The manual path accepts a JSON array of field names, so commas, spaces and underscores in names can be preserved exactly. `_` names the anonymous root, and another `_` selects array items. For `{"data":{"items":[{"sku":"A-1"}]}}`, select `["_", "data", "items", "_"]`.

JSON Lines imports decode and parse one line at a time, appending cells to the table's column dictionaries. The table and the largest current record still need memory. New imports use a path relative to each line's root, starting with `["_"]`. Older OxRefine settings retain their virtual-array path semantics: the usual `["_", "_"]` path streams, while the rare legacy paths `[]` and `["_"]` still load the entire virtual array to preserve their original row alignment.

A limited JSON Lines preview stops after the requested number of records; it does not validate the rest of the source. A later malformed line can therefore fail a full import after its preview succeeded.

### Fixed-width text

Choose **Fixed-width** for text with aligned columns. OxRefine suggests boundaries from a sample when possible; files without consistent spaces may have no suggested boundaries. Add, move or remove the ruler's boundaries to change the preview, or edit **Column widths** directly. **Guess widths** reapplies the suggested boundaries. Invalid widths show an error and prevent project creation until corrected. The remaining text after the last width forms one more column.

The guess examines up to 100 nonempty lines and 65,536 Unicode characters; the ruler displays up to 20 lines and 512 characters per line. Its display marks truncated samples. The ruler and widths count Unicode code points rather than bytes or visual text width. OpenRefine counts UTF-16 code units, so widths can differ for characters outside the basic multilingual plane, including many emoji. Check the preview when reusing those widths between the two tools.

## Big files

Delimited files of 8 MB or more are read on every CPU core. A million-row CSV imports in about 0.35 s. A progress bar shows long imports, and you can cancel them.
