"""Repackage original Optimaize 0.6 profiles and reference API lookup data, without retraining."""
import gzip
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

REPO = Path(__file__).resolve().parents[2]
REFERENCE = Path(os.environ.get("OPENREFINE_HOME", REPO / "target/reference-3.10.1/openrefine-3.10.1"))
JAR = REFERENCE / "webapp/WEB-INF/lib/language-detector-0.6.jar"
EXPECTED = "f53ecc3d71da9ebc82edd10fb35638d32e8b9d849273dd717a021eca02f2278d"
assert hashlib.sha256(JAR.read_bytes()).hexdigest() == EXPECTED, "wrong Optimaize reference jar"
DEST = REPO / "crates/ox-core/assets/language-detector"
DEST.mkdir(parents=True, exist_ok=True)
work = Path(tempfile.mkdtemp(prefix="detectlang-build-", dir=REPO / "target")).resolve()
try:
    classpath = os.pathsep.join(str(REFERENCE / p) for p in ["webapp/WEB-INF/lib/*", "server/target/lib/*"])
    subprocess.run(["javac", "--release", "11", "-cp", classpath, "-d", str(work), str(Path(__file__).with_name("ExportModel.java"))], check=True)
    subprocess.run([str(REFERENCE / "server/target/jre/bin/java.exe"), "-cp", str(work) + os.pathsep + classpath,
                    "ExportModel", str(work / "characters.bin"), str(work / "model.json")], check=True)
    model = json.loads((work / "model.json").read_text(encoding="utf-8"))
    (DEST / "characters.bin.gz").write_bytes(gzip.compress((work / "characters.bin").read_bytes(), compresslevel=9, mtime=0))
    hashes = {}
    with zipfile.ZipFile(JAR) as source, zipfile.ZipFile(DEST / "profiles.zip", "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as out:
        for language in model["languageOrder"]:
            name = "languages/" + language
            content = source.read(name)
            hashes[name] = hashlib.sha256(content).hexdigest()
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            out.writestr(info, content, compresslevel=9)
        (DEST / "LICENSE.txt").write_bytes(source.read("LICENSE"))
        (DEST / "UPSTREAM-README.md").write_bytes(source.read("README.md"))
    model.update({"library": "Optimaize language-detector 0.6", "source": "https://github.com/optimaize/language-detector/tree/language-detector-0.6",
                  "jarSha256": EXPECTED, "profiles": hashes,
                  "referenceRelease": "https://github.com/OpenRefine/OpenRefine/releases/download/3.10.1/openrefine-win-with-java-3.10.1.zip",
                  "referenceReleaseSha256": "6155b20a8103244a26a8f21d41698ee52251538a838e3d5f2d5af507c8b6f3c8",
                  "charactersSha256": hashlib.sha256((work / "characters.bin").read_bytes()).hexdigest(),
                  "referenceJavaSha256": hashlib.sha256((REFERENCE / "server/target/jre/bin/java.exe").read_bytes()).hexdigest(),
                  "changes": "Profiles are byte-identical. ZIP container is repacked. Character normalization and Unicode script data are exhaustive BMP reference API outputs; profile iteration order comes from the actual detector builder."})
    (DEST / "provenance.json").write_text(json.dumps(model, ensure_ascii=False, indent=1) + "\n", encoding="utf-8")
    print("profiles", len(hashes), "ZIP bytes", (DEST / "profiles.zip").stat().st_size, "lookup gzip bytes", (DEST / "characters.bin.gz").stat().st_size)
finally:
    assert work.parent == (REPO / "target").resolve() and work.name.startswith("detectlang-build-")
    shutil.rmtree(work)
