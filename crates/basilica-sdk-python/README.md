# Basilica Python SDK

Python bindings for the Basilica GPU rental network SDK.

## Installation

### From Source

```bash
# Install maturin (build tool)
pip install maturin

# Build and install
cd crates/basilica-sdk-python
maturin develop
```

### From PyPI (when published)

```bash
pip install basilica
```

## Usage

### Basic Example

```python
from basilica import BasilicaClient

# Create client
client = BasilicaClient(
    base_url="https://api.basilica.ai",
    token="your-auth-token"
)

# Check API health
health = client.health_check()
print(f"API Status: {health['status']}")

# List available executors
executors = client.list_executors(available=True)
print(f"Found {len(executors['available_executors'])} executors")

# Start a rental
rental = client.start_rental(
    container_image="nvidia/cuda:12.2.0-base-ubuntu22.04",
    ssh_public_key="ssh-rsa AAAAA...",
    resources={
        "gpu_count": 1,
        "gpu_type": "h100"
    }
)
print(f"Started rental: {rental['rental_id']}")

# Get rental status
status = client.get_rental(rental['rental_id'])
print(f"Rental state: {status['status']['state']}")

# Stop rental
client.stop_rental(rental['rental_id'])
```

## API Reference

### BasilicaClient

#### `__init__(base_url: str, token: Optional[str] = None, timeout_secs: int = 30)`

Initialize a new Basilica client.

**Parameters:**
- `base_url`: The base URL of the Basilica API
- `token`: Optional authentication token
- `timeout_secs`: Request timeout in seconds (default: 30)

#### `health_check() -> Dict[str, Any]`

Check the health of the API.

**Returns:** Health check response containing status, version, and validator info

#### `list_executors(available: Optional[bool] = None, gpu_type: Optional[str] = None, min_gpu_count: Optional[int] = None) -> Dict[str, Any]`

List available executors.

**Parameters:**
- `available`: Filter by availability
- `gpu_type`: Filter by GPU type (e.g., "h100", "a100")
- `min_gpu_count`: Filter by minimum GPU count

**Returns:** List of available executors

#### `start_rental(...) -> Dict[str, Any]`

Start a new GPU rental.

**Parameters:**
- `container_image`: Docker image to run
- `executor_id`: Optional specific executor to use
- `ssh_public_key`: SSH public key for access
- `environment`: Environment variables as dict
- `ports`: Port mappings list
- `resources`: Resource requirements dict
- `command`: Command to run as list
- `volumes`: Volume mounts list
- `no_ssh`: Disable SSH access (default: False)

**Returns:** Rental response with rental ID and details

#### `get_rental(rental_id: str) -> Dict[str, Any]`

Get rental status and details.

**Parameters:**
- `rental_id`: The rental ID

**Returns:** Rental status and details

#### `stop_rental(rental_id: str) -> None`

Stop a rental.

**Parameters:**
- `rental_id`: The rental ID

#### `list_rentals(status: Optional[str] = None, gpu_type: Optional[str] = None, min_gpu_count: Optional[int] = None) -> Dict[str, Any]`

List your rentals.

**Parameters:**
- `status`: Filter by status (e.g., "Active", "Pending")
- `gpu_type`: Filter by GPU type
- `min_gpu_count`: Filter by minimum GPU count

**Returns:** List of rentals

## Examples

See the `examples/` directory for complete examples:
- `list_executors.py` - List available GPU executors
- `start_rental.py` - Start and manage a GPU rental

## Development

### Building

```bash
# Install development dependencies
pip install maturin pytest pytest-asyncio mypy

# Build the extension
maturin develop

# Run tests
pytest tests/
```

### Type Checking

```bash
mypy python/basilica
```

## License

MIT OR Apache-2.0