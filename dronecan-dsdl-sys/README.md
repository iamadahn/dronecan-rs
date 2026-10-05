# dronecan-dsdl-sys

Raw `no_std` Rust FFI for generated DroneCAN DSDL C codecs, sharing Canard types
and native configuration with `canard-sys`. Safe standard-message wrappers live
in `dronecan-core`.

## Build modes

Default features are `dronecan` (which includes `uavcan`) and `vendored`.
Vendored builds compile the selected namespaces from checked-in generated C and
need only a target C compiler and host libclang. Build inputs and upstream
notices are included in the Cargo package.

Disable defaults and select namespaces to generate C from the DSDL definitions at
build time, for example `--no-default-features --features uavcan`. This needs the
Python environment from this crate's `requirements.txt`. The Cargo archive
contains the DSDL definitions, generator and templates too; a manual Git clone
requires initialized submodules. Interpreter discovery uses the workspace's
`.venv/bin/python`; outside that workspace, set `DRONECAN_DSDLC_PYTHON` to a
Python interpreter with those requirements installed.

Regenerate/check the complete vendored snapshot from the repository root:

```sh
.venv/bin/python tools/generate_dsdl.py
.venv/bin/python tools/generate_dsdl.py --check
```

`vendored/manifest.json` records source revisions, pinned generator dependencies
and SHA-256 hashes. CI regenerates and compares every file.

## Namespaces and ABI

`uavcan`, `dronecan`, `ardupilot`, `com`, `cuav`, `mppt`; feature dependencies pull
in their base namespaces. Only selected namespaces are compiled and bound.
The same `DEP_CANARD_DEFINES` are used by C and bindgen, including TAO/CAN FD.

`*_SIGNATURE` constants are `u64`, IDs `u16`, and small constants `u8`.
Dynamic-array structs are named `<Parent>_<field>`. Generated `_encode` functions
require the message's full `MAX_SIZE` output capacity, even for short messages;
`_decode` returns true on invalid data. Raw APIs remain unsafe.

## Notices

Rust sources authored here: `MIT OR Apache-2.0`. DSDL definitions retain their
upstream MIT license (`DSDL/LICENSE`). The generator and its templates are
GPLv3-or-later (`dronecan_dsdlc/COPYING`), and the archive includes them for
source-generation builds. Package metadata therefore uses
`(MIT OR Apache-2.0) AND GPL-3.0-or-later`. See THIRD-PARTY-NOTICES.md and the
vendored manifest for upstream provenance; this repository does not relicense
generated upstream code.
