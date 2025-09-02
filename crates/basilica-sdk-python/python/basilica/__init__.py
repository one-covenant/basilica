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
        base_url: str,
        token: Optional[str] = None,
        timeout_secs: int = 30
    ):
        """
        Initialize a new Basilica client.
        
        Args:
            base_url: The base URL of the Basilica API
            token: Optional authentication token
            timeout_secs: Request timeout in seconds (default: 30)
        """
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
        container_image: str,
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
            container_image: Docker image to run
            executor_id: Optional specific executor to use
            ssh_public_key: SSH public key for access
            environment: Environment variables
            ports: Port mappings
            resources: Resource requirements
            command: Command to run
            volumes: Volume mounts
            no_ssh: Disable SSH access
            
        Returns:
            Rental response with rental ID and details
        """
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