# Basilica Python SDK

Python bindings for the Basilica GPU rental network SDK.

## Installation

### From Source

```bash
# Using uv (recommended)
uv pip install -e crates/basilica-sdk-python

# Or using pip with maturin
pip install maturin
cd crates/basilica-sdk-python
maturin develop
```

### From PyPI (when published)

```bash
pip install basilica
```

## Quick Start

```python
from basilica import BasilicaClient
from basilica.ssh_utils import format_ssh_command

# Create client - automatically uses environment variables
# BASILICA_API_URL (defaults to https://api.basilica.ai)
# BASILICA_API_TOKEN (for authentication)
client = BasilicaClient()

# Start a GPU rental with minimal configuration
rental = client.start_rental()  # All defaults applied

# SSH credentials are returned directly in the rental response
if rental.ssh_credentials:
    ssh_command = format_ssh_command(rental.ssh_credentials)
    print(f"Connect with: {ssh_command}")

# Stop the rental when done
client.stop_rental(rental.rental_id)
```

## Features

### 🚀 Auto-Configuration
- Automatically detects `BASILICA_API_URL` and `BASILICA_API_TOKEN` from environment
- SSH keys auto-detected from `~/.ssh/basilica_*.pub` (Basilica-specific keys)
- Sensible defaults for all parameters

### 🔐 Enhanced SSH Handling
- Built-in SSH utilities for credential parsing and command generation
- Automatic SSH key detection and validation
- Formatted SSH connection instructions with error handling

### 🎯 Simplified API
```python
# Minimal - all defaults
client = BasilicaClient()
rental = client.start_rental()

# Or customize what you need
rental = client.start_rental(
    container_image="pytorch/pytorch:latest",
    resources={"gpu_count": 2, "gpu_type": "a100"}
)
```


## API Reference

### BasilicaClient

#### `__init__(base_url: Optional[str] = None, token: Optional[str] = None)`

Initialize a new Basilica client.

**Parameters:**
- `base_url`: The base URL of the Basilica API (default: from `BASILICA_API_URL` env or `https://api.basilica.ai`)
- `token`: Authentication token (default: from `BASILICA_API_TOKEN` env)

#### `start_rental(...) -> Dict[str, Any]`

Start a new GPU rental with smart defaults.

**Parameters (all optional):**
- `container_image`: Docker image to run (default: `nvidia/cuda:12.2.0-base-ubuntu22.04`)
- `ssh_public_key`: SSH public key (default: auto-detected from `~/.ssh/basilica_*.pub`)
- `gpu_type`: GPU type to request (default: "h100")
- `executor_id`: Specific executor to use
- `environment`: Environment variables as dict
- `ports`: Port mappings list
- `command`: Command to run as list
- `no_ssh`: Disable SSH access (default: False)

**Returns:** Rental response with rental ID and details


#### `get_rental(rental_id: str) -> Dict[str, Any]`

Get rental status and details.

#### `stop_rental(rental_id: str) -> None`

Stop a rental.

#### `list_executors(available: Optional[bool] = None, gpu_type: Optional[str] = None, min_gpu_count: Optional[int] = None) -> Dict[str, Any]`

List available executors.

#### `list_rentals(status: Optional[str] = None, gpu_type: Optional[str] = None, min_gpu_count: Optional[int] = None) -> Dict[str, Any]`

List your rentals.

#### `health_check() -> Dict[str, Any]`

Check the health of the API.

### SSH Utilities

The SDK includes helpful utilities for working with SSH credentials:

#### `parse_ssh_credentials(credentials: str) -> Tuple[str, str, int]`

Parse SSH credentials string in format 'user@host:port'.

**Parameters:**
- `credentials`: SSH credentials string (e.g., 'root@84.200.81.243:32776')

**Returns:** Tuple of (user, host, port)

**Raises:** `ValueError` if credentials format is invalid

#### `format_ssh_command(credentials: str, ssh_key_path: Optional[str] = None) -> str`

Generate a complete SSH command from credentials string.

**Parameters:**
- `credentials`: SSH credentials string (e.g., 'root@84.200.81.243:32776')
- `ssh_key_path`: Optional path to SSH private key (default: `~/.ssh/basilica_ed25519`)

**Returns:** Complete SSH command string

#### `print_ssh_instructions(credentials: Optional[str], rental_id: str, ssh_key_path: Optional[str] = None) -> None`

