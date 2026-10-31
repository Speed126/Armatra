"""Strip unsupported facial morph deformation from the CC0 showcase.

TRS animation, hierarchy, skin palettes, geometry and material data are unchanged.
Unused binary/accessor payloads are preserved; the importer ignores them.
"""
import argparse
import hashlib
import json
from pathlib import Path
import struct

UPSTREAM_SHA256 = "047f5e5fb3bb6d378bd1df16ca6137f2a596c99b3a1b5690b4020c05aaf6f319"

def prepare(source, output):
    assert source.resolve() != output.resolve(), "Never overwrite the source"
    assert not output.exists(), "Choose a new output path"
    data = source.read_bytes()
    assert hashlib.sha256(data).hexdigest() == UPSTREAM_SHA256, "Unexpected upstream model"
    size = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20:20 + size])
    for animation in doc["animations"]:
        animation["channels"] = [c for c in animation["channels"] if c["target"]["path"] != "weights"]
    for node in doc["nodes"]:
        node.pop("weights", None)
    for mesh in doc["meshes"]:
        mesh.pop("weights", None)
        for primitive in mesh["primitives"]:
            primitive.pop("targets", None)
    doc.setdefault("asset", {}).setdefault("extras", {})["armatra_modification"] = "Facial morph channels, defaults and targets removed; skeletal TRS unchanged. CC0-1.0."
    text = json.dumps(doc, separators=(",", ":")).encode()
    text += b" " * (-len(text) % 4)
    tail = data[20 + size:]
    result = struct.pack("<4sII", b"glTF", 2, 20 + len(text) + len(tail)) + struct.pack("<II", len(text), 0x4e4f534a) + text + tail
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_bytes(result)
    print(hashlib.sha256(result).hexdigest())

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    prepare(args.source, args.output)
