"""Build a portable Windows x64 ZIP and its SHA-256 checksum."""
from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MOTIONS = ("Fast-jump", "Fast-walk", "Fast-wave", "Idle", "Jump", "Slow-walk", "Sway", "Walk", "Wave")


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def payload(root: Path, binary: Path) -> dict[str, Path]:
    names = [
        "README.md", "LICENSE", "THIRD_PARTY_LICENSES.md",
        "assets/README.md", "assets/fonts/README.md", "assets/fonts/Inter-LICENSE.txt",
        "assets/demo/RobotExpressive.glb", "assets/demo/RobotExpressive-SOURCE.md",
        "assets/demo/CC0-1.0.txt",
        "docs/images/animate.gif", "docs/images/retarget.gif", "docs/images/library.gif",
        *[f"assets/demo/motions/{name}.glb" for name in MOTIONS],
    ]
    notices = root / "docs/third-party"
    if not (notices / "README.md").is_file():
        raise FileNotFoundError("Dependency license notices are missing")
    names += [p.relative_to(root).as_posix() for p in sorted(notices.rglob("*")) if p.is_file()]
    result = {"armatra.exe": binary}
    for name in names:
        p = root / name
        if p.is_symlink() or not p.is_file() or not p.resolve().is_relative_to(root.resolve()):
            raise ValueError(f"Missing or unsafe package input: {name}")
        result[name] = p
    if not binary.is_file():
        raise FileNotFoundError(binary)
    return result


def create_archive(root: Path, binary: Path, output: Path, version: str) -> Path:
    entries = payload(root, binary)
    output.mkdir(parents=True, exist_ok=True)
    name = f"armatra-v{version}-windows-x64"
    archive = output / f"{name}.zip"
    contents = {name: p.read_bytes() for name, p in entries.items()}
    # Portable documentation links to unbundled source resolve on GitHub.
    for document in ("README.md",):
        text = contents[document].decode("utf-8")
        def resolve(match: re.Match[str]) -> str:
            link = match.group(1)
            if link.startswith(("https:", "http:", "#", "mailto:")) or link.split("#")[0] in entries:
                return match.group(0)
            return "](https://github.com/Speed126/Armatra/blob/main/" + link + ")"
        contents[document] = re.sub(r"\]\(([^)]+)\)", resolve, text).encode("utf-8")
    sums = "".join(f"{digest(data)}  {name}\n" for name, data in sorted(contents.items()))
    contents["CONTENTS.sha256"] = sums.encode()
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as z:
        for relative, data in sorted(contents.items()):
            z.writestr(f"{name}/{relative}", data)
    with zipfile.ZipFile(archive) as z:
        if z.testzip() is not None:
            raise RuntimeError("ZIP integrity check failed")
        for relative, data in contents.items():
            if z.read(f"{name}/{relative}") != data:
                raise RuntimeError(f"ZIP contents differ: {relative}")
    archive.with_suffix(".zip.sha256").write_text(
        f"{digest(archive.read_bytes())}  {archive.name}\n", encoding="utf-8"
    )
    return archive


def build_environment(target: Path) -> dict[str, str]:
    env = os.environ.copy()
    flags = env.get("CARGO_ENCODED_RUSTFLAGS", "").split("\x1f") if env.get("CARGO_ENCODED_RUSTFLAGS") else shlex.split(env.get("RUSTFLAGS", ""))
    if "-C" not in flags or "target-feature=+crt-static" not in flags:
        flags += ["-C", "target-feature=+crt-static"]
    for source, replacement in [
        (ROOT, "/armatra"), (Path.home(), "/build-user"),
        (Path(env.get("CARGO_HOME", Path.home() / ".cargo")), "/cargo"),
        (target, "/target"),
    ]:
        for prefix in dict.fromkeys([str(source.resolve()), source.resolve().as_posix()]):
            flag = f"--remap-path-prefix={prefix}={replacement}"
            if flag not in flags:
                flags.append(flag)
    env["CARGO_ENCODED_RUSTFLAGS"] = "\x1f".join(flags)
    env["CARGO_TARGET_DIR"] = str(target)
    env.setdefault("ARMATRA_BUILD_COMMIT", "local")
    env.pop("RUSTFLAGS", None)
    return env


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--target-directory", type=Path, default=Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target")))
    args = parser.parse_args()
    if os.name != "nt" or platform.machine().lower() not in ("amd64", "x86_64"):
        parser.error("The portable package is built on Windows x64")
    target = args.target_directory.resolve()
    cargo = shutil.which("cargo")
    if cargo is None:
        parser.error("Cargo must be available on PATH")
    subprocess.run([cargo, "build", "--locked", "--release"], cwd=ROOT, env=build_environment(target), check=True)
    version = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]["version"]
    archive = create_archive(ROOT, target / "release/armatra.exe", args.output.resolve(), version)
    print(archive)
    print(archive.with_suffix(".zip.sha256"))


if __name__ == "__main__":
    main()
