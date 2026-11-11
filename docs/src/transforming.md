# Transforming cells and columns

Each operation applies to the rows your facets select, appears in the **History** tab, and can be undone.

## Cells

- **Transform…** runs a GREL expression on every cell, with a preview of the first rows before and after; a cell whose expression fails keeps its value.
- **Common transforms:** trim, collapse whitespace, titlecase, uppercase, lowercase, to number, to date, to text, to blank.
- **Fill down / Blank down:** fill blank cells with the value above, or blank out repeats.
- **Split multi-valued cells… / Join multi-valued cells…:** turn `a; b; c` into one row per value within a record, and back.
- **Edit one cell:** hover the cell and click the pencil. The editor can also change every identical cell at once.

## Columns

- **Split into several columns…:** by a separator; numbers in the new columns become numbers. Recipes can also split by regular expression or field lengths, as in OpenRefine.
- **Add column based on this…:** a new column from a GREL expression. `cells["other column"].value` reads other columns, and `cross()` looks up values in another project.
- **Add column by fetching URLs…:** an expression builds a URL for each row and the response goes into a new column. There are settings for the delay between requests, caching, request headers and what to do on an error. It runs as a background job that you can cancel.
- **Rename…**, **Remove column**, **Move column**.

## Transpose

- **Cells across columns into rows…:** several columns become key/value rows.
- **Cells in rows into columns…:** groups of rows become columns.
- **Columnize by key/value columns…:** a key column and a value column become one column per key.

## Rows

From the **Rows** menu in the toolbar: star, flag, keep only, or remove the rows that match your facets. From a column's menu: **Sort rows by this column…** (a permanent reorder, as in OpenRefine) and **Remove duplicate rows by this column**.

## GREL

OxRefine runs GREL, OpenRefine's expression language, with the same functions, controls, error messages and number formatting. The expression editor completes function names and shows each function's description. See the [GREL reference](grel.md).
