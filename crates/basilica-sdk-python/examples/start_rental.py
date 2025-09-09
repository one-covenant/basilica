#!/usr/bin/env python3
"""
Start Rental Example for Basilica SDK

Demonstrates how to start GPU rentals with various configurations.
"""

from basilica import BasilicaClient


def main():
    # Initialize client (uses BASILICA_API_URL and BASILICA_API_TOKEN from environment)
    client = BasilicaClient()
    
    # Start a rental with all available configuration options
    rental = client.start_rental(
        # Container configuration
        container_image="pytorch/pytorch:2.0.0-cuda11.7-cudnn8-runtime",  # Default: basilica default image
        
        # GPU selection - choose one method:

        # Method 1: Specify GPU type
        gpu_type="h100",  # Options: h100, a100, etc.
        
        # Method 2: Target a specific executor by ID (find the id manually or by using list_rentals method)
        # executor_id="executor-uuid-here",  # Use specific executor
        
        # SSH configuration
        # ssh_public_key="ssh-rsa AAAAB3... user@host",  # Explicit SSH key
        # Auto-detects from ~/.ssh/basilica_*.pub if not specified
        
        # Set custom Environment variables that will be set in the container
        environment={
            "CUDA_VISIBLE_DEVICES": "0,1",
        },
        
        # Port mappings for services
        ports=[
            {"container_port": 8888, "host_port": 8888, "protocol": "tcp"},  # Jupyter
            {"container_port": 6006, "host_port": 6006, "protocol": "tcp"},  # TensorBoard
            {"container_port": 5000, "host_port": 5000, "protocol": "tcp"},  # API server
        ],
        
        command=["/bin/bash"],
    )
    
    # Access rental details
    print(f"Rental ID: {rental.rental_id}")
    print(f"Container: {rental.container_name}")
    print(f"Status: {rental.status}")
    if rental.ssh_credentials:
        print(f"SSH: {rental.ssh_credentials}")
    
    # Get updated rental status
    status = client.get_rental(rental.rental_id)
    print(f"Executor ID: {status.executor.id}")
    print(f"Created at: {status.created_at}")
    
    # List GPU details
    for gpu in status.executor.gpu_specs:
        print(f"GPU: {gpu.name} - {gpu.memory_gb} GB")
    
    # Stop rental when done
    # client.stop_rental(rental.rental_id)


if __name__ == "__main__":
    main()