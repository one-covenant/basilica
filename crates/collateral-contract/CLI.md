# Collateral Contract CLI

A comprehensive command-line interface for interacting with the Collateral smart contract.

## Installation

Build the CLI tool:

```bash
cd crates/collateral-contract
cargo build --release --bin collateral-cli
```

The binary will be available at `target/release/collateral-cli`.

## Usage

The CLI is organized into three main command categories:

### Network Selection

The CLI supports multiple networks:

- **Mainnet** (default): Production Opentensor network
- **Testnet**: Test Opentensor network
- **Local**: Local development network

You can specify the network using the `--network` flag:

```bash
# Use mainnet (default)
collateral-cli query netuid

# Use testnet
collateral-cli --network testnet query netuid

# Use local network
collateral-cli --network local query netuid

# Override with custom contract address
collateral-cli --contract-address 0x1234...abcd query netuid
```

### Command Categories

1. **Transaction commands** (`tx`) - Interact with the contract to modify state
2. **Query commands** (`query`) - Read data from the contract
3. **Event commands** (`events`) - Scan and analyze contract events

### Global Options

- `--help` - Show help information
- `--version` - Show version information
- `--network <NETWORK>` - Network to connect to (mainnet, testnet, local) [default: mainnet]
- `--contract-address <ADDRESS>` - Contract address (overrides network default)

### Authentication

Most transaction commands require a private key for signing. You can provide it in two ways:

1. Command line argument: `--private-key <hex_string>`
2. Environment variable: `export PRIVATE_KEY=<hex_string>`

## Transaction Commands

### Deposit Collateral

Deposit ETH as collateral for an executor:

```bash
# On mainnet (default)
collateral-cli tx deposit \
  --private-key 0x1234... \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --amount 1000000000000000000

# On testnet
collateral-cli --network testnet tx deposit \
  --private-key 0x1234... \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --amount 1000000000000000000

# With custom contract address
collateral-cli --contract-address 0x5FbDB2315678afecb367f032d93F642f64180aa3 tx deposit \
  --private-key 0x1234... \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --amount 1000000000000000000
```

**Parameters:**

- `--private-key`: Private key for transaction signing (64 hex chars)
- `--hotkey`: Hotkey identifier (64 hex chars, 32 bytes)
- `--executor-id`: Executor ID (integer)
- `--amount`: Amount in wei (integer or hex string)

### Reclaim Collateral

Request to reclaim deposited collateral:

```bash
collateral-cli tx reclaim-collateral \
  --private-key 0x1234... \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --url "https://example.com/proof" \
  --url-content-md5-checksum abcdef1234567890abcdef1234567890
```

**Parameters:**

- `--private-key`: Private key for transaction signing
- `--hotkey`: Hotkey identifier (64 hex chars)
- `--executor-id`: Executor ID (integer)
- `--url`: URL containing proof for reclaim
- `--url-content-md5-checksum`: MD5 checksum of URL content (32 hex chars, 16 bytes)

### Finalize Reclaim

Finalize a pending reclaim request:

```bash
collateral-cli tx finalize-reclaim \
  --private-key 0x1234... \
  --reclaim-request-id 42
```

**Parameters:**

- `--private-key`: Private key for transaction signing
- `--reclaim-request-id`: ID of the reclaim request (integer or hex string)

### Deny Reclaim

Deny a reclaim request with proof:

```bash
collateral-cli tx deny-reclaim \
  --private-key 0x1234... \
  --reclaim-request-id 42 \
  --url "https://example.com/denial-proof" \
  --url-content-md5-checksum abcdef1234567890abcdef1234567890
```

**Parameters:**

- `--private-key`: Private key for transaction signing
- `--reclaim-request-id`: ID of the reclaim request
- `--url`: URL containing proof for denial
- `--url-content-md5-checksum`: MD5 checksum of URL content

### Slash Collateral

Slash collateral for misconduct:

```bash
collateral-cli tx slash-collateral \
  --private-key 0x1234... \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --url "https://example.com/slash-proof" \
  --url-content-md5-checksum abcdef1234567890abcdef1234567890
```

**Parameters:**

- `--private-key`: Private key for transaction signing
- `--hotkey`: Hotkey identifier
- `--executor-id`: Executor ID
- `--url`: URL containing proof for slashing
- `--url-content-md5-checksum`: MD5 checksum of URL content

## Query Commands

### Get Network UID

```bash
# On mainnet (default)
collateral-cli query netuid

# On testnet
collateral-cli --network testnet query netuid

# With custom contract
collateral-cli --contract-address 0x1234...abcd query netuid
```

Returns the network UID configured in the contract.

### Get Trustee Address

