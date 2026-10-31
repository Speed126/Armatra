"""Fetch optional Fox interoperability fixture from its official upstream."""
from pathlib import Path
import hashlib
import os
import urllib.request
root = Path(__file__).resolve().parent.parent
# Model: PixelMannen (CC0-1.0); rig/animation: tomkranis (CC-BY-4.0);
# glTF conversion: AsoboStudio and scurest (CC-BY-4.0). External test input only.
# https://github.com/KhronosGroup/glTF-Sample-Assets/tree/main/Models/Fox
url = "https://raw.githubusercontent.com/KhronosGroup/glTF-Sample-Assets/main/Models/Fox/glTF-Binary/Fox.glb"
expected = "d97044e701822bac5a62696459b27d7b375aada5de8574ed4362edbba94771f7"
output = Path(os.environ.get("CARGO_TARGET_DIR", root / "target")) / "external-fixtures/Fox.glb"
data = urllib.request.urlopen(url, timeout=60).read()
assert hashlib.sha256(data).hexdigest() == expected, "Upstream fixture changed; review provenance before updating"
output.parent.mkdir(parents=True, exist_ok=True)
output.write_bytes(data)
print("Fox fixture verified:", expected)