Print formatted SSH connection instructions to console.

**Parameters:**
- `credentials`: SSH credentials string or None
- `rental_id`: Rental ID for context
- `ssh_key_path`: Optional path to SSH private key

## Examples

### Quickstart (Minimal Code)
```python
from basilica import BasilicaClient
from basilica.ssh_utils import format_ssh_command

client = BasilicaClient()
rental = client.start_rental()

# SSH credentials are available immediately in the rental response
if rental.ssh_credentials:
    ssh_command = format_ssh_command(rental.ssh_credentials)
    print(f"Connect with: {ssh_command}")
```

### Custom Configuration with SSH Utilities
```python
from basilica import BasilicaClient
from basilica.ssh_utils import print_ssh_instructions

client = BasilicaClient()

# Start with custom settings
rental = client.start_rental(
    container_image="pytorch/pytorch:2.0.0-cuda11.7-cudnn8-runtime",
    gpu_type="a100",
    environment={
        "CUDA_VISIBLE_DEVICES": "0,1",
        "PYTORCH_CUDA_ALLOC_CONF": "max_split_size_mb:512"
    },
    ports=[
        {"container_port": 8888, "host_port": 8888, "protocol": "tcp"},  # Jupyter
        {"container_port": 6006, "host_port": 6006, "protocol": "tcp"},  # TensorBoard
    ]
)

# Print formatted SSH instructions
print_ssh_instructions(rental.ssh_credentials, rental.rental_id)

# Get updated status with executor details
status = client.get_rental(rental.rental_id)
print(f"Running on executor: {status.executor.id}")
for gpu in status.executor.gpu_specs:
    print(f"GPU: {gpu.name} - {gpu.memory_gb} GB")
```

### SSH Utilities Usage
```python
from basilica import BasilicaClient
from basilica.ssh_utils import parse_ssh_credentials, format_ssh_command, print_ssh_instructions

client = BasilicaClient()
rental = client.start_rental(gpu_type="h100")

# Different ways to work with SSH credentials
if rental.ssh_credentials:
    # Parse credentials into components
    user, host, port = parse_ssh_credentials(rental.ssh_credentials)
    print(f"User: {user}, Host: {host}, Port: {port}")
    
    # Generate SSH command with default key
    ssh_cmd = format_ssh_command(rental.ssh_credentials)
    print(f"SSH command: {ssh_cmd}")
    
    # Generate SSH command with custom key
    ssh_cmd_custom = format_ssh_command(rental.ssh_credentials, "~/.ssh/my_custom_key")
    print(f"Custom key SSH: {ssh_cmd_custom}")
    
    # Print formatted instructions
    print_ssh_instructions(rental.ssh_credentials, rental.rental_id)
```

### List Available GPUs
```python
from basilica import BasilicaClient

client = BasilicaClient()

# Find available H100 GPUs
executors = client.list_executors(
    available=True,
    gpu_type="h100"
)

for executor in executors:
    print(f"Executor {executor.id}: {len(executor.gpu_specs)}x {executor.gpu_specs[0].name}")
```

See the `examples/` directory for more complete examples:
- `quickstart.py` - Minimal example with SSH utilities
- `start_rental.py` - Full rental example with SSH instructions
- `list_executors.py` - List available GPU executors
- `health_check.py` - API health check example

## Development

### Building from Source

```bash
# Clone the repository
git clone https://github.com/basilica/basilica.git
cd basilica/crates/basilica-sdk-python

# Create virtual environment
uv venv
source .venv/bin/activate

# Build and install
uv pip install -e .

# Run tests
pytest tests/
```

### Environment Variables

- `BASILICA_API_URL`: API endpoint (default: `https://api.basilica.ai`)
- `BASILICA_API_TOKEN`: Your authentication token

### SSH Key Configuration

The SDK automatically detects SSH keys from `~/.ssh/basilica_*.pub` (e.g., `basilica_ed25519.pub`, `basilica_rsa.pub`). 

To set up SSH keys for Basilica:
```bash
# Generate a Basilica-specific SSH key
ssh-keygen -t ed25519 -f ~/.ssh/basilica_ed25519 -C "your-email@example.com"

# The public key will be auto-detected by the SDK
ls ~/.ssh/basilica_ed25519.pub
```

## License

MIT OR Apache-2.0