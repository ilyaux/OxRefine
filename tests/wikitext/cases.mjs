export const DEFAULTS = {
  encoding: "UTF-8", headerLines: 1, ignoreLines: -1, skipDataLines: 0, limit: -1,
  storeBlankRows: true, storeBlankColumns: true, storeBlankCellsAsNulls: true,
  guessCellValueTypes: false, blankSpanningCells: true, includeRawTemplates: false,
  parseReferences: true, wikiUrl: null, includeFileSources: false, includeArchiveFileName: false,
}
const basic = "{| class=\"wikitable\"\n! Name !! Value\n|-\n| Alpha || 42\n|-\n| Beta || true\n|}\n"
const spanning = "{|\n! A !! B !! C\n|-\n| rowspan=\"2\" | [[Alpha|A]] || colspan=2 | Wide\n|-\n| X || Y\n|-\n| Z || W || V\n|}\n"
const refs = "{|\n! Name !! Value\n|-\n| A<ref name=\"one\">https://example.test/first</ref>suffix || B<ref>https://example.test/old https://example.test/latest.</ref>tail\n|-\n| C<ref name=\"one\" />after || D\n|}\n"
const templates = "{|\n! A !! B\n|-\n| pre {{T|x=one|two}} post || {{Outer|{{Inner|a}}|z=last}}\n|-\n| [[File:Picture.png|thumb|caption]] || {{{parameter|fallback}}}\n|}\n"
const blanks = "{|\n! A !! !! C\n|-\n| one || || three\n|-\n| || ||\n|-\n| two || ||\n|}\n"
const table = (name, text, options = {}) => ({ name, text, options: { ...DEFAULTS, ...options } })
export const CASES = [
  table("basic", basic),
  table("no headers", basic, { headerLines: 0 }),
  table("two headers", basic, { headerLines: 2 }),
  table("guess types on Cell values", basic, { guessCellValueTypes: true }),
  table("trimmed cells", "{|\n!  A  !! B\n|-\n|  alpha  ||  12  \n|}\n"),
  table("all tables concatenate", basic + "\n== Next ==\n{|\n! Other !! Data\n|-\n| third || value\n|}\n"),
  table("span blanks", spanning),
  table("span repeated", spanning, { blankSpanningCells: false }),
  table("combined span", "{|\n! A !! B !! C\n|-\n| rowspan=2 colspan=2 | Both || End\n|-\n| Last\n|}\n", { blankSpanningCells: false }),
  table("invalid spans", "{|\n! A !! B\n|-\n| rowspan=oops colspan=-2 | Bad || Good\n|-\n| Zero || colspan=0 | Last\n|}\n"),
  table("internal links", "{|\n! A !! B !! C\n|-\n| [[Alpha]]s || [[Beta|Shown]] || [[One]] and [[Two]]\n|-\n| [[:Category:Test|Category label]] || [[Category:Hidden]] || [[Alpha#Section|Section]]\n|}\n"),
  table("external links", "{|\n! A !! B !! C\n|-\n| [https://example.test/path Label] || https://example.test/bare || [ftp://example.test/file]\n|}\n"),
  table("templates removed", templates),
  table("raw templates", templates, { includeRawTemplates: true }),
  table("reference columns", refs),
  table("references hidden", refs, { parseReferences: false }),
  table("reference then links", "{|\n! A\n|-\n| Start<ref>https://example.test/ref</ref>[[Alpha|Label]] [https://example.test/after X] tail\n|}\n"),
  table("URL-valued reference names", "{|\n! A\n|-\n| A<ref name=\"https://example.test/key\">plain citation</ref>tail\n|-\n| B<ref name=\"https://example.test/key\"/>tail\n|}\n"),
  table("URL after inline reference markup", "{|\n! A\n|-\n| A<ref><i>Citation</i> https://example.test/real</ref>tail\n|}\n"),
  table("no URL references", "{|\n! A\n|-\n| A<ref>plain citation</ref>tail\n|-\n| B<ref name=missing/>tail\n|}\n"),
  table("blank rows columns kept", blanks),
  table("blank columns dropped", blanks, { storeBlankColumns: false }),
  table("blank rows dropped", blanks, { storeBlankRows: false }),
  table("empty strings", blanks, { storeBlankCellsAsNulls: false }),
  table("skip and limit", basic, { skipDataLines: 1, limit: 1 }),
  table("ignore row", basic, { ignoreLines: 1, headerLines: 0 }),
  table("zero limit", basic, { limit: 0 }),
  table("incomplete header", basic, { headerLines: 10 }),
  table("span blanks as strings", spanning, { storeBlankCellsAsNulls: false }),
  table("HTML entities tags", "{|\n! A !! B\n|-\n| &amp; &#65; &nbsp; &#x1F600; || <span class=x>Text</span><br />line<br>end\n|-\n| <nowiki>[[literal]] {{template}}</nowiki> || '''Bold''' and ''italic''\n|}\n"),
  table("comments and pipes", "{|\n! A !! B\n|-\n| x<!-- hidden || pipe -->y || [[Target|Label with | pipe]]\n|}\n"),
  table("multiline content", "{|\n! A !! B\n|-\n| first\nsecond\n|\n* one\n* two\n|}\n"),
  table("linebreak tag case and blank line", "{|\n! A !! B\n|-\n| a<br>b<BR>c<br/>d</br>e || first\n\nsecond\n|}\n"),
  table("captions", "{|\n|+ First ''caption''\n! A\n|-\n| row\n|}\n{|\n|+ Second\n! B\n|-\n| next\n|}\n"),
  table("empty table", "{|\n|}\n"),
  table("no table", "Plain text [[Link]] and {{Template}}\n"),
  table("unclosed table", "{|\n! A !! B\n|-\n| one || two\n"),
  table("nested table", "{|\n! Outer !! End\n|-\n| before\n{|\n! Inner\n|-\n| inside\n|}\nafter\n| last\n|}\n"),
  table("local reconciliation", "{|\n! A !! B\n|-\n| [[Alpha|Label]] || [[Beta]]\n|-\n| [[Alpha]] and [[Beta]] || [[Alpha]]\n|}\n", { wikiUrl: "https://en.wikipedia.org/wiki/", reconService: "{SERVICE}" }),
  table("header-only reconciliation config", "{|\n! [[Alpha]]\n|-\n| Unlinked body\n|}\n", { wikiUrl: "https://en.wikipedia.org/wiki/", reconService: "{SERVICE}" }),
  table("skipped reconciliation config", "{|\n! A\n|-\n| [[Alpha]]\n|-\n| Unlinked body\n|}\n", { wikiUrl: "https://en.wikipedia.org/wiki/", reconService: "{SERVICE}", skipDataLines: 1 }),
  table("local reconciliation with reference", "{|\n! A !! B\n|-\n| First<ref>https://example.test/ref</ref> || [[Alpha|Label]]\n|}\n", { wikiUrl: "https://en.wikipedia.org/wiki/", reconService: "{SERVICE}" }),
]
CASES.find((c) => c.name === "local reconciliation with reference").knownDifference = "OpenRefine assigns reconciliation configuration using column indexes before reference-column insertion; Ox keeps it with the actual reconciled output column."
