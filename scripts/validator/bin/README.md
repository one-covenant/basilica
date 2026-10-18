The validator image needs the two Veritas executables at
`/opt/basilica/bin/{executor-binary,validator-binary}/{executor-binary,validator-binary}`.
They are staged in the public workspace's root `bin/` directory, not here.

`scripts/validator/build.sh` downloads the exact blobs from the archived public
source commit, checks SHA-256, and stages them for SSH deployment. The
Dockerfile performs its own pinned download for normal builds, even when local
staged binaries exist. The build fails if either binary is missing or fails
verification; empty placeholders are never accepted. See
[validator build provenance](../README.md#veritas-binary-provenance).

