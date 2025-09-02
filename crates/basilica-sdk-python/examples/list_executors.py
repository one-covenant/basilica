#!/usr/bin/env python3
"""
Example: List available executors
"""

from basilica import BasilicaClient
import os
import json

def main():
    # Get API URL and token from environment variables
    api_url = os.environ.get("BASILICA_API_URL", "https://api.basilica.ai")
    api_token = os.environ.get("BASILICA_API_TOKEN")
    
    if not api_token:
        print("Please set BASILICA_API_TOKEN environment variable")
        return
    
    # Create client
    client = BasilicaClient(api_url, token=api_token)
    
    # Check API health
    print("Checking API health...")
    health = client.health_check()
    print(f"API Status: {health['status']}")
    print(f"Version: {health['version']}")
    print()
    
    # List available executors
    print("Listing available executors...")
    response = client.list_executors(available=True)
    
    executors = response.get("available_executors", [])
    print(f"Found {len(executors)} available executors")
    
    # Display executor details
    for executor in executors[:5]:  # Show first 5
        print(f"\nExecutor: {executor.get('executor_id', 'N/A')}")
        print(f"  Status: {executor.get('status', 'N/A')}")
        
        if "gpu_specs" in executor:
            for gpu in executor["gpu_specs"]:
                print(f"  GPU: {gpu.get('name', 'Unknown')} - {gpu.get('memory_gb', 0)} GB")
        
        if "cpu_spec" in executor:
            cpu = executor["cpu_spec"]
            print(f"  CPU: {cpu.get('cores', 0)} cores, {cpu.get('memory_gb', 0)} GB RAM")
        
        if "pricing" in executor:
            pricing = executor["pricing"]
            print(f"  Price: ${pricing.get('price_per_hour', 0):.2f}/hour")

if __name__ == "__main__":
    main()