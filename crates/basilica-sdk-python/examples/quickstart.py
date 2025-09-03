#!/usr/bin/env python3
"""
Basilica SDK Quickstart - Minimal example
"""

from basilica import BasilicaClient

# That's it! The client auto-configures from environment variables
client = BasilicaClient()

# Start a rental with all defaults
rental = client.start_rental()

# Wait for it to be ready
status = client.wait_for_rental(rental["rental_id"])

# Get SSH details
ssh = status["ssh_access"]
print(f"ssh -p {ssh['port']} {ssh['user']}@{ssh['host']}")

# When done, stop the rental
# client.stop_rental(rental["rental_id"])