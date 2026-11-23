// Wikibase schemas checked against OpenRefine 3.10.1's wikibase extension. Every case runs on the
// project below: `item` and `country` are matched to Wikidata ids (or marked as new items), the
// other columns stay text. MediaWiki requests go to a local mock (gen.mjs), never to Wikidata.
export const SITE = "http://www.wikidata.org/entity/"
export const CSV = `name,item,country,born,population,coords,title,lang,website,alias
Douglas Adams,Q42,Q145,1952-03-11,,51.5/-0.1,The Hitchhiker's Guide,en,https://douglasadams.com,DNA
Ada Lovelace,Q7259,Q145,1815,1200,51.5074/-0.1278,Notes,en,,
New Person,NEW1,Q30,1990-05,42.5,,Ein Titel,de,http://example.org,Neu
Shared New,NEW2,Q142,1999-12-31T10:20,7,48.85/2.35,,fr,,
Shared New Again,NEW2,,not a date,abc,bad,  spaced  ,,,
Blank Item,,Q30,2001,,,,,,
`
// cells of `item` and `country` as Wikidata matches, or as new items (NEW1 alone, NEW2 shared)
export const MATCHES = [
  ["item", "Q42", "Douglas Adams"], ["item", "Q7259", "Ada Lovelace"],
  ["country", "Q145", "United Kingdom"], ["country", "Q30", "United States of America"], ["country", "Q142", "France"],
]
export const NEW = [["item", "NEW1", false], ["item", "NEW2", true]]

const col = (columnName) => ({ type: "wbentityvariable", columnName })
const str = (columnName) => ({ type: "wbstringvariable", columnName })
const constant = (value) => ({ type: "wbstringconstant", value })
const prop = (pid, label, datatype) => ({ type: "wbpropconstant", pid, label, datatype })
const lang = (id) => ({ type: "wblanguageconstant", id, label: id })
const term = (name_type, language, value) => ({ type: "wbnamedescexpr", name_type, value: { type: "wbmonolingualexpr", language, value } })
const strict = { type: "qualifiers", valueMatcher: { type: "strict" }, pids: [] }
const statement = (value, extra = {}) => ({ value, qualifiers: [], references: [], mode: "add_or_merge", mergingStrategy: strict, ...extra })
const group = (property, ...statements) => ({ property, statements })
const schema = (entityEdits) => ({ siteIri: SITE, entityTypeSiteIRI: { item: SITE, property: SITE, mediainfo: "https://commons.wikimedia.org/entity/" }, mediaWikiApiEndpoint: "{API}", entityEdits })
const item = (subject, nameDescs, statementGroups) => ({ type: "wbitemeditexpr", subject, nameDescs, statementGroups })

const country = prop("P27", "country of citizenship", "wikibase-item")
const reference = { snaks: [{ prop: prop("P854", "reference URL", "url"), value: str("website") }, { prop: prop("P813", "retrieved", "time"), value: { type: "wbdateconstant", value: "2024-01-15" } }] }

