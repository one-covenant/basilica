#!/usr/bin/env python3
"""
Example: Start a GPU rental
"""

from basilica import BasilicaClient
import os
import time

def main():
    # Get API URL and token from environment variables
    api_url = os.environ.get("BASILICA_API_URL", "https://api.basilica.ai")
    api_token = os.environ.get("BASILICA_API_TOKEN")
    ssh_key = os.environ.get("SSH_PUBLIC_KEY")
    
    if not api_token:
        print("Please set BASILICA_API_TOKEN environment variable")
        return
    
    if not ssh_key:
        # Try to read from default location
        ssh_key_path = os.path.expanduser("~/.ssh/id_rsa.pub")
        if os.path.exists(ssh_key_path):
            with open(ssh_key_path) as f:
                ssh_key = f.read().strip()
        else:
            print("No SSH public key found. Set SSH_PUBLIC_KEY or create ~/.ssh/id_rsa.pub")
            return
    
    # Create client
    client = BasilicaClient(api_url, token=api_token)
    
    # Start a rental
    print("Starting GPU rental...")
    rental = client.start_rental(
        container_image="nvidia/cuda:12.2.0-base-ubuntu22.04",
        ssh_public_key=ssh_key,
        resources={
            "gpu_count": 1,
            "gpu_type": "h100"
        },
        environment={
            "CUDA_VISIBLE_DEVICES": "0"
        }
    )
    
    rental_id = rental["rental_id"]
    print(f"Rental started with ID: {rental_id}")
    
    # Wait for rental to become active
    print("Waiting for rental to become active...")
    max_wait = 60  # seconds
    start_time = time.time()
    
    while time.time() - start_time < max_wait:
        status = client.get_rental(rental_id)
        state = status.get("status", {}).get("state", "Unknown")
        
        if state == "Active":
            print("Rental is now active!")
            
            # Display SSH access info
            if "ssh_access" in status:
                ssh = status["ssh_access"]
                print(f"\nSSH Access:")
                print(f"  Host: {ssh.get('host', 'N/A')}")
                print(f"  Port: {ssh.get('port', 22)}")
                print(f"  User: {ssh.get('user', 'root')}")
                print(f"\nConnect with:")
                print(f"  ssh -p {ssh.get('port', 22)} {ssh.get('user', 'root')}@{ssh.get('host', 'N/A')}")
            break
        
        print(f"Current state: {state}")
        time.sleep(5)
    else:
        print("Timeout waiting for rental to become active")
    
    # Optionally stop the rental
    # print("\nStopping rental...")
    # client.stop_rental(rental_id)
    # print("Rental stopped")

if __name__ == "__main__":
    main()