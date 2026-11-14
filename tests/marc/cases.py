"""Authored ISO2709 inputs. Expected XML/tables are generated separately by the real reference."""
import gzip
import json
from pathlib import Path
import struct

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
maps = list(struct.iter_unpack(">IIB", gzip.decompress((ROOT / "crates/ox-core/assets/marc8/tables.bin.gz").read_bytes())))


def graphic(text, charset):
    reverse = {value: code & 0xffffff for code, value, _ in reversed(maps) if code >> 24 == charset}
    size = 3 if charset == 0x31 else 1
    return b"".join(reverse[ord(c)].to_bytes(size, "big") for c in text)


def data(*fields, indicators=b"10"):
    return indicators + b"".join(b"\x1f" + code.encode("ascii") + (value.encode("utf-8") if isinstance(value, str) else value) for code, value in fields)


def record(fields, coding="a"):
    fields = [(tag, value.encode("utf-8") if isinstance(value, str) else value) for tag, value in fields]
    directory = bytearray()
    body = bytearray()
    for tag, value in fields:
        directory += f"{tag}{len(value) + 1:04}{len(body):05}".encode("ascii")
        body += value + b"\x1e"
    base = 24 + len(directory) + 1
    leader = bytearray(f"00000nam {coding}2200000   4500".encode("ascii"))
    assert len(leader) == 24
    leader[:5] = f"{base + len(body) + 1:05}".encode("ascii")
    leader[12:17] = f"{base:05}".encode("ascii")
    return bytes(leader + directory + b"\x1e" + body + b"\x1d")


cases = []


def add(name, value, **options):
    file = name + ".mrc"
    (HERE / file).write_bytes(value)
    cases.append({"name": name, "file": file, **options})


ascii_record = record([("001", "test-1"), ("008", "241114s2024    xx            000 0 eng d"),
    ("100", data(("a", "Doe, Jane"), ("d", "1970-"))), ("245", data(("a", "Data & catalogues <test>."), ("b", "A sample."))),
    ("650", data(("a", "Libraries"), ("x", "Data processing"), indicators=b" 0")), ("650", data(("a", "Names"), indicators=b" 0"))])
add("ascii", ascii_record)
add("utf8", record([("001", "unicode"), ("245", data(("a", "Русский Ελληνικά العربية עברית 中文 日本語 😀")))]))
add("utf8-decomposed", record([("245", data(("a", "Cafe\u0301 e\u0308")))]))
add("utf8-blank-flag", record([("245", data(("a", "Русский текст и названия городов")))], " "))
add("marc8-ansel", record([("245", data(("a", b"Caf\xe2e / M\xe8uller / \xa1")))], " "))
add("marc8-combining", record([("245", data(("a", b"\xe2\xe8e"), ("b", b"a\xe2e")))], " "))
add("marc8-scripts", record([("245", data(("a", b"\x1b(N" + graphic("Тест", 0x4e)), ("b", b"\x1b(3" + graphic("كتاب", 0x33)),
    ("c", b"\x1b(2" + graphic("ספר", 0x32))))], " "))
add("marc8-greek", record([("245", data(("a", b"\x1b(S" + graphic("Αλφα", 0x53) + b"\x1b(B ASCII")))], " "))
add("marc8-eacc", record([("245", data(("a", b"\x1b$1" + graphic("中文", 0x31) + b"\x1b(B / end")))], " "))
corrections = json.loads((ROOT / "crates/ox-core/assets/marc8/provenance.json").read_text())["differencesFromCurrentLoC"]
correct_text = "".join(chr(int(c["loc"], 16)) for c in corrections)
add("marc8-supplementary-eacc", record([("245", data(("a", b"\x1b$1" + graphic(correct_text, 0x31) + b"\x1b(B")))], " "),
    knownDifference="MARC4J truncates three supplementary EACC Unicode scalars to 16 bits; OxRefine uses their exact Library of Congress scalar mappings.", locUnicode=correct_text)
add("marc8-ncr", record([("245", data(("a", b"\x1b(B&#x00E9; and <U+4E2D>")))], " "))
add("escaped-entities", record([("001", "A &amp; B"), ("245", data(("a", "&lt;title&gt; &amp;lt; &apos; &quot;")))]))
add("empty-uppercase-subfields", record([("245", data(("a", ""), ("A", "Title"), ("b", ""), ("C", "Author")))]))
add("interleaved-fields", record([("245", data(("a", "Title"))), ("001", "after-title"), ("650", data(("a", "Subject"))), ("003", "local")]))
add("multiple-records", ascii_record + record([("001", "second"), ("245", data(("a", "Вторая запись")))]))
add("limit-one", ascii_record + record([("001", "second")]), options={"limit": 1})
add("limit-zero", ascii_record, options={"limit": 0})
add("subfield-record-path", ascii_record, options={"recordPath": ["collection", "record", "datafield", "subfield"]})
add("truncated-leader", ascii_record[:17], reject=True)
add("truncated-record", ascii_record[:-5], reject=True)
broken = bytearray(ascii_record); broken[-1] = 0x20
add("missing-record-terminator", broken, reject=True)
broken = bytearray(ascii_record); broken[12:17] = b"99999"
add("invalid-base", broken, reject=True)
broken = bytearray(ascii_record); broken[27:31] = b"9999"
add("invalid-field-length", broken, reject=True)
broken = bytearray(ascii_record); broken[27:31] = b"abcd"
add("nonnumeric-directory", broken, reject=True)
add("invalid-utf8", record([("245", data(("a", b"bad\xc3(")))]), reject=True, knownDifference="OxRefine rejects malformed UTF-8 explicitly; Marc4j attempts other encodings and repairs.")
add("unknown-marc8-escape", record([("245", data(("a", b"\x1b(B\x1b%Zbad")))], " "), reject=True, knownDifference="OxRefine rejects unknown MARC-8 escape designations; Marc4j's permissive reader repairs some malformed sequences.")
(HERE / "cases.json").write_text(json.dumps(cases, indent=1) + "\n", encoding="utf-8")
print(len(cases), "authored MARC sources")
