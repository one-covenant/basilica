#!/usr/bin/env python3
"""
Basilica SDK Quickstart - Minimal example
"""

from basilica import BasilicaClient

# That's it! The client auto-configures from environment variables
client = BasilicaClient()

# Start a rental with all defaults - returns typed RentalResponse with SSH credentials
rental = client.start_rental(gpu_type="b200")
print(f"Rental started with ID: {rental.rental_id}")

# Print SSH details if available - using typed attributes
if rental.ssh_credentials:
    print(f"SSH credentials: {rental.ssh_credentials}")
else:
    print("No SSH access (no_ssh=True or not provisioned)")

# When done, stop the rental
# client.stop_rental(rental.rental_id)