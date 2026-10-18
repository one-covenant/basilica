# Vendored smoldot 0.19.4

This is the crates.io `smoldot` 0.19.4 source archive (SHA-256
`e16e5723359f0048bf64bfdfba64e5732a56847d42c4fd3fe56f18280c813413`),
published from upstream Git commit
[`9446ff27f73be86fbf5ba3b3da57dabc1bff6bc2`](https://github.com/paritytech/smoldot-archive/tree/9446ff27f73be86fbf5ba3b3da57dabc1bff6bc2/lib).
The only runtime dependency change is in the normalized `Cargo.toml`:
`ruzstd` advances from `0.8.0` to `0.9.1`, the first version outside
[RUSTSEC-2026-0344](https://rustsec.org/advisories/RUSTSEC-2026-0344.html).
The package inclusion list also carries `LICENSE` and this provenance file into
Python SDK source distributions. All Rust source and features match the archive;
`Cargo.toml.orig` remains the upstream original. The archive's nested lockfile
is omitted because both parent workspaces own their locked graphs.

The upstream crate archive omits a license file. `LICENSE` is copied unchanged
from that upstream Git commit (SHA-256
`aab56b4a581fc1c50b7c782eacf2fc8be05a47cd98e4bf4d836dd9b6dd9c86f4`)
and declares `GPL-3.0-or-later WITH Classpath-exception-2.0`. The selected
`smoldot-light` 0.17.2 has a manifest-only path dependency on this crate so
Maturin carries it transitively into source distributions. Remove this patch
once a compatible Subxt/smoldot-light/smoldot release resolves a fixed `ruzstd`
without it; refresh and validate both root and public lockfiles together.
