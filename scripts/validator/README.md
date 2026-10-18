# Basilica Validator

Bittensor neuron for verification and scoring of miners and executors.

## Quick Start

```bash
# Build locally
./build.sh

# Deploy with binary mode (default)
./deploy.sh -s root@validator.example.com:9001

# Deploy with systemd
./deploy.sh -s root@validator.example.com:9001 -d systemd

# Run locally for development
docker-compose -f compose.dev.yml up
```

## Veritas binary provenance

Validator images include two x86-64 Veritas executables that were already
published as Git blobs in `one-covenant/basilica` commit
`e40e11f96712806644a586a9ee8895548efdebd9`. They are downloaded from the
commit-pinned `raw.githubusercontent.com/one-covenant/basilica` paths and
verified by `scripts/validator/stage-veritas.sh` before use:

| Binary | SHA-256 |
| --- | --- |
| `bin/executor-binary/executor-binary` | `056922331186a08fd9a644ce8cbd0b0455accd73bc23dedd740109415ce6968a` |
| `bin/validator-binary/validator-binary` | `391d6c5c673276e428ee5d9531a8c1aa7e306944a2eeb2102a5d42e548402eab` |

The build script stages these under root `bin/`, which is ignored by Git. The
Dockerfile always downloads and verifies the pinned blobs for its default
build, including direct Docker or Compose builds. Existing local files in
`bin/` cannot change a pinned image. Neither build path uses empty
placeholders. Builds fail if the download is unavailable or its hash changes.

To use a separately supplied Veritas build, pass
`--veritas-binaries DIR` to `build.sh`; `DIR` must contain both
`<name>/<name>` files. This explicit override validates that each input is a
nonempty x86-64 ELF file before staging it. Its provenance is the operator's
responsibility and the pinned SHA-256 checks do not apply to the override.
`deploy.sh` uses the pinned files staged by `build.sh` for an ordinary SSH
deployment and uses the supplied files when `-b DIR` is set. Both are copied to
the nested paths expected by the validator configuration.

## Deployment Modes

The validator supports two deployment modes:

### Binary Mode (Default)
Deploys the validator binary and runs it with nohup:
```bash
./deploy.sh -s root@validator.example.com:9001
./deploy.sh -s root@validator.example.com:9001 -d binary
```

### Systemd Mode
Deploys the validator binary and manages it as a systemd service:
```bash
./deploy.sh -s root@validator.example.com:9001 -d systemd
```

## Files

- `Dockerfile` - Container image definition
- `build.sh` - Build script for Docker image and binary
- `deploy.sh` - Multi-mode deployment script
- `systemd/basilica-validator.service` - Systemd service definition
- `compose.dev.yml` - Development docker-compose with local build
- `compose.local.yml` - Localnet-oriented docker-compose configuration

## Configuration

The validator uses configuration files from the `config/` directory. Default is `config/validator.correct.toml`.

Specify custom config:
```bash
./deploy.sh -s root@validator.example.com:9001 -c config/validator.prod.toml
```

Key settings:
- `[bittensor]` - Wallet name, hotkey, network (finney/test/local), netuid 387
- `[server]` - Port 8081 for API server
- `[verification]` - GPU attestation and benchmark settings
- `[ssh_validation]` - SSH connection parameters for executor validation

## Ports

- `8081` - HTTP API server
- `9091` - Bittensor Axon port
- `9090` - Prometheus metrics endpoint

## Deployment Options

```bash
Usage: ./deploy.sh [OPTIONS]

OPTIONS:
    -s, --server USER@HOST:PORT      Server connection
    -d, --deploy-mode MODE           Deployment mode: binary, systemd (default: binary)
    -c, --config FILE                Config file path (default: config/validator.correct.toml)
    -w, --sync-wallets               Sync local wallets to remote server
    -f, --follow-logs                Stream logs after deployment
    --health-check                   Perform health checks on service endpoints
    -t, --timeout SECONDS           SSH timeout (default: 60)
    -b, --veritas-binaries DIR       Directory containing veritas binaries to deploy
    -h, --help                       Show this help
```

## Examples

```bash
# Deploy with wallet sync and health checks
./deploy.sh -s root@validator.example.com:9001 -w --health-check

# Deploy with custom config and follow logs
./deploy.sh -s root@validator.example.com:9001 -c config/validator.staging.toml -f

# Deploy with veritas binaries
./deploy.sh -s root@validator.example.com:9001 -b ../veritas/binaries

# Deploy with systemd and health monitoring
./deploy.sh -s root@validator.example.com:9001 -d systemd --health-check -f
```

## Service Management

### Binary Mode
```bash
# Check status
ssh root@validator.example.com -p 9001 "pgrep -f validator"

# View logs
ssh root@validator.example.com -p 9001 "tail -f /opt/basilica/validator.log"

# Stop service
ssh root@validator.example.com -p 9001 "pkill -f validator"
```

### Systemd Mode
```bash
# Check status
ssh root@validator.example.com -p 9001 "systemctl status basilica-validator"

# View logs
ssh root@validator.example.com -p 9001 "journalctl -u basilica-validator -f"

# Restart service
ssh root@validator.example.com -p 9001 "systemctl restart basilica-validator"
```

## SSH Keys

Generate validator SSH keys for executor access:
```bash
ssh-keygen -t ed25519 -f ~/.ssh/validator_ssh -N ""
```

The public key must be added to executor machines for verification access.
