"""
Basilica SDK for Python

A Python SDK for interacting with the Basilica GPU rental network.
"""

import os
from typing import Optional, Dict, Any, List

from basilica._basilica import (
    BasilicaClient as _BasilicaClient,
    # Helper functions
    executor_by_id,
    executor_by_gpu,
    # Response types
    HealthCheckResponse,
    RentalResponse,
    RentalStatusWithSshResponse,
    RentalStatus,
    SshAccess,
    ExecutorDetails,
    GpuSpec,
    CpuSpec,
    AvailableExecutor,
    AvailabilityInfo,
    # Request types
    StartRentalApiRequest,
    ExecutorSelection,
    GpuRequirements,
    PortMappingRequest,
    ResourceRequirementsRequest,
    VolumeMountRequest,
    ListAvailableExecutorsQuery,
    ListRentalsQuery,
    # Constants from Rust
    DEFAULT_API_URL,
    DEFAULT_TIMEOUT_SECS,
    DEFAULT_CONTAINER_IMAGE,
    DEFAULT_GPU_TYPE,
    DEFAULT_GPU_COUNT,
    DEFAULT_GPU_MIN_MEMORY_GB,
    DEFAULT_CPU_CORES,
    DEFAULT_MEMORY_MB,
    DEFAULT_STORAGE_MB,
)

# Default command is a list in Python
DEFAULT_COMMAND = ["/bin/bash"]

__version__ = "0.1.0"
__all__ = [
    "BasilicaClient",
    # Helper functions
    "executor_by_id",
    "executor_by_gpu",
    # Response types
    "HealthCheckResponse",
    "RentalResponse",
    "RentalStatusWithSshResponse",
    "RentalStatus",
    "SshAccess",
    "ExecutorDetails",
    "GpuSpec",
    "CpuSpec",
    "AvailableExecutor",
    "AvailabilityInfo",
    # Request types
    "StartRentalApiRequest",
    "ExecutorSelection",
    "GpuRequirements",
    "PortMappingRequest",
    "ResourceRequirementsRequest",
    "VolumeMountRequest",
    "ListAvailableExecutorsQuery",
    "ListRentalsQuery",
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
        min_gpu_count: Optional[int] = None,
        min_gpu_memory: Optional[int] = None
    ) -> List[AvailableExecutor]:
        """
        List available executors.
        
        Args:
            available: Filter by availability
            gpu_type: Filter by GPU type
            min_gpu_count: Filter by minimum GPU count
            min_gpu_memory: Filter by minimum GPU memory in GB
            
        Returns:
            List[AvailableExecutor]: List of typed executor objects with details
        """
        if any([available is not None, gpu_type is not None, min_gpu_count is not None, min_gpu_memory is not None]):
            query = ListAvailableExecutorsQuery(
                available=available,
                gpu_type=gpu_type,
                min_gpu_count=min_gpu_count,
                min_gpu_memory=min_gpu_memory
            )
            return self._client.list_executors(query)
        else:
            return self._client.list_executors(None)
    
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
            command: Command to run (default: ["/bin/bash"])
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
            ssh_key_paths = glob.glob(os.path.expanduser("~/.ssh/basilica_*.pub"))
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
            executor_selection = executor_by_id(executor_id)
        else:
            # Use GPU requirements from resources for auto-selection
            gpu_count_val = resources.get("gpu_count", gpu_count)
            min_memory_gb_val = DEFAULT_GPU_MIN_MEMORY_GB
            
            # Get GPU type from gpu_types array if available
            gpu_types = resources.get("gpu_types", [])
            gpu_type_val = None
            if gpu_types and len(gpu_types) > 0:
                gpu_type_val = gpu_types[0]
            elif gpu_type:
                gpu_type_val = gpu_type
            
            gpu_requirements = GpuRequirements(
                gpu_count=gpu_count_val,
                min_memory_gb=min_memory_gb_val,
                gpu_type=gpu_type_val
            )
            executor_selection = executor_by_gpu(gpu_requirements)
        
        # Convert ports to PortMappingRequest objects
        port_mappings = []
        if ports:
            for port in ports:
                port_mappings.append(PortMappingRequest(
                    container_port=port.get("container_port", 0),
                    host_port=port.get("host_port", 0),
                    protocol=port.get("protocol", "tcp")
                ))
        
        # Create ResourceRequirementsRequest
        resource_req = ResourceRequirementsRequest(
            cpu_cores=resources.get("cpu_cores", DEFAULT_CPU_CORES),
            memory_mb=resources.get("memory_mb", DEFAULT_MEMORY_MB),
            storage_mb=resources.get("storage_mb", DEFAULT_STORAGE_MB),
            gpu_count=resources.get("gpu_count", gpu_count),
            gpu_types=resources.get("gpu_types", [])
        )
        
        # Convert volumes to VolumeMountRequest objects
        volume_mounts = []
        if volumes:
            for vol in volumes:
                volume_mounts.append(VolumeMountRequest(
                    host_path=vol.get("host_path", ""),
                    container_path=vol.get("container_path", ""),
                    read_only=vol.get("read_only", False)
                ))
        
        request = StartRentalApiRequest(
            executor_selection=executor_selection,
            container_image=container_image,
            ssh_public_key=ssh_public_key if ssh_public_key else "",
            environment=environment or {},
            ports=port_mappings,
            resources=resource_req,
            command=command if command is not None else DEFAULT_COMMAND,
            volumes=volume_mounts,
            no_ssh=no_ssh
        )
            
        return self._client.start_rental(request)
    
    def get_rental(self, rental_id: str) -> RentalStatusWithSshResponse:
        """
        Get rental status.
        
        Args:
            rental_id: The rental ID
            
        Returns:
            RentalStatusWithSshResponse: Typed response with status and SSH details
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
            status: Filter by status (e.g., "active", "provisioning")
            gpu_type: Filter by GPU type
            min_gpu_count: Filter by minimum GPU count
            
        Returns:
            List of rentals
        """
        if any([status is not None, gpu_type is not None, min_gpu_count is not None]):
            query = ListRentalsQuery(
                status=status,
                gpu_type=gpu_type,
                min_gpu_count=min_gpu_count
            )
            return self._client.list_rentals(query)
        else:
            return self._client.list_rentals(None)