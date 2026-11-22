"""text-tools: an example OxRefine extension.

Copy this folder into the `extensions` folder next to your projects (see docs/src/extensions.md),
then restart OxRefine or press Reload in Preferences -> Extensions.
"""
import configparser
import re
import unicodedata

from oxrefine import exporter, function, importer, operation


@function("slugify", "Lowercase ASCII words joined by dashes: 'Hello, World!' becomes 'hello-world'.", "string s")
def slugify(s):
    if s is None:
        return None
    text = unicodedata.normalize("NFKD", str(s)).encode("ascii", "ignore").decode("ascii")
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")


@function("wordCount", "How many whitespace-separated words a string has.", "string s")
def word_count(s):
    return 0 if s is None else len(str(s).split())


@importer("ini", label="INI settings file", extensions=[".ini", ".cfg"])
def read_ini(data, options):
    """One row per setting: section, key, value."""
    parser = configparser.ConfigParser(interpolation=None)
    parser.optionxform = str  # keep the keys' case
    parser.read_string(data.decode(options.get("encoding") or "utf-8"))
    rows = [[section, key, value] for section in parser.sections() for key, value in parser.items(section, raw=True)]
    return {"columns": ["section", "key", "value"], "rows": rows}


@exporter("markdown", label="Markdown table", extension=".md", mime="text/markdown")
def to_markdown(columns, rows, options):
    def cell(v):
        return "" if v is None else str(v).replace("|", "\\|").replace("\n", " ")

    lines = ["| " + " | ".join(cell(c) for c in columns) + " |", "| " + " | ".join("---" for _ in columns) + " |"]
    lines += ["| " + " | ".join(cell(v) for v in row) + " |" for row in rows]
    return "\n".join(lines) + "\n"


@operation(
    "number-rows",
    label="Number the rows",
    description="Add a first column that numbers the rows.",
    params=[{"name": "column", "label": "New column name", "default": "#"}, {"name": "start", "label": "First number", "default": 1}],
)
def number_rows(columns, rows, params):
    name = params.get("column") or "#"
    start = int(params.get("start", 1))
    return {"columns": [name] + columns, "rows": [[start + i] + row for i, row in enumerate(rows)]}
