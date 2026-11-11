// Reconciliation cases: each runs on a fresh project from CORPUS, against service.mjs ({BASE}).
export const CORPUS = `name,country,city
Douglas Adams,UK,London
Douglas Adams,UK,London
douglas adams,US,York
Douglas Adam,US,New York
Paris,FR,Paris
Paris,US,Paris
Paris Hilton,US,New York
Ada Lovelace,UK,London
Lovelace,UK,
,UK,York
Nobody Known,XX,Nowhere
Broken,UK,London
`

const all = { facets: [], mode: "row-based" }
const text = (columnName, query) => ({
  facets: [{ type: "text", name: columnName, columnName, query, mode: "text", caseSensitive: false, invert: false }],
  mode: "row-based",
})
const svc = (path = "/svc") => ({ mode: "standard-service", service: `{BASE}${path}`, identifierSpace: "http://ox.test/entity/", schemaSpace: "http://ox.test/prop/" })
const recon = (columnName, config = {}, engineConfig = all) => ({
  op: "core/recon", engineConfig, columnName,
  config: { ...svc(), type: null, autoMatch: true, columnDetails: [], limit: 0, ...config },
})
const human = { type: { id: "Q5", name: "human" } }
const city = { type: { id: "Q515", name: "city" } }
const country = (column, pid = "P17") => ({ columnDetails: [{ column, propertyName: "country", propertyID: pid }] })
const addColumn = (name, expression, base = "name") => ({
  op: "core/column-addition", engineConfig: all, baseColumnName: base, expression: "grel:" + expression,
  onError: "set-to-blank", newColumnName: name, columnInsertIndex: 1,
})

// the Recon fields get-rows leaves out, as a column: judgment action, batch size, match rank
const hidden = addColumn("hidden", 'if(cell.recon == null, null, cell.recon.judgmentAction + "|" + cell.recon.judgmentBatchSize + "|" + cell.recon.matchRank + "|" + cell.recon.features.nameMatch)')
const withHidden = (cases) => cases.map((c) => ({ ...c, ops: [...c.ops, hidden] }))
const uk = text("country", "uk")
const paris = { id: "Q10", name: "Paris", types: ["Q515"], score: 70 }
const op = (name, extra = {}, engineConfig = all) => ({ op: `core/${name}`, engineConfig, columnName: "name", ...extra })
const qids = addColumn("qid", 'if(value == "London", "Q12", if(value == "Paris", "http://ox.test/entity/Q10", value))', "city")

