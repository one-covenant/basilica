use alloy_primitives::{Address, address};
// Deployed Collateral contract address in product environment, will be updated after deployment
pub const COLLATERAL_ADDRESS: Address = address!("0x0000000000000000000000000000000000000000");
pub const PROXY_ADDRESS: Address = address!("0x0000000000000000000000000000000000000001");
pub const CHAIN_ID: u64 = 964;
pub const RPC_URL: &str = "https://lite.chain.opentensor.ai:443";

// Test environment
pub const TEST_CHAIN_ID: u64 = 945;
pub const TEST_RPC_URL: &str = "https://test.finney.opentensor.ai";

pub const LOCAL_CHAIN_ID: u64 = 0;
pub const LOCAL_RPC_URL: &str = "http://localhost:9944";
pub const LOCAL_WS_URL: &str = "ws://localhost:9944";

pub const PRIVATE_KEY_FILE: &str = "$HOME/.basilca/private_key";

pub const MAX_BLOCKS_PER_SCAN: u64 = 1000;
