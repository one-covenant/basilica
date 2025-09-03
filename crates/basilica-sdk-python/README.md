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

# Create client - automatically uses environment variables
# BASILICA_API_URL (defaults to https://api.basilica.ai)
# BASILICA_API_TOKEN (for authentication)
client = BasilicaClient()

# Start a GPU rental with minimal configuration
rental = client.start_rental()  # All defaults applied

# Wait for rental to be ready
status = client.wait_for_rental(rental["rental_id"])

# Get SSH access details
ssh = status["ssh_access"]
print(f"Connect: ssh -p {ssh['port']} {ssh['user']}@{ssh['host']}")

# Stop the rental when done
client.stop_rental(rental["rental_id"])
```

## Features

### 🚀 Auto-Configuration
- Automatically detects `BASILICA_API_URL` and `BASILICA_API_TOKEN` from environment
- SSH keys auto-detected from `~/.ssh/id_*.pub`
- Sensible defaults for all parameters

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

### ⏳ Blocking Wait
```python
# No more manual polling loops!
status = client.wait_for_rental(
    rental_id,
    timeout=600,  # Max 10 minutes
    poll_interval=10  # Check every 10 seconds
)
```

## API Reference

### BasilicaClient

#### `__init__(base_url: Optional[str] = None, token: Optional[str] = None, timeout_secs: int = 30)`

Initialize a new Basilica client.

**Parameters:**
- `base_url`: The base URL of the Basilica API (default: from `BASILICA_API_URL` env or `https://api.basilica.ai`)
- `token`: Authentication token (default: from `BASILICA_API_TOKEN` env)
- `timeout_secs`: Request timeout in seconds (default: 30)

#### `start_rental(...) -> Dict[str, Any]`

Start a new GPU rental with smart defaults.

**Parameters (all optional):**
- `container_image`: Docker image to run (default: `nvidia/cuda:12.2.0-base-ubuntu22.04`)
- `ssh_public_key`: SSH public key (default: auto-detected from `~/.ssh/`)
- `resources`: Resource requirements (default: `{"gpu_count": 1, "gpu_type": "h100"}`)
- `environment`: Environment variables as dict
- `executor_id`: Specific executor to use
- `ports`: Port mappings list
- `command`: Command to run as list
- `volumes`: Volume mounts list
- `no_ssh`: Disable SSH access (default: False)

**Returns:** Rental response with rental ID and details

#### `wait_for_rental(rental_id: str, target_state: str = "Active", timeout: int = 300, poll_interval: int = 5) -> Dict[str, Any]`

Wait for a rental to reach a specific state.

**Parameters:**
- `rental_id`: The rental ID to wait for
- `target_state`: The state to wait for (default: "Active")
- `timeout`: Maximum time to wait in seconds (default: 300)
- `poll_interval`: How often to check status in seconds (default: 5)

**Returns:** Final rental status

**Raises:**
- `TimeoutError`: If timeout is reached before target state
- `RuntimeError`: If rental reaches a terminal error state

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

## Examples

### Quickstart (Minimal Code)
```python
from basilica import BasilicaClient

client = BasilicaClient()
rental = client.start_rental()
status = client.wait_for_rental(rental["rental_id"])
print(f"SSH: ssh -p {status['ssh_access']['port']} root@{status['ssh_access']['host']}")
```

### Custom Configuration
```python
from basilica import BasilicaClient

client = BasilicaClient()

# Start with custom settings
rental = client.start_rental(
    container_image="pytorch/pytorch:2.0.0-cuda11.7-cudnn8-runtime",
    resources={
        "gpu_count": 2,
        "gpu_type": "a100"
    },
    environment={
        "CUDA_VISIBLE_DEVICES": "0,1",
        "PYTORCH_CUDA_ALLOC_CONF": "max_split_size_mb:512"
    }
)

# Wait with custom timeout
status = client.wait_for_rental(
    rental["rental_id"],
    timeout=600,  # 10 minutes
    poll_interval=10  # Check every 10 seconds
)
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

for executor in executors["available_executors"]:
    print(f"Executor {executor['id']}: {executor['gpu_count']}x {executor['gpu_type']}")
```

See the `examples/` directory for more complete examples:
- `quickstart.py` - Minimal example
- `start_rental.py` - Simple rental with defaults
- `start_rental_advanced.py` - Custom configuration example
- `list_executors.py` - List available GPU executors

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
- `SSH_PUBLIC_KEY`: Override SSH key auto-detection

## License

MIT OR Apache-2.0