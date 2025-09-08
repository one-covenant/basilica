#!/usr/bin/env python3
"""
Example: Start a GPU rental with simplified API
"""

from basilica import BasilicaClient
import os

def main():
    # Create client - automatically uses BASILICA_API_TOKEN and BASILICA_API_URL from env
    # No need to manually handle tokens or SSH keys
    client = BasilicaClient()
    
    try:
        # Start a rental with minimal configuration
        # SSH key is auto-detected from ~/.ssh/id_*.pub
        # Default container image and resources are used
        print("Starting GPU rental...")
        rental = client.start_rental()
        
        rental_id = rental.rental_id  # Now using typed attribute access!
        print(f"Rental started with ID: {rental_id}")
        
        # Use the new blocking wait method
        print("Waiting for rental to become active...")
        status = client.wait_for_rental(rental_id)
        
        print("Rental is now active!")
        
        # Display SSH access info using typed attributes
        if status.ssh_access:
            ssh = status.ssh_access
            print(f"\nSSH Access:")
            print(f"  Host: {ssh.host}")
            print(f"  Port: {ssh.port}")
            print(f"  User: {ssh.user}")
            print(f"\nConnect with:")
            print(f"  ssh -p {ssh.port} {ssh.user}@{ssh.host}")
        
        # Optionally stop the rental
        # print("\nStopping rental...")
        # client.stop_rental(rental_id)
        # print("Rental stopped")
        
    except TimeoutError as e:
        print(f"Error: {e}")
    except Exception as e:
        print(f"Error: {e}")

if __name__ == "__main__":
    main()