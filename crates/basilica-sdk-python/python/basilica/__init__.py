"""
Basilica SDK for Python

A Python SDK for interacting with the Basilica GPU rental network.
"""

import os
from typing import Optional, Dict, Any, List

from basilica._basilica import (
    BasilicaClient as _BasilicaClient,
    create_rental_request,
    # Response types
    HealthCheckResponse,
    RentalResponse,
    RentalStatusResponse,
    RentalStatus,
    SshAccess,
    ExecutorDetails,
    GpuSpec,
    CpuSpec,
    AvailableExecutor,
    AvailabilityInfo,
)

from basilica.constants import (
    DEFAULT_API_URL,
    DEFAULT_TIMEOUT_SECS,
    DEFAULT_CONTAINER_IMAGE,
    DEFAULT_GPU_TYPE,
    DEFAULT_GPU_COUNT,
    DEFAULT_GPU_MIN_MEMORY_GB,
    DEFAULT_CPU_CORES,
    DEFAULT_MEMORY_MB,
    DEFAULT_STORAGE_MB,
    DEFAULT_WAIT_TIMEOUT_SECS,
    DEFAULT_POLL_INTERVAL_SECS,
    RENTAL_STATE_ACTIVE,
    TERMINAL_RENTAL_STATES,
)

__version__ = "0.1.0"
__all__ = [
    "BasilicaClient",
    "create_rental_request",
    # Response types
    "HealthCheckResponse",
    "RentalResponse",
    "RentalStatusResponse",
    "RentalStatus",
    "SshAccess",
    "ExecutorDetails",
    "GpuSpec",
    "CpuSpec",
    "AvailableExecutor",
    "AvailabilityInfo",
]


