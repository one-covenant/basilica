#!/usr/bin/env python3
"""
Example: Start a GPU rental with custom configuration
"""

from basilica import BasilicaClient

def main():
    # Create client with automatic configuration
    client = BasilicaClient()
    
    try:
        # Start a rental with custom configuration
        print("Starting customized GPU rental...")
        
        # You can override any of the defaults
        rental = client.start_rental(
            # Override the default container image
            container_image="pytorch/pytorch:2.0.0-cuda11.7-cudnn8-runtime",
            
            # Override GPU configuration
            gpu_type="b200",
            gpu_count=1,
            
            # Add custom environment variables  
            environment={
                "CUDA_VISIBLE_DEVICES": "0,1",
                "PYTORCH_CUDA_ALLOC_CONF": "max_split_size_mb:512"
            },
            
            # SSH key is still auto-detected if not specified
            # Or you can explicitly provide one:
            # ssh_public_key="ssh-rsa AAAAB3... user@host"
        )
        
        # Using typed response
        rental_id = rental.rental_id
        print(f"Rental started with ID: {rental_id}")
        print(f"Container: {rental.container_name}")
        print(f"Status: {rental.status}")
        
        # Wait for rental with custom timeout and poll interval
        print("Waiting for rental to become active...")
        status = client.wait_for_rental(
            rental_id,
            timeout=600,  # Wait up to 10 minutes
            poll_interval=10  # Check every 10 seconds
        )
        
        print("Rental is now active!")
        
        # Display rental details using typed attributes
        if status.ssh_credentials:
            print(f"\nSSH credentials: {status.ssh_credentials}")
        
        # Display executor details
        executor = status.executor
        print(f"\nExecutor details:")
        print(f"  ID: {executor.id}")
        for gpu in executor.gpu_specs:
            print(f"  GPU: {gpu.name} - {gpu.memory_gb} GB")
        
    except TimeoutError as e:
        print(f"Timeout: {e}")
    except Exception as e:
        print(f"Error: {e}")

if __name__ == "__main__":
    main()