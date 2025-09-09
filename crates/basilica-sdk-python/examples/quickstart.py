#!/usr/bin/env python3
"""
Basilica SDK Quickstart - Minimal example
"""

from basilica import BasilicaClient

# That's it! The client auto-configures from environment variables
client = BasilicaClient()

# Start a rental with all defaults - returns typed RentalResponse
rental = client.start_rental(gpu_type="b200")
print(f"Rental started with ID: {rental.rental_id}")

# Wait for it to be ready - returns typed RentalStatusResponse
status = client.wait_for_rental(rental.rental_id)

# Print SSH details if available - using typed attributes
if status.ssh_credentials:
    print(f"SSH credentials: {status.ssh_credentials}")
else:
    print("No SSH access (no_ssh=True or not provisioned)")

# When done, stop the rental
# client.stop_rental(rental.rental_id)