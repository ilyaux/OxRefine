# Wikibase and Wikidata

OxRefine does what OpenRefine's Wikibase extension does: a project keeps a **schema** that turns its rows into edits of a Wikibase, such as Wikidata. You can check those edits, export them as QuickStatements, or upload them. Schemas move between OpenRefine and OxRefine in project archives and recipes.

## The schema

**Edit Wikibase schema…** in the command palette (<kbd>Ctrl K</kbd>) opens the editor. On the left, pick the Wikibase: Wikidata, Wikimedia Commons, or another one by its MediaWiki API address. The entity address must match the identifier space of your reconciled cells.

Add an **item** or **media info** entity. Its subject is a reconciled column or a fixed id. A matched cell edits that item. A cell marked as new creates one, and cells sharing the same new item create it once. Then add:

- **Terms**: labels, descriptions and aliases in a language, from a column or a value. A term **if new** is only set where the item has none in that language.
- **Statements**: a property, found by name or typed as an id, with values from columns or constants. Each value can have qualifiers and references. Its mode is **Add or merge** (the default, merged into the same existing statement), **Add**, or **Delete**.

Dates are written `2024-01-31`, `2024-01`, `2024` or `1950D` for a decade. A `_Q1985786` suffix gives the Julian calendar. Coordinates are written `51.5,-0.12`, and a quantity is a number with an optional unit item. A value that a row can't supply is left out of that row's edit.

**Preview** shows the edits on the rows matching your facets, as QuickStatements. **Issues** lists what OpenRefine's checks would report, including the property constraints recorded on the Wikibase (format, single value, allowed values, qualifiers, units and others). **Save schema** records it as a history step, so undo and redo work, and recipes carry it as `wikidata/save-wikibase-schema`.

## Checking and exporting

`wikibaseIssues()` gives one row's issues in an expression, for example in a facet. With a schema saved, **Export** offers **QuickStatements** and **Wikibase schema**; the schema file is what OpenRefine's **Import schema** reads.

## Uploading

The **Upload** tab logs in to the Wikibase. Use a [bot password](https://www.mediawiki.org/wiki/Manual:Bot_passwords) (`Name@BotName` and its password) or an owner-only OAuth consumer's four tokens. OxRefine keeps the login in memory and forgets it when it closes. It never writes credentials to disk.

Give an edit summary and upload. OxRefine fetches the current entities and creates the new items first. It then updates existing ones with only what changes, merging statements and references with those already there, and waits when the wiki lags (maxlag). On Wikidata, a batch is grouped in EditGroups so it can be reviewed and undone together.

Afterwards, the cells of new items are matched to the items made. A **Wikibase editing results** column holds each row's result: the new item, the edit's diff, or the error. The upload is one history step. Undoing it changes the project, not the wiki, and replaying the history never uploads again.

Media info (Commons files) can be edited in the schema and checked, but not uploaded yet. See [Differences](differences.md#wikibase-uploads) for what else differs from OpenRefine.
