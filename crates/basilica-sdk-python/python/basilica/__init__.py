"""
Basilica SDK for Python

A Python SDK for interacting with the Basilica GPU rental network.
"""

from typing import Optional, Dict, Any, List

from basilica._basilica import BasilicaClient as _BasilicaClient
from basilica._basilica import create_rental_request

__version__ = "0.1.0"
__all__ = ["BasilicaClient", "create_rental_request"]


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
        timeout_secs: int = 30
    ):
        """
        Initialize a new Basilica client.
        
        Args:
            base_url: The base URL of the Basilica API (default: from BASILICA_API_URL env or https://api.basilica.ai)
            token: Optional authentication token (default: from BASILICA_API_TOKEN env)
            timeout_secs: Request timeout in seconds (default: 30)
        """
        import os
        
        # Auto-detect base_url if not provided
        if base_url is None:
            base_url = os.environ.get("BASILICA_API_URL", "https://api.basilica.ai")
        
        # Auto-detect token if not provided
        if token is None:
            token = os.environ.get("BASILICA_API_TOKEN")
        
        self._client = _BasilicaClient(base_url, token, timeout_secs)
    
    def health_check(self) -> Dict[str, Any]:
        """
        Check the health of the API.
        
        Returns:
            Health check response containing status, version, and validator info
        """
        return self._client.health_check()
    
    def list_executors(
        self,
        available: Optional[bool] = None,
        gpu_type: Optional[str] = None,
        min_gpu_count: Optional[int] = None
    ) -> Dict[str, Any]:
        """
        List available executors.
        
        Args:
            available: Filter by availability
            gpu_type: Filter by GPU type
            min_gpu_count: Filter by minimum GPU count
            
        Returns:
            List of available executors
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
        ssh_public_key: Optional[str] = None,
        environment: Optional[Dict[str, str]] = None,
        ports: Optional[List[Dict[str, Any]]] = None,
        resources: Optional[Dict[str, Any]] = None,
        command: Optional[List[str]] = None,
        volumes: Optional[List[Dict[str, str]]] = None,
        no_ssh: bool = False
    ) -> Dict[str, Any]:
        """
        Start a new rental.
        
        Args:
            container_image: Docker image to run (default: nvidia/cuda:12.2.0-base-ubuntu22.04)
            executor_id: Optional specific executor to use
            ssh_public_key: SSH public key for access (auto-detected from ~/.ssh/id_*.pub if not provided)
            environment: Environment variables
            ports: Port mappings
            resources: Resource requirements (default: {"gpu_count": 1, "gpu_type": "h100"})
            command: Command to run
            volumes: Volume mounts
            no_ssh: Disable SSH access
            
        Returns:
            Rental response with rental ID and details
        """
        # Set defaults
        if container_image is None:
            container_image = "nvidia/cuda:12.2.0-base-ubuntu22.04"
        
        if ssh_public_key is None and not no_ssh:
            # Auto-detect SSH key
            import os
            import glob
            ssh_key_paths = glob.glob(os.path.expanduser("~/.ssh/id_*.pub"))
            if ssh_key_paths:
                with open(ssh_key_paths[0]) as f:
                    ssh_public_key = f.read().strip()
        
        if resources is None:
            resources = {
                "gpu_count": 1,
                "gpu_type": "h100"
            }
        
        request = {
            "container_image": container_image,
            "no_ssh": no_ssh
        }
        
        if executor_id:
            request["executor_id"] = executor_id
        if ssh_public_key:
            request["ssh_public_key"] = ssh_public_key
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
    
    def get_rental(self, rental_id: str) -> Dict[str, Any]:
        """
        Get rental status.
        
        Args:
            rental_id: The rental ID
            
        Returns:
            Rental status and details
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
        target_state: str = "Active",
        timeout: int = 300,
        poll_interval: int = 5
    ) -> Dict[str, Any]:
        """
        Wait for a rental to reach a specific state.
        
        Args:
            rental_id: The rental ID to wait for
            target_state: The state to wait for (default: "Active")
            timeout: Maximum time to wait in seconds (default: 300)
            poll_interval: How often to check status in seconds (default: 5)
            
        Returns:
            Final rental status
            
        Raises:
            TimeoutError: If timeout is reached before target state
        """
        import time
        start_time = time.time()
        
        while time.time() - start_time < timeout:
            status = self.get_rental(rental_id)
            current_state = status.get("status", {}).get("state", "Unknown")
            
            if current_state == target_state:
                return status
            
            # Check for terminal states that won't transition to target
            if current_state in ["Failed", "Terminated", "Cancelled"]:
                raise RuntimeError(f"Rental reached terminal state: {current_state}")
            
            time.sleep(poll_interval)
        
        raise TimeoutError(f"Timeout waiting for rental to reach {target_state} state")