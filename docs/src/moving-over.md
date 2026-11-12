# Moving projects and recipes

You don't have to choose between the two tools. Recipes and whole projects move between OpenRefine and OxRefine in both directions.

## Projects

### From OpenRefine to OxRefine

1. In OpenRefine, open the project and choose **Export → OpenRefine project archive to file**. You get a `.openrefine.tar.gz` file.
2. In OxRefine, choose **Open an OpenRefine project** on the home screen (or **Import → OpenRefine project…** in a project) and pick that file.

The project opens with its data, stars, flags, reconciliation data and **its undo history**: every step can be undone and redone as it could in OpenRefine. OxRefine rebuilds each step from OpenRefine's own change records, then checks that redoing the step gives OpenRefine's result.

A step that OxRefine can't redo, such as an operation from an extension it doesn't support yet, is merged into the starting data together with everything before it. You then see the data exactly as OpenRefine had it; only that part of the history can't be undone. OxRefine tells you when this happens and why.

### From OxRefine to OpenRefine

Choose **Export → OpenRefine project** and import the file in OpenRefine with **Import project**. OpenRefine opens it with OxRefine's full history, and each step can be undone there.

Every archive in OxRefine's test suite is checked this way in a real OpenRefine 3.10.1, with every undo step compared.

## Recipes

A recipe is the list of operations OpenRefine shows under **Undo / Redo → Extract**, as JSON. OxRefine reads and writes the same JSON, including the facets each step was limited to.

- **Use an OpenRefine recipe in OxRefine:** choose **Recipe → Apply a recipe…** and paste the JSON or pick a file.
- **Save an OxRefine recipe:** choose **Recipe** in the top bar, tick the steps to keep, and copy or save the JSON. OpenRefine's **Apply…** accepts it.
- **Run a recipe without opening the app:**

  ```bash
  ox apply recipe.json input.csv -o cleaned.csv
  ```

  The input and the output can also be OpenRefine project archives.

### What travels in a recipe

As in OpenRefine, a recipe contains the operations that act on the whole column or table: transforms, mass edits, splits, reconciliation and so on. Single-cell edits and starring or flagging one row are recorded in the history, but are not part of a recipe.

Steps that call a web service, such as fetching URLs, reconciling, or adding columns from a reconciliation service, contact the service again when a recipe is applied to new data, as they do in OpenRefine. Within a project, OxRefine keeps what the service answered, so undoing and redoing never asks again.
