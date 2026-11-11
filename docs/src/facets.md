# Facets and filters

Facets show what is in a column and narrow the grid to the rows you pick. Every operation and every export then applies to those rows only, as in OpenRefine. Open them from a column's menu under **Filter**. They appear in the **Facets / Filters** tab of the side panel.

## Kinds

- **Text facet:** each distinct value with its row count, by count or by name. Click choices to include them, use *invert* to exclude them instead, and use the pencil to change a value in every row.
- **Numeric facet:** a histogram with a range to drag, plus switches for non-numeric, blank and error cells.
- **Timeline facet:** the same for dates.
- **Text filter:** keeps the rows whose cell contains the text you type.
- **Custom text facet… / Custom numeric facet…:** a facet over a GREL expression, such as `value.length()` or `cells["city"].value + ", " + value`.
- **Customized facets:** ready-made expressions for words, duplicates, text length, Unicode character codes, blank cells, errors, stars and flags.
- **Reconciliation facets:** judgment, best candidate's score, type match, name match, edit distance, word similarity, types, judgment action and history entry. See [Reconciliation](reconciliation.md).

## Rows and records

Switch between **Rows** and **Records** at the bottom of the grid. In records mode, a row with a value in the first column starts a record, and the following rows with that cell blank belong to it. Facets then count and filter whole records, as in OpenRefine.

## Speed

Facets count each distinct value once, not each row. A text facet over a million rows takes about 10 ms.
