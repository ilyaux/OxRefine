const cube = `CHARSET="ANSI";
AXIS-VERSION="2000";
TITLE="A statistical table";
CONTENTS="People";
UNITS="persons";
STUB="Country";
HEADING="Year";
VALUES("Year")="2022","2023","2024";
VALUES("Country")="Sweden","Finland";
CODES("Country")="SE","FI";
DATA=
1 2 .
.. "..." -6;
`
export const CASES = [
  { name:"cube-dimension-order-missing-markers", text:cube },
  { name:"skip-limit", text:cube, options:{skipDataLines:2,limit:3} },
  { name:"limit-zero", text:cube, options:{limit:0} },
  { name:"forced-header-blank-settings", text:cube, options:{ignoreLines:99,headerLines:0,storeBlankRows:false,storeBlankCellsAsNulls:false} },
  { name:"typed-trim-explicit", text:cube, options:{guessCellValueTypes:true,trimStrings:true} },
  { name:"three-dimensions", text:`STUB="Area","Sex";\nHEADING="Time";\nVALUES("Time")="2023","2024";\nVALUES("Area")="A","B";\nVALUES("Sex")="F","M";\nDATA=1 2 3 4 5 6 7 8;\n` },
  { name:"multiline-values-and-string-continuation", text:`STUB="Kind";\nHEADING="Year";\nVALUES("Kind")="Long "\n"label","Other";\nVALUES("Year")="2023",\n"2024";\nDATA=1\t2\n3 4;\n` },
  { name:"quoted-punctuation-and-empty-dimension", text:`STUB="value";\nVALUES("value")="a,b;=c",""," spaced ";\nDATA=1 2 3;\n` },
  { name:"scalar", text:`TITLE="No dimensions";\nDATA=12.5;\n` },
  { name:"unquoted-unicode-metadata", text:`TITLE=é;\nSTUB="Kind";\nVALUES("Kind")=東京;\nDATA=1;\n` },
  { name:"data-semicolon-discards-rest-of-line", text:`TITLE="Scalar values";\nDATA=1; 2\n3; 4\n` },
  { name:"unicode-whitespace-after-ascii-boundary", text:`DATA=1 \u2003 2\t\u2003 3;\n` },
  { name:"unicode-whitespace-inside-token", text:`DATA=1\u20032;\n` },
  { name:"numeric-forms-stay-text", text:`STUB="Item";\nVALUES("Item")="a","b","c","d","e","f";\nDATA=001 -2 +3 1.25 1e3 NaN;\n` },
  { name:"utf8-labels", text:`CHARSET="UTF-8";\nSTUB="Регион";\nVALUES("Регион")="Åland","東京";\nDATA=1 2;\n`, options:{encoding:"UTF-8"} },
  { name:"windows1252-labels", text:`CHARSET="ANSI";\nSTUB="Région";\nVALUES("Région")="Åland","Café €";\nDATA=1 2;\n`, encoding:"windows-1252",options:{encoding:"windows-1252"} },
  { name:"windows1252-blank-encoding", text:`CHARSET="UTF-8";\nSTUB="Région";\nVALUES("Région")="Åland","Café €";\nDATA=1 2;\n`, encoding:"windows-1252",options:{encoding:""} },
  { name:"utf8-charset-does-not-override-option", text:`CHARSET="ANSI";\nSTUB="Регион";\nVALUES("Регион")="Åland","東京";\nDATA=1 2;\n`, options:{encoding:"UTF-8"} },
  { name:"crlf-data-and-metadata", text:cube.replaceAll("\n","\r\n") },
  { name:"cr-data-and-metadata", text:cube.replaceAll("\n","\r") },
  { name:"metadata-localized-values-ignored", text:`STUB="Kind";\nVALUES("Kind")="A","B";\nVALUES[en]("Kind")="English A","English B";\nNOTE("Kind")="A note";\nDATA=1 2;\n` },
  { name:"no-data-section", text:`STUB="Kind";\nVALUES("Kind")="A";\n`, malformed:true },
  { name:"too-few-data-points", text:`STUB="Kind";\nVALUES("Kind")="A","B";\nDATA=1;\n`, malformed:true },
  { name:"too-many-data-points", text:`STUB="Kind";\nVALUES("Kind")="A";\nDATA=1 2;\n`, malformed:true },
  { name:"missing-dimension-order", text:`VALUES("Kind")="A","B";\nVALUES("Other")="C";\nDATA=1 2;\n`, malformed:true },
  { name:"unclosed-quoted-value", text:`STUB="Kind";\nVALUES("Kind")="A;\nDATA=1;\n`, malformed:true },
]
