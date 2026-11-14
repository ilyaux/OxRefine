# Importing data

## Sources

- **A file:** CSV and TSV (any separator), fixed-width text, Excel (`.xlsx`, `.xls`), OpenDocument (`.ods`), JSON and JSON Lines, XML, or plain lines.
- **A folder or an archive:** zip, tar, gzip and bzip2 archives (`.tar.gz`, `.tar.bz2`), or a folder of files. Choose which files to import. Optional columns record the file and archive each row came from.
- **The clipboard:** **Paste from clipboard** on the home screen (or **Import → Paste data…** in a project) takes rows copied from a spreadsheet, CSV text, or plain lines.
- **A web address:** **From a web address** (or **Import → Web address…**) downloads the file and opens the same preview.
- **An OpenRefine project:** see [Moving projects and recipes](moving-over.md).

## The preview

OxRefine detects the format and opens the import view: what it detected and the project name at the top, the options on the left, and the first rows as they will be read on the right, with the time they took to parse. **Create project** (<kbd>Ctrl ↵</kbd>) imports the file; <kbd>Esc</kbd> cancels. The options match OpenRefine's:

- **Delimited text:** separator, encoding, header lines, lines to skip, quote handling, blank rows and cells, trimming, guessing cell types.
- **Fixed-width:** column widths, an editable ruler and guessed boundaries from a sample.
- **Excel and ODS:** which sheets to import, and header rows.
- **JSON and XML:** the element that holds one record, chosen from a list.

The defaults match OpenRefine's import wizard, so a file opens the way OpenRefine would open it.

### Fixed-width text

Choose **Fixed-width** for text with aligned columns. OxRefine suggests boundaries from a sample when possible; files without consistent spaces may have no suggested boundaries. Add, move or remove the ruler's boundaries to change the preview, or edit **Column widths** directly. **Guess widths** reapplies the suggested boundaries. Invalid widths show an error and prevent project creation until corrected. The remaining text after the last width forms one more column.

The guess examines up to 100 nonempty lines and 65,536 Unicode characters; the ruler displays up to 20 lines and 512 characters per line. Its display marks truncated samples. The ruler and widths count Unicode code points rather than bytes or visual text width. OpenRefine counts UTF-16 code units, so widths can differ for characters outside the basic multilingual plane, including many emoji. Check the preview when reusing those widths between the two tools.

## Big files

Delimited files of 8 MB or more are read on every CPU core. A million-row CSV imports in about 0.35 s. A progress bar shows long imports, and you can cancel them.
