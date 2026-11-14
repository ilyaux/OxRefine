These are standardized MARC-8 code-point mapping facts, generated through the
MARC4J 2.9.6 `CodeTableGenerated` public API and checked against the Library of
Congress XML character tables. The Library of Congress provides those tables
for use in software applications:
https://www.loc.gov/marc/specifications/specchartables.html

`provenance.json` records the mapping-source URL/hash, exact reference JAR and
release ZIP hashes, the generated table hash, coverage and differences from the
current Library of Congress mapping. Three supplementary EACC characters use
their exact LoC Unicode scalar values: MARC4J's 16-bit return type truncates those
values to unrelated BMP characters. This deliberate correction is recorded in
the real-reference fixtures and provenance.

The ISO2709/MARC-8 parser in `src/import_marc.rs` is an independent implementation
of the record format and graphic-set designations. No MARC4J implementation
source or bytecode is copied, adapted or bundled. MARC4J's implementation is
LGPL-2.1-or-later; it is used only by the reference generators in tests/marc/.
The Library of Congress XML mapping file contains no separate copyright notice.

Regenerate mapping data with `python tests/marc/tables.py`. Generate authored
record inputs with `python tests/marc/cases.py`, then obtain actual MARC4J XML
and OpenRefine wizard tables with `node tests/marc/gen.mjs`.