export const CASES = [
  { name: "no type", ops: [recon("name")] },
  { name: "type human", ops: [recon("name", human)] },
  { name: "type city", ops: [recon("city", city)] },
  { name: "no auto-match", ops: [recon("name", { ...human, autoMatch: false })] },
  { name: "limit 2", ops: [recon("name", { ...human, limit: 2 })] },
  { name: "property from column", ops: [recon("city", { ...city, ...country("country") })] },
  { name: "property from reconciled column", ops: [
    recon("country", { type: { id: "Q6256", name: "country" } }),
    recon("name", { ...human, ...country("country", "P27") }),
  ] },
  { name: "text facet", ops: [recon("name", human, text("country", "uk"))] },
  { name: "batch size 2", ops: [recon("name", { ...human, batchSize: 2 })] },
  { name: "service error 500", ops: [recon("name", { ...svc("/svc500"), ...human })] },
  { name: "service answers null", ops: [recon("name", { ...svc("/svcnull"), ...human })] },
  { name: "twice", ops: [recon("name", human), recon("name", { ...human, autoMatch: false })] },
  { name: "grel recon fields", ops: [
    recon("name", human),
    addColumn("j", 'cell.recon.judgment + "|" + cell.recon.judgmentAction + "|" + cell.recon.matched + "|" + cell.recon.new + "|" + cell.recon.judgmentBatchSize + "|" + cell.recon.matchRank'),
    addColumn("m", 'cell.recon.match.id + "|" + cell.recon.match.name + "|" + cell.recon.match.score + "|" + cell.recon.match.type.join(",")'),
    addColumn("b", 'cell.recon.best.id + "|" + cell.recon.best.score + "|" + cell.recon.candidates.length() + "|" + forEach(cell.recon.candidates, c, c.id).join(",")'),
    addColumn("f", 'cell.recon.features.typeMatch + "|" + cell.recon.features.nameMatch + "|" + cell.recon.features.nameLevenshtein + "|" + cell.recon.features.nameWordDistance'),
    addColumn("s", 'cell.recon.service.length() > 0 + "|" + cell.recon.identifierSpace + "|" + cell.recon.schemaSpace + "|" + cell.recon.error'),
    addColumn("t", 'type(cell.recon) + "|" + type(cell.recon.match) + "|" + type(cell.recon.candidates) + "|" + type(cell.recon.features)'),
  ] },
  { name: "grel recon of unreconciled", ops: [addColumn("r", 'type(cell.recon) + "|" + isNull(cell.recon)')] },
  { name: "transform keeps recon", ops: [
    recon("name", human),
    { op: "core/text-transform", engineConfig: all, columnName: "name", expression: "grel:value.toUppercase()", onError: "keep-original", repeat: false, repeatCount: 10 },
  ] },
  { name: "copy column keeps recon", ops: [
    recon("name", human),
    addColumn("copy", "value"),
    addColumn("copy cell", "cell"),
  ] },
  // ---------------------------------------------------------------- operations on Recons
  ...withHidden([
    { name: "match best candidates", ops: [recon("name", { ...human, autoMatch: false }), op("recon-match-best-candidates")] },
    { name: "match best candidates in facet", ops: [recon("name", { ...human, autoMatch: false }), op("recon-match-best-candidates", {}, uk)] },
    { name: "mark new, one each", ops: [recon("name", human), op("recon-mark-new-topics", { shareNewTopics: false })] },
    { name: "mark new, shared", ops: [recon("name", human), op("recon-mark-new-topics", { shareNewTopics: true })] },
    { name: "mark new on unreconciled", ops: [op("recon-mark-new-topics", { columnName: "city", shareNewTopics: true, service: "{BASE}/svc", identifierSpace: "http://ox.test/entity/", schemaSpace: "http://ox.test/prop/" })] },
    { name: "match specific topic", ops: [recon("name", human), op("recon-match-specific-topic-to-cells", { match: { id: "Q1", name: "Douglas Adams", types: ["Q5"] }, identifierSpace: "http://ox.test/entity/", schemaSpace: "http://ox.test/prop/" }, text("name", "doug"))] },
    { name: "match specific topic, unreconciled", ops: [op("recon-match-specific-topic-to-cells", { columnName: "city", match: { id: "Q12", name: "London", types: ["Q515"] }, identifierSpace: "http://ox.test/entity/", schemaSpace: "http://ox.test/prop/" }, text("city", "london"))] },
    { name: "discard judgments", ops: [recon("name", human), op("recon-discard-judgments", { clearData: false })] },
    { name: "discard judgments and data", ops: [recon("name", human), op("recon-discard-judgments", { clearData: true }, uk)] },
    { name: "clear similar cells", ops: [recon("name", human), op("recon-clear-similar-cells", { similarValue: "Paris" })] },
    { name: "judge similar: match", ops: [recon("name", human), op("recon-judge-similar-cells", { similarValue: "Paris", judgment: "matched", match: paris, shareNewTopics: false })] },
    { name: "judge similar: new, shared", ops: [recon("name", human), op("recon-judge-similar-cells", { similarValue: "Paris", judgment: "new", shareNewTopics: true })] },
    { name: "judge similar: new each", ops: [recon("name", human), op("recon-judge-similar-cells", { similarValue: "Paris", judgment: "new", shareNewTopics: false })] },
    { name: "judge similar: none", ops: [recon("name", human), op("recon-judge-similar-cells", { similarValue: "Douglas Adams", judgment: "none", shareNewTopics: false })] },
    { name: "judge similar: none keeps errors", ops: [recon("name", human), op("recon-judge-similar-cells", { similarValue: "Broken", judgment: "none", shareNewTopics: false })] },
    { name: "use values as identifiers", ops: [qids, op("recon-use-values-as-identifiers", { columnName: "qid", service: "{BASE}/svc", identifierSpace: "http://ox.test/entity/", schemaSpace: "http://ox.test/prop/" })] },
    { name: "copy judgments across columns", ops: [
      recon("city", city),
      addColumn("city2", "value", "city"),
      { op: "core/recon-copy-across-columns", engineConfig: all, fromColumnName: "city", toColumnNames: ["city2"], judgments: ["matched"], applyToJudgedCells: false },
    ] },
    { name: "copy unjudged too, over judged cells", ops: [
      recon("city", city),
      addColumn("city2", "value", "city"),
      recon("city2", { ...city, autoMatch: false }),
      { op: "core/recon-copy-across-columns", engineConfig: all, fromColumnName: "city", toColumnNames: ["city2"], judgments: ["matched", "none"], applyToJudgedCells: true },
    ] },
    // ---------------------------------------------------------------- judging one cell
    { name: "judge one cell: match a candidate", ops: [recon("name", human), { op: "ox/recon-judge-one-cell", row: 5, columnName: "name", judgment: "matched", match: { id: "Q11", name: "Paris", types: ["Q515"], score: 70 } }] },
    { name: "judge one cell: new", ops: [recon("name", human), { op: "ox/recon-judge-one-cell", row: 0, columnName: "name", judgment: "new" }] },
    { name: "judge one cell: none", ops: [recon("name", human), { op: "ox/recon-judge-one-cell", row: 3, columnName: "name", judgment: "none" }] },
    { name: "judge one cell: unreconciled", ops: [{ op: "ox/recon-judge-one-cell", row: 0, columnName: "city", judgment: "matched", match: { id: "Q12", name: "London", types: ["Q515"], score: 100 } }] },
    { name: "clear one cell", ops: [recon("name", human), { op: "ox/recon-clear-one-cell", row: 4, columnName: "name" }] },
    // ---------------------------------------------------------------- Recons through other steps
    { name: "fill down copies recon", ops: [recon("name", human), { op: "core/fill-down", engineConfig: all, columnName: "name" }] },
    { name: "blank down drops recon", ops: [recon("name", human), { op: "core/blank-down", engineConfig: all, columnName: "name" }] },
    { name: "mass edit keeps recon", ops: [recon("name", human), { op: "core/mass-edit", engineConfig: all, columnName: "name", expression: "value", edits: [{ from: ["Paris"], fromBlank: false, fromError: false, to: "Paris, France" }] }] },
    { name: "rows removed and sorted", ops: [recon("name", human), { op: "core/row-removal", engineConfig: uk }, { op: "core/row-reorder", mode: "row-based", sorting: { criteria: [{ column: "name", valueType: "string", reverse: false, blankPosition: 2, errorPosition: 1, caseSensitive: false }] } }] },
    { name: "split column drops recon", ops: [recon("name", human), { op: "core/column-split", engineConfig: all, columnName: "name", guessCellType: false, removeOriginalColumn: false, mode: "separator", separator: " ", regex: false, maxColumns: 0 }] },
  ]),
]
