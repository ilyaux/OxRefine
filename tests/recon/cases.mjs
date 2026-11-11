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
]