class BasilicaClient:
    """
    Client for interacting with the Basilica API.
    
    Example:
        >>> client = BasilicaClient("https://api.basilica.ai", token="your-token")
        >>> health = client.health_check()
        >>> print(health["status"])
    """
    
    def __init__(
        self,
        base_url: Optional[str] = None,
        token: Optional[str] = None,
        timeout_secs: int = DEFAULT_TIMEOUT_SECS
    ):
        """
        Initialize a new Basilica client.
        
        Args:
            base_url: The base URL of the Basilica API (default: from BASILICA_API_URL env or DEFAULT_API_URL)
            token: Optional authentication token (default: from BASILICA_API_TOKEN env)
            timeout_secs: Request timeout in seconds (default: DEFAULT_TIMEOUT_SECS)
        """
        # Auto-detect base_url if not provided
        if base_url is None:
            base_url = os.environ.get("BASILICA_API_URL", DEFAULT_API_URL)
        
        # Auto-detect token if not provided
        if token is None:
            token = os.environ.get("BASILICA_API_TOKEN")
        
        self._client = _BasilicaClient(base_url, token, timeout_secs)
    
    def health_check(self) -> HealthCheckResponse:
        """
        Check the health of the API.
        
        Returns:
            HealthCheckResponse: Typed response with status, version, and validator info
        """
        return self._client.health_check()
    
    def list_executors(
        self,
        available: Optional[bool] = None,
        gpu_type: Optional[str] = None,
        min_gpu_count: Optional[int] = None
    ) -> List[AvailableExecutor]:
        """
        List available executors.
        
        Args:
            available: Filter by availability
            gpu_type: Filter by GPU type
            min_gpu_count: Filter by minimum GPU count
            
        Returns:
            List[AvailableExecutor]: List of typed executor objects with details
        """
        query = {}
        if available is not None:
            query["available"] = available
        if gpu_type is not None:
            query["gpu_type"] = gpu_type
        if min_gpu_count is not None:
            query["min_gpu_count"] = min_gpu_count
            
        return self._client.list_executors(query if query else None)
    
    def start_rental(
        self,
        container_image: Optional[str] = None,
        executor_id: Optional[str] = None,
        gpu_type: Optional[str] = None,
        gpu_count: int = DEFAULT_GPU_COUNT,
        ssh_public_key: Optional[str] = None,
        environment: Optional[Dict[str, str]] = None,
        ports: Optional[List[Dict[str, Any]]] = None,
        resources: Optional[Dict[str, Any]] = None,
        command: Optional[List[str]] = None,
        volumes: Optional[List[Dict[str, str]]] = None,
        no_ssh: bool = False
    ) -> RentalResponse:
        """
        Start a new rental.
        
        Args:
            container_image: Docker image to run (default: DEFAULT_CONTAINER_IMAGE)
            executor_id: Optional specific executor to use
            gpu_type: GPU type to request (default: DEFAULT_GPU_TYPE)
            gpu_count: Number of GPUs to request (default: DEFAULT_GPU_COUNT)
            ssh_public_key: SSH public key for access (auto-detected from ~/.ssh/id_*.pub if not provided)
            environment: Environment variables
            ports: Port mappings
            resources: Resource requirements (uses defaults if not provided)
            command: Command to run
            volumes: Volume mounts
            no_ssh: Disable SSH access
            
        Returns:
            RentalResponse: Typed response with rental details
        """
        # Set defaults from constants
        if container_image is None:
            container_image = DEFAULT_CONTAINER_IMAGE
        
        if gpu_type is None:
            gpu_type = DEFAULT_GPU_TYPE
        
        if ssh_public_key is None and not no_ssh:
            # Auto-detect SSH key
            import glob
            ssh_key_paths = glob.glob(os.path.expanduser("~/.ssh/id_*.pub"))
            if ssh_key_paths:
                with open(ssh_key_paths[0]) as f:
                    ssh_public_key = f.read().strip()
        
        if resources is None:
            resources = {
                "gpu_count": gpu_count,
                "gpu_types": [gpu_type] if gpu_type else [],  # Array of GPU types
                "cpu_cores": DEFAULT_CPU_CORES,
                "memory_mb": DEFAULT_MEMORY_MB,
                "storage_mb": DEFAULT_STORAGE_MB
            }
        else:
            # Merge with defaults to ensure all required fields are present
            # Handle gpu_type -> gpu_types conversion
            if "gpu_type" in resources and "gpu_types" not in resources:
                # Convert singular gpu_type to plural gpu_types array
                gpu_type_value = resources.pop("gpu_type")
                resources["gpu_types"] = [gpu_type_value] if gpu_type_value else []
            elif "gpu_types" not in resources:
                resources["gpu_types"] = [gpu_type] if gpu_type else []
            
            # Ensure all required fields have defaults
            if "gpu_count" not in resources:
                resources["gpu_count"] = gpu_count
            if "cpu_cores" not in resources:
                resources["cpu_cores"] = DEFAULT_CPU_CORES
            if "memory_mb" not in resources:
                resources["memory_mb"] = DEFAULT_MEMORY_MB
            if "storage_mb" not in resources:
                resources["storage_mb"] = DEFAULT_STORAGE_MB
        
        # Build executor_selection based on whether executor_id is provided
        if executor_id:
            executor_selection = {
                "type": "executor_id",
                "executor_id": executor_id
            }
        else:
            # Use GPU requirements from resources for auto-selection
            gpu_requirements = {
                "min_memory_gb": DEFAULT_GPU_MIN_MEMORY_GB,
                "gpu_count": resources.get("gpu_count", gpu_count)
            }
            # Get GPU type from gpu_types array if available
            gpu_types = resources.get("gpu_types", [])
            if gpu_types and len(gpu_types) > 0:
                gpu_requirements["gpu_type"] = gpu_types[0]
            elif gpu_type:
                gpu_requirements["gpu_type"] = gpu_type
            
            executor_selection = {
                "type": "gpu_requirements",
                "gpu_requirements": gpu_requirements
            }
        
        request = {
            "executor_selection": executor_selection,
            "container_image": container_image,
            "ssh_public_key": ssh_public_key if ssh_public_key else "",
            "no_ssh": no_ssh
        }
        if environment:
            request["environment"] = environment
        if ports:
            request["ports"] = ports
        if resources:
            request["resources"] = resources
        if command:
            request["command"] = command
        if volumes:
            request["volumes"] = volumes
            
        return self._client.start_rental(request)
    
    def get_rental(self, rental_id: str) -> RentalStatusResponse:
        """
        Get rental status.
        
        Args:
            rental_id: The rental ID
            
        Returns:
            RentalStatusResponse: Typed response with status and details
        """
        return self._client.get_rental(rental_id)
    
    def stop_rental(self, rental_id: str) -> None:
        """
        Stop a rental.
        
        Args:
            rental_id: The rental ID
        """
        self._client.stop_rental(rental_id)
    
    def list_rentals(
        self,
        status: Optional[str] = None,
        gpu_type: Optional[str] = None,
        min_gpu_count: Optional[int] = None
    ) -> Dict[str, Any]:
        """
        List rentals.
        
        Args:
            status: Filter by status (e.g., "Active", "Pending")
            gpu_type: Filter by GPU type
            min_gpu_count: Filter by minimum GPU count
            
        Returns:
            List of rentals
        """
        query = {}
        if status is not None:
            query["status"] = status
        if gpu_type is not None:
            query["gpu_type"] = gpu_type
        if min_gpu_count is not None:
            query["min_gpu_count"] = min_gpu_count
            
        return self._client.list_rentals(query if query else None)
    
    def wait_for_rental(
        self,
        rental_id: str,
        target_state: str = RENTAL_STATE_ACTIVE,
        timeout: int = DEFAULT_WAIT_TIMEOUT_SECS,
        poll_interval: int = DEFAULT_POLL_INTERVAL_SECS
    ) -> RentalStatusResponse:
        """
        Wait for a rental to reach a specific state.
        
        Args:
            rental_id: The rental ID to wait for
            target_state: The state to wait for (default: RENTAL_STATE_ACTIVE)
            timeout: Maximum time to wait in seconds (default: DEFAULT_WAIT_TIMEOUT_SECS)
            poll_interval: How often to check status in seconds (default: DEFAULT_POLL_INTERVAL_SECS)
            
        Returns:
            RentalStatusResponse: Final rental status
            
        Raises:
            TimeoutError: If timeout is reached before target state
        """
        import time
        start_time = time.time()
        
        while time.time() - start_time < timeout:
            status = self.get_rental(rental_id)
            current_state = status.status.state
            
            if current_state == target_state:
                return status
            
            # Check for terminal states that won't transition to target
            if current_state in TERMINAL_RENTAL_STATES:
                raise RuntimeError(f"Rental reached terminal state: {current_state}")
            
            time.sleep(poll_interval)
        
        raise TimeoutError(f"Timeout waiting for rental to reach {target_state} state")