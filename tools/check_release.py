#!/usr/bin/env python3
"""Build actual Cargo archives and independent consumers; never upload anything."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[1]
CRATES = ("canard-sys", "dronecan-dsdl-sys", "dronecan-core")
SOURCE = """#![no_std]
use dronecan_core::{Arena, Error, MEM_BLOCK_SIZE, Node, NodeId, NodeStatus};

// Pull a generated codec into the independent consumer as well.
pub const BATTERY_INFO_ID: u16 = dronecan_dsdl_sys::UAVCAN_EQUIPMENT_POWER_BATTERYINFO_ID;

pub fn queue_status() -> Result<usize, Error> {
    const SIZE: usize = MEM_BLOCK_SIZE * 256;
    let mut arena = Arena::<SIZE>::new();
    let mut node: Node<'_, SIZE> = Node::new(&mut arena, NodeId::new(69)?, &[])?;
    node.broadcast_status(NodeStatus::operational(1), 1)?;
    let mut count = 0;
    while node.try_transmit(|_| {
        count += 1;
        Ok::<_, core::convert::Infallible>(())
    }).unwrap() {}
    Ok(count)
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_transport_and_codec_link() {
        assert_eq!(super::queue_status().unwrap(), 1);
        assert_eq!(super::BATTERY_INFO_ID, 1092);
    }
}
"""


def run(*command, cwd=ROOT, env=None):
    print("+ " + " ".join(map(str, command)), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def write_consumer(directory, dependency_source, standalone=True):
    directory.mkdir()
    (directory / "src").mkdir()
    (directory / "src/lib.rs").write_text(SOURCE)
    workspace = "[workspace]" if standalone else ""
    (directory / "Cargo.toml").write_text(f'''[package]
name = "dronecan-release-consumer"
version = "0.0.0"
edition = "2024"
publish = false

{workspace}

[features]
default = ["vendored"]
vendored = ["dronecan-core/vendored"]
canfd = ["dronecan-core/canfd"]
deadline = ["dronecan-core/deadline"]
multi-iface = ["dronecan-core/multi-iface"]
tao-option = ["dronecan-core/tao-option"]

[dependencies]
dronecan-core = {{ {dependency_source('dronecan-core')}, default-features = false }}
dronecan-dsdl-sys = {{ {dependency_source('dronecan-dsdl-sys')}, default-features = false, features = ["uavcan"] }}
''')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--offline", action="store_true", help="use cached registry dependencies and Git sources")
    parser.add_argument("--allow-dirty", action="store_true", help="allow packaging uncommitted files")
    parser.add_argument("--skip-package", action="store_true", help="check existing archives in target/package")
    parser.add_argument("--source-generation", action="store_true", help="also check packaged Python generation")
    parser.add_argument("--git-rev", help="also build a Cargo Git dependency at this local repository revision")
    parser.add_argument("--target", help="also cross-check the no_std consumer, e.g. thumbv6m-none-eabi")
    args = parser.parse_args()
    versions = {name: tomllib.loads((ROOT / name / "Cargo.toml").read_text())["package"]["version"] for name in CRATES}
    for name in CRATES:
        for notice in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md"):
            assert (ROOT / name / notice).read_bytes() == (ROOT / notice).read_bytes(), f"{name}: stale {notice} copy"
    assert (ROOT / "requirements.txt").read_bytes() == (ROOT / "dronecan-dsdl-sys/requirements.txt").read_bytes(), "stale packaged Python requirements"
    network = ["--offline"] if args.offline else []
    with tempfile.TemporaryDirectory(prefix="dronecan-release-") as scratch:
        scratch = Path(scratch)
        archives = ROOT / "target/package"
        if not args.skip_package:
            dirty = ["--allow-dirty"] if args.allow_dirty else []
            # Cargo caches private registry sources by registry path/version.
            # Use a fresh path so edits to an unreleased version cannot silently
            # validate stale dependency archives from an earlier invocation.
            run("cargo", "package", "--workspace", "--locked", "--target-dir", scratch / "package-build", *dirty, *network)
            archives = scratch / "package-build/package"
        # Independent of the repository and its C submodules. These are the
        # normalized manifests/sources actually shipped to crates.io.
        for name in CRATES:
            archive = archives / f"{name}-{versions[name]}.crate"
            with tarfile.open(archive) as package:
                package.extractall(scratch, filter="data")
            unpacked = scratch / f"{name}-{versions[name]}"
            for notice in ("LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-NOTICES.md"):
                assert (unpacked / notice).is_file(), f"{name}: missing {notice}"
            manifest = tomllib.loads((unpacked / "Cargo.toml").read_text())
            for dependency in manifest.get("dependencies", {}).values():
                assert "path" not in dependency, f"{name}: packaged dependency still uses a path"

        patches = "\n[patch.crates-io]\n" + "".join(
            f"{name} = {{ path = {json.dumps(str(scratch / f'{name}-{versions[name]}'))} }}\n" for name in CRATES
        )
        members = [f"{name}-{versions[name]}" for name in CRATES] + ["archives"]
        (scratch / "Cargo.toml").write_text(f'[workspace]\nresolver = "2"\nmembers = {json.dumps(members)}\n{patches}')
        consumer = scratch / "archives"
        write_consumer(consumer, lambda name: f'version = "={versions[name]}"', standalone=False)
        build_env = os.environ.copy()
        build_env["CARGO_TARGET_DIR"] = str(ROOT / "target/release-check")
        for flags in ([], ["--all-features"]):
            run("cargo", "test", "-p", "dronecan-core", *flags, *network, cwd=consumer, env=build_env)
            run("cargo", "test", "-p", "dronecan-release-consumer", *flags, *network, cwd=consumer, env=build_env)
        if args.target:
            run("cargo", "check", "--all-features", "--target", args.target, *network, cwd=consumer, env=build_env)
        if args.source_generation:
            run("cargo", "test", "-p", "dronecan-release-consumer", "--no-default-features", *network, cwd=consumer, env=build_env)

        if args.git_rev:
            revision = subprocess.check_output(["git", "rev-parse", args.git_rev], cwd=ROOT, text=True).strip()
            consumer = scratch / "git"
            source = f"git = {json.dumps(ROOT.as_uri())}, rev = {json.dumps(revision)}"
            write_consumer(consumer, lambda _: source)
            # In offline mode this requires a previously fetched Git revision,
            # just like Cargo registry dependencies must already be cached.
            host = next(line.removeprefix("host: ") for line in subprocess.check_output(["rustc", "-vV"], text=True).splitlines() if line.startswith("host: "))
            targets = ["--target", host]
            if args.target:
                targets += ["--target", args.target]
            run("cargo", "--config", "net.git-fetch-with-cli=true", "fetch", *targets, *network, cwd=consumer, env=build_env)
            run("cargo", "test", "--all-features", *network, cwd=consumer, env=build_env)
            if args.target:
                run("cargo", "check", "--all-features", "--target", args.target, *network, cwd=consumer, env=build_env)
        if not args.skip_package:
            destination = ROOT / "target/package"
            destination.mkdir(parents=True, exist_ok=True)
            for name in CRATES:
                filename = f"{name}-{versions[name]}.crate"
                shutil.copyfile(archives / filename, destination / filename)
    print("Release archives and independent consumers passed.", flush=True)


if __name__ == "__main__":
    main()
