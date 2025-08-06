# Basilica API Service

## Overview

API service that provides a simple interface to the Basilica GPU network. Handles GPU discovery, rental management, and log streaming.

## Command-to-Endpoint Mapping

This section shows how Basilica CLI commands map to API endpoints:

| CLI Command | API Endpoint | Method | Description |
|-------------|--------------|---------|-------------|
| `basilica init` | `POST /api/v1/register` | POST | Register user and setup account for credits |
| `basilica ls` | `GET /api/v1/rentals/available` | GET | List available GPU resources with filtering |
| `basilica up` | `POST /api/v1/rentals` | POST | Provision and start GPU instances |
| `basilica ps` | `GET /api/v1/rentals` | GET | List active rentals and their status |
| `basilica status <uid>` | `GET /api/v1/rentals/{rental_id}` | GET | Check specific rental status |
| `basilica logs <uid>` | `GET /api/v1/rentals/{rental_id}/logs` | GET | Stream rental logs via Server-Sent Events |
| `basilica down <uid>` | `DELETE /api/v1/rentals/{rental_id}` | DELETE | Terminate active rental |
| `basilica exec <uid> "cmd"` | *Via SSH* | N/A | Execute commands using SSH access from rental |
| `basilica ssh <uid>` | *Via SSH* | N/A | SSH access using connection info from rental |
| `basilica cp` | *Via SSH/rsync* | N/A | File transfer using SSH access from rental |

**Notes:**
- Commands like `exec`, `ssh`, and `cp` use SSH access provided in the rental response rather than direct API calls
- `basilica up <uid>` maps to `POST /api/v1/rentals` with the executor ID specified in the request body
- `basilica ls` with filters maps to `GET /api/v1/rentals/available?gpu_type=X&min_gpu_count=Y` etc.
- Interactive commands (`basilica up`, `basilica down`) without parameters use `GET` endpoints first to present options

## API Endpoints

### Registration
- `POST /api/v1/register` - Register user and create account for holding credits
- `GET /api/v1/credit-wallet/{user_id}` - Get wallet address for registered user

### GPU Discovery
- `GET /api/v1/rentals/available` - List available GPU executors with filtering options

### Rental Management
- `POST /api/v1/rentals` - Create new GPU rental with executor ID, Docker image, SSH key
- `GET /api/v1/rentals` - List user's active rentals with status filtering
- `GET /api/v1/rentals/{rental_id}` - Get detailed rental status and SSH access info
- `DELETE /api/v1/rentals/{rental_id}` - Terminate active rental

### Log Streaming
- `GET /api/v1/rentals/{rental_id}/logs` - Stream rental logs via Server-Sent Events

## TODO

1. **Fix path inconsistency**: Client uses `/api/v1/register/wallet` but server has `/credit-wallet/:user_id`
2. **Migrate basilica-cli**: Replace custom ApiClient with BasilicaClient from basilica-api crate
3. **Complete SSE implementation**: Implement proper Server-Sent Events parsing for `follow_logs` method in client