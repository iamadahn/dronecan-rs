# Third-party notices

Rust sources and build scripts authored in dronecan-rs are available under
MIT OR Apache-2.0; see LICENSE-MIT and LICENSE-APACHE. This grant does not
replace the licenses of the following upstream components.

| Component | Source | Revision | License / notice |
| --- | --- | --- | --- |
| libcanard | https://github.com/dronecan/libcanard | 601ed35467e0ac38819df17cd7c918de19f62d58 | MIT, canard-sys/libcanard/LICENSE |
| DSDL definitions | https://github.com/dronecan/DSDL | b4653c7abc3c47cb31b16efa24ea755232774756 | MIT, dronecan-dsdl-sys/DSDL/LICENSE |
| dronecan_dsdlc and its templates | https://github.com/dronecan/dronecan_dsdlc | 431170fa4bfe2212b516b8f33bdc796267907f1c | GPL-3.0-or-later, dronecan-dsdl-sys/dronecan_dsdlc/COPYING |

The generated codecs in dronecan-dsdl-sys/vendored are produced from these
DSDL definitions and generator templates. Their provenance and file hashes
are recorded in vendored/manifest.json. No alternative license is granted
for upstream templates or their generated code by this repository.

Within individual Cargo archives, the upstream notice paths are relative to
the crate directory (libcanard/LICENSE, DSDL/LICENSE, dronecan_dsdlc/COPYING).
The dronecan-dsdl-sys archive also contains the generator and templates for
optional source-generation builds. Its package license expression includes
GPL-3.0-or-later; the Rust sources in it retain the dual license above.
