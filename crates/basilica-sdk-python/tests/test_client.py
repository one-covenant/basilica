"""
Tests for the Basilica Python SDK
"""

import pytest
from unittest.mock import Mock, patch
from basilica import BasilicaClient, create_rental_request


def test_client_creation():
    """Test creating a client"""
    client = BasilicaClient("https://api.basilica.ai")
    assert client is not None
    
    client_with_token = BasilicaClient(
        "https://api.basilica.ai",
        token="test-token",
        timeout_secs=60
    )
    assert client_with_token is not None


def test_create_rental_request():
    """Test creating a rental request helper"""
    request = create_rental_request(
        container_image="nginx:latest",
        ssh_public_key="ssh-rsa AAAAA..."
    )
    
    assert request["container_image"] == "nginx:latest"
    assert request["ssh_public_key"] == "ssh-rsa AAAAA..."


@patch("basilica._basilica.BasilicaClient")
def test_health_check(mock_client_class):
    """Test health check method"""
    mock_instance = Mock()
    mock_instance.health_check.return_value = {
        "status": "healthy",
        "version": "1.0.0",
        "healthy_validators": 5,
        "total_validators": 5
    }
    mock_client_class.return_value = mock_instance
    
    client = BasilicaClient("https://api.basilica.ai")
    health = client.health_check()
    
    assert health["status"] == "healthy"
    assert health["version"] == "1.0.0"


@patch("basilica._basilica.BasilicaClient")
def test_list_executors(mock_client_class):
    """Test listing executors"""
    mock_instance = Mock()
    mock_instance.list_executors.return_value = {
        "available_executors": [],
        "total_count": 0
    }
    mock_client_class.return_value = mock_instance
    
    client = BasilicaClient("https://api.basilica.ai", token="test-token")
    executors = client.list_executors(available=True, gpu_type="h100")
    
    assert "available_executors" in executors
    assert executors["total_count"] == 0


@patch("basilica._basilica.BasilicaClient")
def test_start_rental(mock_client_class):
    """Test starting a rental"""
    mock_instance = Mock()
    mock_instance.start_rental.return_value = {
        "rental_id": "rental-123",
        "status": "Pending"
    }
    mock_client_class.return_value = mock_instance
    
    client = BasilicaClient("https://api.basilica.ai", token="test-token")
    rental = client.start_rental(
        container_image="nginx:latest",
        ssh_public_key="ssh-rsa AAAAA...",
        resources={"gpu_count": 1}
    )
    
    assert rental["rental_id"] == "rental-123"
    assert rental["status"] == "Pending"


@patch("basilica._basilica.BasilicaClient")
def test_get_rental(mock_client_class):
    """Test getting rental status"""
    mock_instance = Mock()
    mock_instance.get_rental.return_value = {
        "rental_id": "rental-123",
        "status": {"state": "Active"}
    }
    mock_client_class.return_value = mock_instance
    
    client = BasilicaClient("https://api.basilica.ai", token="test-token")
    status = client.get_rental("rental-123")
    
    assert status["rental_id"] == "rental-123"
    assert status["status"]["state"] == "Active"


@patch("basilica._basilica.BasilicaClient")
def test_stop_rental(mock_client_class):
    """Test stopping a rental"""
    mock_instance = Mock()
    mock_instance.stop_rental.return_value = None
    mock_client_class.return_value = mock_instance
    
    client = BasilicaClient("https://api.basilica.ai", token="test-token")
    
    # Should not raise an exception
    client.stop_rental("rental-123")
    mock_instance.stop_rental.assert_called_once()


@patch("basilica._basilica.BasilicaClient")
def test_list_rentals(mock_client_class):
    """Test listing rentals"""
    mock_instance = Mock()
    mock_instance.list_rentals.return_value = {
        "rentals": [],
        "total_count": 0
    }
    mock_client_class.return_value = mock_instance
    
    client = BasilicaClient("https://api.basilica.ai", token="test-token")
    rentals = client.list_rentals(status="Active")
    
    assert "rentals" in rentals
    assert rentals["total_count"] == 0