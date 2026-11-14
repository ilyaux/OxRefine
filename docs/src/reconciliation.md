# Reconciliation

Reconciliation matches the values in a column to entities in an outside database, such as Wikidata or any service that speaks the [W3C Reconciliation API](https://www.w3.org/community/reports/reconciliation/CG-FINAL-specs-0.2-20230410/). Matched cells carry the entity's id, which you can use to add more data or to link records.

## Start reconciling

Column menu → **Reconcile → Start reconciling…**

1. **Service:** Wikidata is listed by default. To add another, paste its URL; OxRefine reads its manifest.
2. **Type:** OxRefine asks the service about a sample of the column and suggests the types that fit best, as OpenRefine does. You can also search for a type, enter its id, or reconcile against no particular type.
3. **Other columns as properties:** give another column a property id, such as `P17` for country, and its values are sent with each query to narrow the matches. Columns that are already reconciled send their matched entity.
4. **Auto-match:** when the service is confident about a candidate, the cell is matched straight away.

Cells with the same text are queried once. The job shows its progress and can be cancelled. When it finishes, facets by judgment and by best score open by themselves.

## Judge the results

After reconciliation, the **Reconcile** view shows one row per matching text with its cell count, types, saved reconciliation states, candidates and scores. It reads saved results without calling the service again. Different candidate lists or judgments for the same text remain visible as separate states, with pagination for values, states and candidates.

**Match cell** acts on the explicitly numbered representative row of the selected state. **Match all** applies the candidate to all matching cells inside the current facets (including the selected records in records mode). It uses OpenRefine's same-text semantics: numeric `1` and text `"1"` share a visible group, with their type counts shown. Bulk matching is disabled when cells lack reconciliation data or use different services or identifier spaces; individual cells can still be judged. The action enters the normal undo history. **Reconcile again…** returns to service configuration.

Each reconciled cell shows a mark after its value:

- **matched:** a check mark and the entity's name;
- **candidates:** how many there are, for cells still to judge;
- **error:** a warning sign, with the service's message.

Click the mark to see the candidates with their scores and types, and links to the service's page for each. From there you can:

- **match** a candidate to this cell, or to every cell with the same text;
- **search** the service for another item, or enter an id;
- **create a new item** for this cell, or one new item for every identical cell;
- **preview** a candidate, if the service offers previews;
- **discard** the judgment or **clear** the reconciliation data.

## Work in bulk

**Reconcile → Actions** applies to the rows your facets select:

- **Match each cell to its best candidate**
- **Create a new item for each cell**, or **one new item for similar cells**
- **Match all filtered cells to…** one item
- **Discard reconciliation judgments**, **Clear reconciliation data**
- **Copy reconciliation data to other columns…**
- **Use values as identifiers…**, for a column that already holds ids
- **Add entity identifiers column…**

**Reconcile → Facets** has OpenRefine's reconciliation facets: by judgment, best candidate's score, type match, name match, edit distance, word similarity, types, judgment action and history entry.

## Add data from the matched items

Column menu → **Edit column → Add columns from reconciled values…** asks the service which properties it can provide for the column's type. Pick any, or enter property ids, and each becomes a new column. When an item has several values, they fill extra rows of its record. Values that are entities themselves, such as a country, come back already matched.

## In expressions

`cell.recon` gives GREL access to the reconciliation data: `judgment`, `match.id`, `match.name`, `best.score`, `candidates`, `features.nameMatch`, and more. For example, `cell.recon.match.id` lists the matched ids.

## Compatibility

Reconciliation sends the same queries as OpenRefine 3.10.1 and gets the same results. That covers auto-matching, the features, batching and error messages, every action, single-cell judgments and data extension. The reconciliation data is kept in OpenRefine project archives in both directions.
