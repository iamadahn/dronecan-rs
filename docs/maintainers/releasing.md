# Publishing dronecan-rs

Repository: https://github.com/iamadahn/dronecan-rs

Run the commands below from the repository root.

The versioned library workspace contains `canard-sys`, `dronecan-dsdl-sys` and
`dronecan-core`. The firmware in `bin/stm32g0b1` is a separate workspace and is
not published as a crate. Use a current stable Cargo for release commands
(`cargo package/publish --workspace`); library MSRV is Rust 1.85.

## Validate a release

Install host libclang and GNU C/binutils; cross validation additionally needs
the Rust target plus arm-none-eabi GCC, binutils and newlib. Initialize all
submodules before working from a manual clone.

```sh
git submodule update --init --recursive
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test -p dronecan-core --all-features --locked
cargo check --workspace --all-features --locked
cargo +1.85.0 test --workspace --locked
RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps --locked
python3 tools/check_release.py --target thumbv6m-none-eabi
cargo publish --workspace --dry-run --locked
```

`check_release.py` requires Python 3.11+ for the validation script only; it
does not install or invoke the DSDL generator unless requested. It packages
all three crates, extracts the actual archives outside the repository,
checks license/dependency files, runs the core regression suite and links a
separate consumer with default/all safe features. It can also check a Cargo
Git dependency from a committed revision:

```sh
python3 tools/check_release.py --git-rev HEAD --target thumbv6m-none-eabi
```

Pass `--offline` to use cached registry dependencies, `--allow-dirty` during
preparation, or `--skip-package` to inspect archives already built. An offline
Git check also requires the revision and submodules in Cargo's cache. Nothing in this script uploads
packages or pushes Git refs.

Verify packaged source-generation mode and snapshot reproducibility too:

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r requirements.txt
.venv/bin/python tools/generate_dsdl.py --check
DRONECAN_DSDLC_PYTHON="$PWD/.venv/bin/python" python3 tools/check_release.py --source-generation
```

All CI jobs must pass. Keep the three versions and their internal dependency
requirements in sync. Update CHANGELOG.md and upstream notice revisions when
changing bundled upstream sources. Both own license texts/notices are duplicated
into crate directories so each archive is self-contained; the validation checks
them. The DSDL package includes upstream GPL generator/templates and records
that in its package license expression.

## Publish Git release

Create the GitHub repository above if it does not exist, then push the reviewed
commit and an annotated tag:

```sh
git push -u origin main
git tag -a v0.1.0 -m 'dronecan-rs 0.1.0'
git push origin v0.1.0
```

Downstream users can then depend on the Git tag from README.md immediately;
no crates.io account or Cargo registry token is needed for this route.

## Publish crates.io release

Log in locally with a crates.io token; never put tokens in project files or Git.
Then publish the tested workspace:

```sh
cargo login
cargo publish --workspace --locked
```

Cargo publishes the dependency crates before their consumers. With an older
Cargo lacking workspace publishing, publish individually in this order and
wait for each package to become available in the registry index:

```sh
cargo publish -p canard-sys --locked
cargo publish -p dronecan-dsdl-sys --locked
cargo publish -p dronecan-core --locked
```

After upload, check https://docs.rs/dronecan-core and the other two crate pages.
docs.rs builds on the Linux host using vendored C. Raw assertion/semaphore
features require consumer hooks and are intentionally excluded from docs.rs.
