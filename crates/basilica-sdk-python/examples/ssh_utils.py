"""
SSH utilities for Basilica SDK

Provides functions for parsing SSH credentials and generating SSH commands.
"""

from typing import Optional, Tuple
import os


def parse_ssh_credentials(credentials: str) -> Tuple[str, str, int]:
    """
    Parse SSH credentials string in format 'user@host:port'.
    
    Args:
        credentials: SSH credentials string (e.g., 'root@84.200.81.243:32776')
        
    Returns:
        Tuple of (user, host, port)
        
    Raises:
        ValueError: If credentials format is invalid
    """
    if not credentials:
        raise ValueError("SSH credentials cannot be empty")
    
    try:
        # Split user@host:port
        if '@' not in credentials:
            raise ValueError(f"Invalid SSH credentials format: {credentials}")
        
        user_part, host_port = credentials.split('@', 1)
        
        if ':' not in host_port:
            raise ValueError(f"Invalid SSH credentials format (missing port): {credentials}")
        
        host, port_str = host_port.rsplit(':', 1)
        
        # Validate and parse port
        try:
            port = int(port_str)
            if port < 1 or port > 65535:
                raise ValueError(f"Invalid port number: {port}")
        except ValueError:
            raise ValueError(f"Invalid port number: {port_str}")
        
        return user_part, host, port
        
    except Exception as e:
        raise ValueError(f"Failed to parse SSH credentials '{credentials}': {e}")


def format_ssh_command(
    credentials: str,
    ssh_key_path: Optional[str] = None
) -> str:
    """
    Generate a complete SSH command from credentials string.
    
    Args:
        credentials: SSH credentials string (e.g., 'root@84.200.81.243:32776')
        ssh_key_path: Optional path to SSH private key. 
                      Defaults to ~/.ssh/basilica_ed25519
        
    Returns:
        Complete SSH command string
        
    Raises:
        ValueError: If credentials format is invalid
    """
    user, host, port = parse_ssh_credentials(credentials)
    
    # Use default key path if not provided (keep ~ unexpanded for display)
    if ssh_key_path is None:
        display_key_path = "~/.ssh/basilica_ed25519"
    else:
        display_key_path = ssh_key_path
    
    # Build SSH command
    return f"ssh -i {display_key_path} {user}@{host} -p {port}"


def print_ssh_instructions(
    credentials: Optional[str],
    rental_id: str,
    ssh_key_path: Optional[str] = None
) -> None:
    """
    Print formatted SSH connection instructions.
    
    Args:
        credentials: SSH credentials string or None
        rental_id: Rental ID for context
        ssh_key_path: Optional path to SSH private key
    """
    if not credentials:
        print(f"No SSH access available for rental {rental_id}")
        print("(SSH was disabled with no_ssh=True or not yet provisioned)")
        return
    
    try:
        ssh_command = format_ssh_command(credentials, ssh_key_path)
        print(f"\nSSH Connection Instructions for rental {rental_id}:")
        print(f"  Command: {ssh_command}")
        
        # Parse for additional details
        user, host, port = parse_ssh_credentials(credentials)
        print(f"  Host: {host}")
        print(f"  Port: {port}")
        print(f"  User: {user}")
        
        # Check if key exists (expand for checking, but display with ~)
        key_path_expanded = os.path.expanduser(ssh_key_path or "~/.ssh/basilica_ed25519")
        display_path = ssh_key_path or "~/.ssh/basilica_ed25519"
        if not os.path.exists(key_path_expanded):
            print(f"\n  Warning: SSH key not found at {display_path}")
            print(f"     Please ensure your SSH key is properly configured")
            
    except ValueError as e:
        print(f"Error parsing SSH credentials: {e}")
        print(f"Raw credentials: {credentials}")