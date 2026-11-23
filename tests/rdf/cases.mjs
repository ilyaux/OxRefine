// RDF imports checked against OpenRefine 3.10.1 (its RdfTripleImporter, through Apache Jena).
// `format` is OpenRefine's format id; inputs are in inputs/.
export const CASES = [
  { name: "turtle", file: "basic.ttl", format: "text/rdf/ttl" },
  { name: "turtle-as-n3", file: "basic.ttl", format: "text/rdf/n3" },
  { name: "turtle-literals", file: "literals.ttl", format: "text/rdf/ttl" },
  { name: "turtle-structure", file: "structure.ttl", format: "text/rdf/ttl" },
  { name: "turtle-relative-without-base", file: "nobase.ttl", format: "text/rdf/ttl" },
  { name: "turtle-empty", file: "empty.ttl", format: "text/rdf/ttl" },
  { name: "turtle-broken", file: "broken.ttl", format: "text/rdf/ttl", malformed: true },
  { name: "n-triples", file: "basic.nt", format: "text/rdf/nt" },
  { name: "rdf-xml", file: "people.rdf", format: "text/rdf/xml" },
  { name: "json-ld", file: "people.jsonld", format: "text/rdf/ld+json" },
]
// The import wizard's guess and defaults for each kind of file name
export const WIZARD = ["basic.ttl", "basic.nt", "people.rdf", "people.jsonld"]