export const CASES = [
  { name: "empty schema", schema: schema([]) },
  { name: "terms", schema: schema([item(col("item"), [
    term("LABEL_IF_NEW", lang("en"), str("name")),
    term("LABEL", { type: "wblanguagevariable", columnName: "lang" }, str("title")),
    term("DESCRIPTION_IF_NEW", lang("en"), constant("a person")),
    term("DESCRIPTION", lang("fr"), constant("une personne")),
    term("ALIAS", lang("en"), str("alias")),
  ], [])]) },
  { name: "entity values", schema: schema([item(col("item"), [], [
    group(country, statement(col("country"))),
    group(prop("P31", "instance of", "wikibase-item"), statement({ type: "wbentityidvalueconstant", id: "Q5", label: "human" })),
  ])]) },
  { name: "dates", schema: schema([item(col("item"), [], [group(prop("P569", "date of birth", "time"), statement({ type: "wbdatevariable", columnName: "born" }))])]) },
  { name: "quantities and coordinates", schema: schema([item(col("item"), [], [
    group(prop("P1082", "population", "quantity"), statement({ type: "wbquantityexpr", amount: str("population"), unit: null })),
    group(prop("P2048", "height", "quantity"), statement({ type: "wbquantityexpr", amount: str("population"), unit: { type: "wbentityidvalueconstant", id: "Q11573", label: "metre" } })),
    group(prop("P625", "coordinate location", "globe-coordinate"), statement({ type: "wblocationvariable", columnName: "coords" })),
  ])]) },
  { name: "strings and monolingual text", schema: schema([item(col("item"), [], [
    group(prop("P856", "official website", "url"), statement(str("website"))),
    group(prop("P1476", "title", "monolingualtext"), statement({ type: "wbmonolingualexpr", language: { type: "wblanguagevariable", columnName: "lang" }, value: str("title") })),
    group(prop("P742", "pseudonym", "string"), statement(str("alias")), statement(constant("Constant"))),
  ])]) },
  { name: "qualifiers and references", schema: schema([item(col("item"), [], [
    group(country, statement(col("country"), {
      qualifiers: [{ prop: prop("P580", "start time", "time"), value: { type: "wbdatevariable", columnName: "born" } }],
      references: [reference],
    })),
  ])]) },
  { name: "statement modes", schema: schema([item(col("item"), [], [
    group(country, statement(col("country"), { mode: "add" })),
    group(prop("P31", "instance of", "wikibase-item"), statement({ type: "wbentityidvalueconstant", id: "Q5", label: "human" }, { mode: "delete" })),
    group(prop("P106", "occupation", "wikibase-item"), statement(null, { mode: "delete", mergingStrategy: { type: "property" } })),
  ])]) },
  { name: "string details", schema: schema([item(col("item"), [], [
    group(prop("P742", "pseudonym", "string"), statement(str("title")), statement(constant('He said "hi"')), statement(constant("  padded  "))),
  ])]) },
  { name: "constant formats", schema: schema([item(col("item"), [], [
    group(prop("P625", "coordinate location", "globe-coordinate"), statement({ type: "wblocationconstant", value: "48.8566,2.3522" }), statement({ type: "wblocationconstant", value: "-33.86/151.21" })),
    group(prop("P571", "inception", "time"), ...["1950D", "1952-03-11_Q1985786", "2024-01-15T13:45:00Z", "1066-10-14", "-0044-03-15", "2024-02", "1999"].map((value) => statement({ type: "wbdateconstant", value }))),
    group(prop("P1082", "population", "quantity"), statement({ type: "wbquantityexpr", amount: constant("1e3"), unit: null }), statement({ type: "wbquantityexpr", amount: constant("-0.50"), unit: null })),
  ])]) },
  { name: "invalid constants", schema: schema([item(col("item"), [], [
    group(prop("P571", "inception", "time"), statement({ type: "wbdateconstant", value: "19C" }), statement({ type: "wbdateconstant", value: "2M" })),
    group(prop("P625", "coordinate location", "globe-coordinate"), statement({ type: "wblocationconstant", value: "north" })),
  ])]) },
  { name: "two references", schema: schema([item(col("item"), [], [
    group(country, statement(col("country"), { references: [reference, { snaks: [{ prop: prop("P143", "imported from", "wikibase-item"), value: { type: "wbentityidvalueconstant", id: "Q328", label: "English Wikipedia" } }] }] })),
  ])]) },
  { name: "unreconciled subject", schema: schema([item(col("name"), [term("LABEL", lang("en"), str("name"))], [])]) },
  { name: "media info", schema: schema([{ type: "wbmediainfoeditexpr", subject: col("item"), filePath: str("website"), fileName: str("name"), wikitext: constant("== Summary =="), overrideWikitext: false,
    nameDescs: [term("LABEL", lang("en"), str("name"))], statementGroups: [group(prop("P180", "depicts", "wikibase-item"), statement(col("country")))] }]) },
  { name: "two entities per row", schema: schema([
    item(col("item"), [term("LABEL_IF_NEW", lang("en"), str("name"))], [group(country, statement(col("country")))]),
    item(col("country"), [], [group(prop("P1082", "population", "quantity"), statement({ type: "wbquantityexpr", amount: str("population"), unit: null }))]),
  ]) },
]