```bash
collateral-cli query trustee
```

Returns the address of the contract trustee.

### Get Decision Timeout

```bash
collateral-cli query decision-timeout
```

Returns the decision timeout in seconds.

### Get Minimum Collateral Increase

```bash
collateral-cli query min-collateral-increase
```

Returns the minimum collateral increase amount in wei.

### Get Executor to Miner Mapping

```bash
collateral-cli query executor-to-miner \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123
```

Returns the miner address associated with the executor.

### Get Collateral Amount

```bash
collateral-cli query collaterals \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123
```

Returns the collateral amount for the specified executor.

### Get Reclaim Details

```bash
collateral-cli query reclaims \
  --reclaim-request-id 42
```

Returns detailed information about a reclaim request.

## Event Commands

### Scan Contract Events

Scan for contract events within a block range:

```bash
# Pretty format (default) on mainnet
collateral-cli events scan --from-block 1000

# JSON format on testnet
collateral-cli --network testnet events scan --from-block 1000 --format json

# With custom contract address
collateral-cli --contract-address 0x1234...abcd events scan --from-block 1000
```

**Parameters:**

- `--from-block`: Starting block number
- `--format`: Output format (`pretty` or `json`)

**Output includes:**

- Deposit events
- Reclaim events
- Slash events

## Examples

### Complete Workflow Example

1. **Deposit collateral:**

```bash
export PRIVATE_KEY=0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12

collateral-cli tx deposit \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --amount 5000000000000000000  # 5 ETH
```

2. **Check collateral balance:**

```bash
collateral-cli query collaterals \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123
```

3. **Request reclaim:**

```bash
collateral-cli tx reclaim-collateral \
  --hotkey 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef \
  --executor-id 123 \
  --url "https://myproof.com/evidence" \
  --url-content-md5-checksum d41d8cd98f00b204e9800998ecf8427e
```

4. **Check events:**

```bash
collateral-cli events scan --from-block 1000 --format json
```

### Monitoring Example

Monitor recent events:

```bash
# Get current block and scan recent events
collateral-cli events scan --from-block $(expr $(curl -s -X POST -H "Content-Type: application/json" --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' http://localhost:8545 | jq -r .result | sed 's/0x//') - 100)
```

## Error Handling

The CLI provides detailed error messages for common issues:

- Invalid hex format for hotkeys or checksums
- Network connectivity problems
- Invalid private keys
- Transaction failures
- Contract interaction errors

## Configuration

The CLI supports multiple network configurations:

### Network Configurations

- **Mainnet** (default):

  - Chain ID: 964
  - RPC URL: https://lite.chain.opentensor.ai:443
  - Default Contract: 0x0000000000000000000000000000000000000001

- **Testnet**:

  - Chain ID: 945
  - RPC URL: https://test.finney.opentensor.ai
  - Default Contract: 0x0000000000000000000000000000000000000001

- **Local**:
  - Chain ID: 31337
  - RPC URL: http://localhost:8545
  - Default Contract: 0x0000000000000000000000000000000000000001

### Additional Settings

- **MAX_BLOCKS_PER_SCAN**: Maximum blocks to scan per request (1000)

## Testing

Run the comprehensive test suite:

```bash
# Run all tests
cargo test

# Run CLI-specific tests
cargo test --bin collateral-cli

# Run library tests
cargo test --lib
```

The test suite includes:

- Argument parsing validation
- Data conversion functions
- Error handling scenarios
- Event formatting
- Integration test scenarios

## Security Notes

- **Private Key Security**: Never commit private keys to version control
- **Environment Variables**: Use environment variables for sensitive data
- **Network Security**: Ensure RPC endpoints are trusted
- **Input Validation**: The CLI validates all inputs before processing

## Troubleshooting

### Common Issues

1. **"Invalid hotkey format"**

   - Ensure hotkey is exactly 64 hex characters (32 bytes)
   - Remove any `0x` prefix if present

2. **"Invalid amount format"**

   - Use wei denomination for amounts
   - Support both decimal and hex formats

3. **"Invalid address format"**

   - Ensure contract address is exactly 40 hex characters (20 bytes)
   - Include or exclude 0x prefix as needed

4. **"Transaction failed"**

   - Check account balance for gas fees
   - Verify network connectivity
   - Ensure private key has required permissions

5. **"Contract call failed"**
   - Verify contract address and ABI
   - Check if contract is deployed on target network
   - Ensure function parameters are correct
   - Try using `--contract-address` to override default

### Debug Mode

Enable debug logging:

```bash
RUST_LOG=debug collateral-cli <command>
```

This will show detailed information about:

- Network requests
- Transaction details
- Contract interactions
- Error traces
