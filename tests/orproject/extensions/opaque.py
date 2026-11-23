# Derive opaque.openrefine.tar.gz from the OpenRefine-generated wikibase.openrefine.tar.gz:
# the same archive with an overlay model OxRefine doesn't know (`example`) in data.txt, so it
# keeps exercising the preserved-original path now that the Wikibase schema is understood.
#   python -I tests/orproject/extensions/opaque.py
import io, pathlib, tarfile, zipfile

here = pathlib.Path(__file__).parent
src = tarfile.open(here / "wikibase.openrefine.tar.gz", "r:gz")
out = io.BytesIO()
with tarfile.open(fileobj=out, mode="w:gz") as dst:
    for member in src.getmembers():
        data = src.extractfile(member).read() if member.isfile() else None
        if member.name.lstrip("./") == "data.zip":
            zin = zipfile.ZipFile(io.BytesIO(data))
            buf = io.BytesIO()
            with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as zout:
                for info in zin.infolist():
                    body = zin.read(info.filename)
                    if info.filename == "data.txt":
                        body = body.replace(b"rowCount=", b'overlayModel:example={"column":"name","rows":[0]}\nrowCount=', 1)
                    zout.writestr(info.filename, body)
            data = buf.getvalue()
            member.size = len(data)
        dst.addfile(member, io.BytesIO(data) if data is not None else None)
(here / "opaque.openrefine.tar.gz").write_bytes(out.getvalue())
print("wrote", here / "opaque.openrefine.tar.gz")
