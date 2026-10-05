#!/usr/bin/env python3
"""Regenerate checked-in DSDL C, or verify it without modifying the checkout."""
import argparse
import hashlib
import importlib.metadata
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CRATE = ROOT / "dronecan-dsdl-sys"
NAMESPACES = ("uavcan", "dronecan", "ardupilot", "com", "cuav", "mppt")
VERSIONS = {"dronecan": "1.0.27", "empy": "3.3.4", "pexpect": "4.9.0", "ptyprocess": "0.7.0"}


def generate(destination):
    for package, expected in VERSIONS.items():
        actual = importlib.metadata.version(package)
        if actual != expected:
            raise RuntimeError(f"{package}=={expected} required, found {actual}; install requirements.txt")
    output = subprocess.run(
        [sys.executable, str(CRATE / "dronecan_dsdlc/dronecan_dsdlc.py"),
         "-j", "2", "-O", str(destination),
         *(str(CRATE / "DSDL" / namespace) for namespace in NAMESPACES)],
        capture_output=True, text=True,
    )
    if output.returncode:
        raise RuntimeError("DSDL generation failed:\n" + output.stdout + output.stderr)
    # Only build inputs are vendored, not generator test executables/helpers.
    for path in list(destination.iterdir()):
        if path.name not in ("include", "src"):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    files = sorted(path for path in destination.rglob("*") if path.is_file())
    manifest = {
        "format": 1,
        "namespaces": list(NAMESPACES),
        "python_packages": VERSIONS,
        "revisions": {
            name: subprocess.check_output(["git", "-C", str(CRATE / name), "rev-parse", "HEAD"], text=True).strip()
            for name in ("DSDL", "dronecan_dsdlc")
        },
        "sha256": {str(path.relative_to(destination)): hashlib.sha256(path.read_bytes()).hexdigest() for path in files},
    }
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail if generated files have drifted")
    args = parser.parse_args()
    vendor = CRATE / "vendored"
    with tempfile.TemporaryDirectory(prefix="dronecan-dsdl-") as temporary:
        generated = Path(temporary) / "generated"
        generate(generated)
        if args.check:
            expected = {str(p.relative_to(generated)): p.read_bytes() for p in generated.rglob("*") if p.is_file()}
            actual = {str(p.relative_to(vendor)): p.read_bytes() for p in vendor.rglob("*") if p.is_file()}
            if actual != expected:
                changed = sorted(key for key in actual.keys() | expected.keys() if actual.get(key) != expected.get(key))
                raise RuntimeError("vendored DSDL has drifted: " + ", ".join(changed))
            print("Vendored DSDL matches the pinned generator and definitions.")
        else:
            if vendor.exists():
                shutil.rmtree(vendor)
            shutil.copytree(generated, vendor)
            print(f"Generated {len(list(vendor.rglob('*.c')))} codecs into {vendor}")


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError, importlib.metadata.PackageNotFoundError) as error:
        print(error, file=sys.stderr)
        sys.exit(1)
