"""Generate compact MARC-8 mapping facts; no MARC4J implementation is redistributed."""
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import urllib.request
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]
REFERENCE = Path(os.environ.get("OPENREFINE_HOME", ROOT / "target/reference-3.10.1/openrefine-3.10.1"))
JAR = REFERENCE / "webapp/WEB-INF/lib/marc4j-2.9.6.jar"
JAR_HASH = "7a770fd4f35adc77ef09c1dbafd12de73895d80627aa056742bc74d7cc5d6995"
assert hashlib.sha256(JAR.read_bytes()).hexdigest() == JAR_HASH
DEST = ROOT / "crates/ox-core/assets/marc8"
DEST.mkdir(parents=True, exist_ok=True)
source = ROOT / "target/marc-research/codetables.xml"
if not source.exists():
    source.parent.mkdir(parents=True, exist_ok=True)
    urllib.request.urlretrieve("https://www.loc.gov/marc/specifications/codetables.xml", source)
assert hashlib.sha256(source.read_bytes()).hexdigest() == "3e54ea62d494671cb07b3c931eda76c7f5052b204722bf576038ecbb8fb958c5", "LoC mapping source changed; review and repin before regeneration"
work = Path(tempfile.mkdtemp(prefix="marc-tables-", dir=ROOT / "target")).resolve()
try:
    subprocess.run(["javac", "--release", "11", "-cp", str(JAR), "-d", str(work), str(Path(__file__).with_name("ExportTables.java"))], check=True)
    subprocess.run([str(REFERENCE / "server/target/jre/bin/java.exe"), "-cp", str(work) + os.pathsep + str(JAR), "ExportTables", str(work / "tables.bin")], check=True)
    reference_raw = (work / "tables.bin").read_bytes()
    maps = {key: (value, bool(combining)) for key, value, combining in struct.iter_unpack(">IHB", reference_raw)}
    differences = []
    checked = 0
    for charset in ET.parse(source).getroot().iter("characterSet"):
        iso = int(charset.attrib["ISOcode"], 16)
        for code in charset.iter("code"):
            marc = code.findtext("marc", "").strip()
            ucs = code.findtext("ucs", "").strip()
            if not marc or not ucs:
                continue
            checked += 1
            expected = int(ucs, 16)
            actual = maps.get((iso << 24) | int(marc, 16), (0, False))[0]
            if actual != expected:
                decision = "reference mapping retained"
                if expected > 0xffff and actual == expected & 0xffff:
                    key = (iso << 24) | int(marc, 16)
                    maps[key] = (expected, maps[key][1])
                    decision = "Use the LoC Unicode scalar; the reference char API truncates it to 16 bits."
                differences.append({"set": f"{iso:02X}", "marc": marc, "loc": ucs, "reference": f"{actual:04X}", "decision": decision})
    raw = b"".join(struct.pack(">IIB", key, value, combining) for key, (value, combining) in maps.items())
    (DEST / "tables.bin.gz").write_bytes(gzip.compress(raw, compresslevel=9, mtime=0))
    metadata = {"mappingSource": "Library of Congress MARC-8 character mappings", "sourceUrl": "https://www.loc.gov/marc/specifications/codetables.xml",
                "sourceSha256": hashlib.sha256(source.read_bytes()).hexdigest(), "referenceLibrary": "org.marc4j:marc4j:2.9.6", "referenceJarSha256": JAR_HASH,
                "referenceRelease": "https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip",
                "referenceReleaseSha256": "6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8",
                "mappingEntries": len(maps), "referenceMappingSha256": hashlib.sha256(reference_raw).hexdigest(), "uncompressedSha256": hashlib.sha256(raw).hexdigest(), "locEntriesChecked": checked, "differencesFromCurrentLoC": differences,
                "format": "Repeated big-endian u32 (ISO set <<24 | MARC code), u32 Unicode scalar, u8 combining flag.",
                "scope": "11 single-byte graphic sets, all EACC triples with bytes 20..7E, as mapped by the reference release.",
                "licensing": "Code-point correspondences are standardized mapping facts. LoC publishes XML tables for use in software (specchartables.html). No MARC4J Java implementation is copied or adapted; that implementation is LGPL-2.1-or-later. No separate copyright notice appears in the LoC mapping XML."}
    (DEST / "provenance.json").write_text(json.dumps(metadata, indent=1) + "\n", encoding="utf-8")
    print("entries", len(maps), "gzip bytes", (DEST / "tables.bin.gz").stat().st_size, "LoC checks", checked, "differences", len(differences))
finally:
    assert work.parent == (ROOT / "target").resolve() and work.name.startswith("marc-tables-")
    shutil.rmtree(work)
