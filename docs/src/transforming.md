# Transforming cells and columns

Each operation applies to the rows your facets select, appears in the **History** tab, and can be undone.

## Cells

- **Transform…** (<kbd>T</kbd> on the column you last used) runs a GREL expression on every cell. The transform view groups the preview by distinct before/after values with row counts; **Rows** switches to individual examples. Totals count every selected row: how many distinct values change and how many result. Typed text and numbers remain distinct. Expressions that depend on the row can show several outcomes for one original value. The displayed list is limited to 200 pairs or 20 row examples, while the totals cover the full selection. *On error* (keep the original, set to blank, or store the error) and *repeat until no change* use the same operation as Apply; <kbd>Ctrl ↵</kbd> applies it after the latest preview is ready.
- **Common transforms:** trim, collapse whitespace, titlecase, uppercase, lowercase, to number, to date, to text, to blank.
- **Fill down / Blank down:** fill blank cells with the value above, or blank out repeats.
- **Split multi-valued cells… / Join multi-valued cells…:** turn `a; b; c` into one row per value within a record, and back.
- **Edit one cell:** click the cell and press <kbd>Enter</kbd>, double-click it, or hover it and click the pencil. The editor can also change every identical cell at once. The status bar shows the selected cell's value with its spaces made visible, and says when it has a leading or trailing space.

The transform preview does not change the table or history. It calculates a temporary transformed column to obtain exact counts, so large unique-value columns can take time and memory. Nondeterministic expressions are evaluated again when applied; their values may differ from a prior preview.

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

From the **Rows** menu in the top bar: star, flag, keep only, or remove the rows that match your facets. From a column's menu: **Sort rows by this column…** (a permanent reorder, as in OpenRefine) and **Remove duplicate rows by this column**.

## GREL

OxRefine runs GREL, OpenRefine's expression language, with the same functions, controls, error messages and number formatting. The expression editor completes function names and shows each function's description. See the [GREL reference](grel.md).
