"""Portable payload tests: explicit inputs, complete notices, and verified bytes."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("package", Path(__file__).resolve().parents[1] / "scripts/package.py")
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


class PortablePackage(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name) / "source"
        names = [
            "README.md", "LICENSE", "THIRD_PARTY_LICENSES.md",
            "assets/README.md", "assets/fonts/README.md", "assets/fonts/Inter-LICENSE.txt",
            "assets/demo/RobotExpressive.glb", "assets/demo/RobotExpressive-SOURCE.md",
            "assets/demo/CC0-1.0.txt", "docs/third-party/README.md",
            "docs/third-party/example/LICENSE",
            "docs/images/animate.gif", "docs/images/retarget.gif", "docs/images/library.gif",
            *[f"assets/demo/motions/{name}.glb" for name in package.MOTIONS],
        ]
        for name in names:
            p = self.root / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(name.encode())
        self.binary = self.root / "target/release/armatra.exe"
        self.binary.parent.mkdir(parents=True)
        self.binary.write_bytes(b"MZ-test-payload")

    def test_only_explicit_demo_files_and_notices_are_packaged(self):
        (self.root / "assets/demo/unapproved.glb").write_bytes(b"excluded")
        (self.root / "target/build.log").write_bytes(b"excluded")
        (self.root / "RELEASE_NOTES.md").write_bytes(b"release-only notes")
        entries = package.payload(self.root, self.binary)
        self.assertNotIn("assets/demo/unapproved.glb", entries)
        self.assertNotIn("target/build.log", entries)
        self.assertNotIn("RELEASE_NOTES.md", entries)
        self.assertIn("docs/third-party/example/LICENSE", entries)

    def test_missing_notice_stops_packaging(self):
        (self.root / "assets/fonts/Inter-LICENSE.txt").unlink()
        with self.assertRaises(ValueError):
            package.payload(self.root, self.binary)

    def test_source_links_resolve_from_portable_readme(self):
        (self.root / "README.md").write_text("[code](src/rig.rs) [demo](assets/README.md)", encoding="utf-8")
        archive = package.create_archive(self.root, self.binary, self.root.parent / "out", "1.2.1")
        with zipfile.ZipFile(archive) as z:
            readme = z.read("armatra-v1.2.1-windows-x64/README.md").decode()
        self.assertIn("https://github.com/Speed126/Armatra/blob/main/src/rig.rs", readme)
        self.assertIn("[demo](assets/README.md)", readme)

    def test_archive_roundtrip_and_both_checksum_files(self):
        archive = package.create_archive(self.root, self.binary, self.root.parent / "out", "1.2.1")
        checksum = archive.with_suffix(".zip.sha256").read_text().split()[0]
        self.assertEqual(checksum, package.digest(archive.read_bytes()))
        with zipfile.ZipFile(archive) as z:
            prefix = "armatra-v1.2.1-windows-x64/"
            self.assertEqual(z.read(prefix + "armatra.exe"), self.binary.read_bytes())
            sums = z.read(prefix + "CONTENTS.sha256").decode().splitlines()
            for row in sums:
                digest, name = row.split("  ", 1)
                self.assertEqual(digest, package.digest(z.read(prefix + name)))
            self.assertEqual(len(z.namelist()), len(sums) + 1)


if __name__ == "__main__":
    unittest.main()
