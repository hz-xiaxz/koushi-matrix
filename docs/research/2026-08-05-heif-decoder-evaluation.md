# HEIF decoder evaluation

Issue #426 requires the decoder to be distributable on Linux, macOS, and
Windows without depending on an optional end-user codec. The candidates were
checked from their crates.io metadata and package documentation on 2026-08-05.

| Candidate | License / build | Coverage | Decision |
| --- | --- | --- | --- |
| `heif-oxide 0.1.0` | MIT OR Apache-2.0; pure Rust; no C or runtime library | HEVC `hvc1` and grids, 8/10-bit, `irot`/`imir`/`clap`, Display P3 to sRGB; rejects unsupported codecs and HDR transfer functions | Selected |
| `libheif-rs 2.7.0` | Apache-2.0 wrapper around `libheif-sys`; native libheif and codec backends must be built and packaged | Broad libheif coverage, but packaging and codec patent obligations vary by target | Rejected for this gate |
| `heif-rs 26.7.0` | Apache-2.0 Rust wrapper with statically linked libheif, x265, and libde265 | Broad native-backed coverage | Rejected for native packaging and codec-patent surface |
| `heic 0.1.6` | AGPL-3.0-only or commercial license | Pure Rust HEIC decoder | Rejected for the workspace's MIT/Apache distribution |

`heif-oxide` was built in an isolated release probe on Linux with the checked-in
64×64 HEIC fixture. The dependency graph contains only Rust crates (`heif-oxide`,
`rust_h265`, and `thiserror`), so the same source build has no target-specific
runtime codec requirement. The current checkout does not have macOS or Windows
Rust targets installed; release packaging CI remains the cross-target smoke
gate.

The selected decoder is intentionally bounded by Koushi before decode: the
container `ftyp`/`ispe` probe rejects malformed or oversized dimensions, and the
existing decoded allocation limit remains in force. It decodes only a primary
still image. Unsupported sequence/codec/HDR content returns a typed failure and
leaves the exact original bytes available.

The fixture is generated test data from the `heif-oxide` package's MIT/Apache
test suite and is used only for decoder regression coverage. No user media or
metadata is stored in the repository.
