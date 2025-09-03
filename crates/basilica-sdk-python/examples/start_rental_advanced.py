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
            
            # Override the default resources
            resources={
                "gpu_count": 2,
                "gpu_type": "a100"
            },
            
            # Add custom environment variables  
            environment={
                "CUDA_VISIBLE_DEVICES": "0,1",
                "PYTORCH_CUDA_ALLOC_CONF": "max_split_size_mb:512"
            },
            
            # SSH key is still auto-detected if not specified
            # Or you can explicitly provide one:
            # ssh_public_key="ssh-rsa AAAAB3... user@host"
        )
        
        rental_id = rental["rental_id"]
        print(f"Rental started with ID: {rental_id}")
        
        # Wait for rental with custom timeout and poll interval
        print("Waiting for rental to become active...")
        status = client.wait_for_rental(
            rental_id,
            timeout=600,  # Wait up to 10 minutes
            poll_interval=10  # Check every 10 seconds
        )
        
        print("Rental is now active!")
        
        # Display rental details
        if "ssh_access" in status:
            ssh = status["ssh_access"]
            print(f"\nSSH Access:")
            print(f"  ssh -p {ssh.get('port', 22)} {ssh.get('user', 'root')}@{ssh.get('host', 'N/A')}")
        
    except TimeoutError as e:
        print(f"Timeout: {e}")
    except Exception as e:
        print(f"Error: {e}")

if __name__ == "__main__":
    main()