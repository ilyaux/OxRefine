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

OxRefine runs GREL, OpenRefine's expression language, with the same functions, controls, error messages and number formatting. The expression editor completes function names and shows each function's description. See the [GREL reference](grel.md). [Extensions](extensions.md) can add functions of their own.

## Python and Clojure

The language switch above the expression editor picks **GREL**, **Python** or **Clojure**, as OpenRefine's does. Steps keep OpenRefine's prefixes (`jython:`, `clojure:`), so recipes move between the two in both directions.

**Python** is a function body that returns the result, with `value`, `cell`, `cells`, `row` and `rowIndex` bound as in OpenRefine:

```python
import re
return re.sub(r"\s+", " ", value).strip().title()
```

OpenRefine runs these in Jython 2.7. OxRefine runs them in an embedded Python 3 (RustPython) and keeps the Python 2 behaviour that expressions rely on. Integer division stays integer (`5 / 2` is 2), `print x` and `except E, e:` still parse, and `unicode`, `xrange`, `long`, `reduce` and `dict.has_key` work. `\w`, `\d` and `\s` in regexes match ASCII only unless `re.UNICODE` is given. `round()` rounds halves away from zero. The standard library is there (`re`, `json`, `math`, `datetime`, `hashlib`, `base64`, `urllib`...); PyPI packages aren't. Three Jython details differ: the text type's name is `str`, not `unicode`; `str()` keeps non-ASCII text instead of raising `UnicodeEncodeError`; and dictionaries keep insertion order.

**Clojure** is one expression, with the same names bound:

```clojure
(clojure.string/join "-" (clojure.string/split value #","))
```

OxRefine interprets the part of Clojure 1.12 that expressions use. That covers the core special forms and macros (`let`, `fn`, `loop`/`recur`, `cond`, `->`, `->>`, `if-let`, `case`, `try`...), about 150 `clojure.core` and `clojure.string` functions, and the numeric tower (long overflow errors, ratios, `N` and `M` literals). Java interop works on strings, numbers, `Math`, `Character`, `Integer`/`Long`/`Double` parsing and the `cells`/`row` objects (`(.getField cells "name" nil)`). An unknown name is a syntax error, as in Clojure. Sequences are eager, so `(range)` and `(iterate f x)` without a bound are refused.

Both are checked against 337 expressions run through OpenRefine 3.10.1 (`tests/langs`); 334 give the same result, and the three Python differences above are the rest. Python is slower than GREL, about 50 µs a row, so a million rows takes most of a minute.
