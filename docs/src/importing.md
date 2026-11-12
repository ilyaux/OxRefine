# Importing data

## Sources

- **A file:** CSV and TSV (any separator), fixed-width text, Excel (`.xlsx`, `.xls`), OpenDocument (`.ods`), JSON and JSON Lines, XML, or plain lines.
- **A folder or an archive:** zip, tar, gzip and bzip2 archives (`.tar.gz`, `.tar.bz2`), or a folder of files. Choose which files to import. Optional columns record the file and archive each row came from.
- **The clipboard:** **Import → Paste data…** takes rows copied from a spreadsheet, CSV text, or plain lines.
- **A web address:** **Import → Web address…** downloads the file and opens the same preview.
- **An OpenRefine project:** see [Moving projects and recipes](moving-over.md).

## The preview

OxRefine detects the format and shows the first rows as they will be read. The options match OpenRefine's:

- **Delimited text:** separator, encoding, header lines, lines to skip, quote handling, blank rows and cells, trimming, guessing cell types.
- **Fixed-width:** column widths.
- **Excel and ODS:** which sheets to import, and header rows.
- **JSON and XML:** the element that holds one record, chosen from a list.

The defaults match OpenRefine's import wizard, so a file opens the way OpenRefine would open it.

## Big files

Delimited files of 8 MB or more are read on every CPU core. A million-row CSV imports in about 0.35 s. A progress bar shows long imports, and you can cancel them.
