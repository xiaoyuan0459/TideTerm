#!/usr/bin/env python3
"""Generate a minimal CycloneDX SBOM from TideTerm's Cargo.lock files.

This is intentionally lightweight: it parses the Rust lockfiles that are
checked into the repository and emits a single sbom.json containing the
unique third-party crates. It does not require network access or extra
Rust tooling.
"""

from __future__ import annotations

import json
import sys
import tomllib
from pathlib import Path
from urllib.parse import quote

ROOT = Path(__file__).resolve().parent.parent
LOCKFILES = [
    ROOT / "native" / "Cargo.lock",
    ROOT / "crates" / "tiderm-core" / "Cargo.lock",
]
OUT = ROOT / "sbom.json"


def load_packages(path: Path) -> list[dict]:
    with path.open("rb") as f:
        data = tomllib.load(f)
    return data.get("package", [])


def purl(name: str, version: str) -> str:
    # pkg:cargo/<url-encoded-name>@<version>
    return f"pkg:cargo/{quote(name, safe='')}@{quote(version, safe='')}"


def main() -> int:
    components: dict[tuple[str, str], dict] = {}
    for lock in LOCKFILES:
        for pkg in load_packages(lock):
            name = pkg.get("name")
            version = pkg.get("version")
            if not name or not version:
                continue
            key = (name, version)
            if key in components:
                continue
            component: dict = {
                "type": "library",
                "name": name,
                "version": version,
                "purl": purl(name, version),
                "bom-ref": purl(name, version),
            }
            checksum = pkg.get("checksum")
            if checksum:
                component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
            components[key] = component

    sbom = {
        "bomFormat": "CycloneDX",
        "specVersion": "1.5",
        "serialNumber": "urn:uuid:00000000-0000-0000-0000-000000000000",
        "version": 1,
        "metadata": {
            "component": {
                "type": "application",
                "name": "TideTerm",
                "version": _root_version(),
            },
            "tools": [{"vendor": "TideTerm", "name": "generate-sbom.py"}],
        },
        "components": sorted(components.values(), key=lambda c: (c["name"], c["version"])),
    }

    OUT.write_text(json.dumps(sbom, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {OUT} with {len(sbom['components'])} components")
    return 0


def _root_version() -> str:
    pkg = ROOT / "native" / "Cargo.toml"
    with pkg.open("rb") as f:
        data = tomllib.load(f)
    return str(data["package"]["version"])


if __name__ == "__main__":
    sys.exit(main())
