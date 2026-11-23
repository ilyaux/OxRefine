# Extensions

An extension adds GREL functions, importers, exporters and operations to OxRefine without rebuilding it. It is a folder with a `main.py`, written in the same Python that runs `jython:` expressions. OpenRefine's own extensions are Java modules; those don't load in OxRefine, and an OxRefine extension doesn't load in OpenRefine. Steps from OpenRefine's Java extensions can still run, in an OpenRefine you have installed: see [OpenRefine's Java extensions](#openrefines-java-extensions).

[`docs/extensions/text-tools`](https://github.com/ilyaux/OxRefine/tree/main/docs/extensions/text-tools) is a complete example of all four kinds. Copy its folder to try it.

## Installing one

Extensions live in the `extensions` folder of the OxRefine data folder:

| System | Folder |
|---|---|
| Windows | `%APPDATA%\OxRefine\extensions` |
| macOS | `~/Library/Application Support/OxRefine/extensions` |
| Linux | `~/.local/share/oxrefine/extensions` |
| `ox serve --data DIR` | `DIR/extensions` |

`OXREFINE_EXTENSIONS` adds more folders, separated like `PATH`. Each extension is a subfolder:

```
extensions/
  text-tools/
    main.py           required
    extension.json    optional: {"name", "version", "description"}
```

**Preferences → Extensions** lists what each one provides, or why it failed to load, and **Reload extensions** picks up changes without restarting. `ox apply` loads the same folder, so recipes that use extension functions or operations run headless too.

An extension runs with the same rights as OxRefine: it can read and write your files and use the network. Install only extensions you trust, as you would with OpenRefine's.

## Writing one

`main.py` registers its parts with decorators from the `oxrefine` module:

```python
from oxrefine import exporter, function, importer, operation

@function("slugify", "Lowercase ASCII words joined by dashes.", "string s")
def slugify(s):
    ...
```

Values arrive as Python values: text is `str`, numbers are `int` or `float`, booleans `bool`, dates `datetime.datetime` in UTC, blanks `None`. What you return is turned back the same way. A `list` becomes an array, and an exception becomes an error value or a failed step.

### `@function(name, description="", params="")`

A GREL function, called like any other: `slugify(value)`, or `value.slugify()`. Its name can't be one of GREL's built-in functions. `params` is the argument list shown in Preferences and in the editor's suggestions.

### `@importer(id, label=None, extensions=())`

Reads a file. The function gets the file's `bytes` and the import options as a `dict` (`encoding`, `limit`...). It returns `{"columns": [...], "rows": [[...], ...]}`. Files whose extension is in `extensions` open with it automatically; the import preview shows its `label`.

### `@exporter(id, label=None, extension="txt", mime="text/plain")`

Writes the matching rows. The function gets the column names, the rows (lists of values) and the export options. It returns `str` or `bytes`. It shows in the Export dialog's format list; a `text/…` MIME type gets a text preview.

### `@operation(id, label=None, description="", params=())`

Changes the whole table, as one undoable step. The function gets the column names, every row and a `dict` of parameters. It returns `{"columns", "rows"}` like an importer. `params` is a list of `{"name", "label", "default"}`; the command palette (Ctrl+K) asks for each one, and a number `default` makes the answer a number.

The step is recorded as:

```json
{"op": "ext/operation", "operation": "number-rows", "params": {"column": "#", "start": 1}}
```

It goes into extracted recipes like any other step. Replaying it needs the same extension installed; without it the recipe stops at that step and names the missing operation. The rows it returns replace the table, so stars, flags and reconciliation data don't survive an extension operation.

## Limits

- Code runs in OxRefine's embedded Python 3 (RustPython) with its standard library. Packages from PyPI aren't available, and C extensions can't load.
- Operations and exporters receive the whole table as Python lists. That is fine for tens of thousands of rows and slow for millions.
- Two extensions can't register the same id; the second one is listed with the conflict and provides nothing.

## OpenRefine's Java extensions

OpenRefine extensions are Java modules: OxRefine can't load them, but it can hand their steps to OpenRefine itself. Install OpenRefine with the extensions you need, and give its folder (the one with `server` and `webapp`) in **Preferences → Extensions → OpenRefine folder**. Then **Run steps in OpenRefine…** in the command palette takes OpenRefine operations as JSON, such as a recipe extracted from OpenRefine.

OxRefine writes the open project as an OpenRefine project archive, history included. It starts that OpenRefine headless with a scratch workspace on a local port, imports the project, runs the steps and exports the result. The result opens as a new project, with OpenRefine's steps in its history; the original project is unchanged. OpenRefine starts each time, which takes a few seconds. It uses its bundled Java, or the one in `JAVA_HOME`. Nothing is downloaded or installed.

This runs extension *operations*, not their dialogs or menus: build the steps in OpenRefine once, extract them as a recipe, and run that recipe from OxRefine on other data.

