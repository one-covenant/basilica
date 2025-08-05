# Basilica Unified CLI Specification

## Overview

```bash
# Simple workflow - get started with GPU rental
basilica init                           # Initial setup
basilica up                             # Interactive GPU selection and rental
basilica exec <uid/huid> "python train.py"  # Run your code
basilica status <uid/huid>              # Check status
basilica ls                             # List all GPUs
basilica down                           # Interactive termination of rented GPUs

# Other subcommands (retain existing arguments)
basilica validator      # Run/manage validator
basilica miner          # Run/manage miner
basilica executor       # Run/manage executor
```

## Other thoughts
- add `--json` flag where appropriate
- add `--verbose` where we can show more information

### Command Categories

|Category| Commands                                                        |Description|
|---|---|---|
|**GPU Rental**| `ls`, `pricing`, `up`, `exec`, `ps`, `ssh`, `cp`, `logs`, `status`, `down` |Direct API interactions for renting and managing GPU instances|
|**Other binaries**| `validator`, `miner`, `executor`                                |Subcommands for running network infrastructure components (they keep the same argument)|
|**Other**| `init`, `config`, `wallet`                                      |Other setup and utility subcommands|

## Command Specifications

#### `basilica init`

Initialize and configure the Basilica CLI for API access. Registers user's hotkey with the Basilica API and creates a hotwallet for holding TAO credits to pay for GPU rentals.

```bash
# Interactive setup
basilica init
```

**Process:**
- Registers user hotkey with `/register` API endpoint
- Creates a hotwallet for holding TAO credits
- Returns hotwallet address that needs to be funded with TAO
- Caches hotwallet address locally for future reference
- Stores basic configuration for subsequent API calls

#### `basilica config`

Manage CLI configuration settings.

```bash
# Show current configuration
basilica config show

# Set configuration values (Lium-style)
basilica config set api-url https://api.basilica.network
basilica config set default-image basilica/ubuntu:latest

# Get specific configuration value
basilica config get api-url

# Reset configuration
basilica config reset
```

#### `basilica ls`

List available GPU resources.

```bash
# List all available GPUs
basilica ls

# List available GPUs with minimum requirements
basilica ls --gpu-min=2 --gpu-max=8 --gpu-type=h100 --price-max=5
```

#### `basilica pricing`

Display current pricing for GPU resources in a user-friendly format.

```bash
# Show all GPU pricing
basilica pricing

# Filter by GPU type
basilica pricing --gpu-type h100

# Filter by minimum memory
basilica pricing --min-memory 40

# Sort by price (cheapest first)
basilica pricing --sort-price asc
```

#### `basilica up`

Provision and start GPU instances.

```bash
# Interactive GPU selection (checks available GPUs and prompts user to select)
basilica up

# Rent GPU by UID/HUID
basilica up <uid/huid>

# Rent with requirements
basilica up --gpu-type h100 --gpu-min 2 --name my-training-job

# Rent with custom image and environment
basilica up <uid/huid> --image pytorch/pytorch:latest --env CUDA_VISIBLE_DEVICES=0,1
```

#### `basilica exec`

Execute commands on instances with flexible targeting.

```bash
# Execute by HUID
basilica exec <uid/huid> "python train.py"

# Execute on multiple targets
basilica exec <list of uid/huid> "python train.py"
basilica exec all "python train.py"
```

#### `basilica ps`

List active rentals and their status.

```bash
# List all active rentals
basilica ps

# Filter by status
basilica ps --status running
```

#### `basilica cp`

Copy files to/from instances.

> NOTE: preferring cp over rsync since its easier to type and remember :)

```bash
# Copy to instance
basilica cp ~/dataset/ <uid/huid>:/workspace/data/

# Copy from instance  
basilica cp <uid/huid>:/results/ ~/results/
```

#### `basilica ssh`

SSH into instances.

> NOTE: we can reuse existing ssh arguments to not add another subcommand

```bash
# SSH by UID/HUID
basilica ssh <uid/huid>

# SSH with port forwarding
basilica ssh <uid/huid> -L 8080:localhost:8080
```

#### `basilica logs`

View instance logs.

```bash
# View logs
basilica logs <target>

# Follow logs in real-time
basilica logs <target> --follow
```

#### `basilica status`

Check instance status.

```bash
# Check status
basilica status <target>
```

#### `basilica down`

Terminate instances.

```bash
# Interactive termination (shows rented GPUs and prompts user to select which to terminate)
basilica down

# Remove instance
basilica down <uid/huid>

# Remove multiple instances
basilica down <list of uid/huid>
```

#### `basilica wallet`

View wallet information and address.

```bash
# Show default wallet address and balance
basilica wallet

# Show specific wallet info
basilica wallet --name validator_wallet
```

## Configuration File Format

TOML configuration stored at `~/.basilica/config.toml`:

```toml
[api]
api_key = "your-api-key-here"
base_url = "https://api.basilica.network"

network = "mainnet"  # subnet should be assumed 39 for mainnet, 387 for testnet

[ssh]
# Default ssh public key used for giving ssh access
key_path = "~/.ssh/basilica_rsa.pub" 

[image]
name = "basilica/default:latest"

[wallet]
default_wallet = "main"
wallet_path = "~/.basilica/wallets/"
```

## Cache File Format

Place to store values like hotwallet used for funding users credit

Path: `~/.basilica/cache.json`:

```json
{
  "registration": {
    "hotwallet": "5HpG9w8EBLe5XCrbczpwq5TSXvedjrBGCwqxK1iQ7qUsSWFc",
    "created_at": "2024-01-15T10:30:00Z",
    "last_updated": "2024-01-15T10:30:00Z"
  }
}
```

## Installation and Distribution

- Should be able to install with a simple command

```shell
bash <(curl -fsSL basilica.ai/install.sh)
```

