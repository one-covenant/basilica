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

# Print SSH details if available
ssh = status.get("ssh_access")
if isinstance(ssh, dict):
    port = ssh.get("port", 22)
    user = ssh.get("user", "root")
    host = ssh.get("host")
    if host:
        print(f"ssh -p {port} {user}@{host}")
    else:
        print("SSH access reported but host missing")
else:
    print("No SSH access (no_ssh=True or not provisioned)")

# When done, stop the rental
# client.stop_rental(rental["rental_id"])