# Vendored smoldot-light 0.17.2

This is the crates.io `smoldot-light` 0.17.2 source archive (SHA-256
`f1bba9e591716567d704a8252feeb2f1261a286e1e2cbdd4e49e9197c34a14e2`),
published from upstream Git commit
[`59ebd05e463ff90f04631ee930fd7bedfa7b2908`](https://github.com/smol-dot/smoldot/tree/59ebd05e463ff90f04631ee930fd7bedfa7b2908/light-base).
The runtime dependency change is in `Cargo.toml`: its `lru` requirement advances
from `0.12.0` to `0.18.2`, the first version outside both
[GHSA-rhfx-m35p-ff5j](https://github.com/advisories/GHSA-rhfx-m35p-ff5j)
and [RUSTSEC-2026-0253](https://rustsec.org/advisories/RUSTSEC-2026-0253.html).
The same manifest also includes `LICENSE` and this provenance file in Python
SDK source distributions; all Rust source and features remain upstream-identical.
`Cargo.toml.orig` remains the upstream original. The archive's nested `Cargo.lock`
is omitted because both parent workspaces own their own locked graphs; it remains
recoverable from the checksum-identified crates.io archive.

The upstream archive does not include a license file. `LICENSE` is copied
unchanged from that upstream Git commit and declares
`GPL-3.0-or-later WITH Classpath-exception-2.0`. Preserve that file and this
provenance when updating the vendor. Remove this patch once the selected Subxt
and `smoldot-light` releases resolve to a fixed `lru` without it. Until then,
refresh and validate both root and `public/` lockfiles together.
