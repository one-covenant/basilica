use alloy_primitives::{Address, FixedBytes, U256};
use alloy_provider::ProviderBuilder;
use alloy_sol_types::sol;

use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;

pub mod config;

use config::{CHAIN_ID, COLLATERAL_ADDRESS, RPC_URL};

sol!(
    #[allow(missing_docs)]
    #[sol(
        rpc,
        bytecode = "6080604052600436106100aa575f3560e01c8063881cf23b11610063578063881cf23b1461020f57806396c42a0a146102395780639cf9631814610263578063b4314e2b146102a3578063dac0ed01146102cb578063e00f7b8114610307576100e1565b806306016f711461011357806307d867881461013d5780632497d31f146101655780634a7393b21461018d578063501e4e91146101b7578063679fcc21146101d3576100e1565b366100e1576040517f84ee6c0a00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b6040517f84ee6c0a00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b34801561011e575f5ffd5b5061012761032f565b60405161013491906114a2565b60405180910390f35b348015610148575f5ffd5b50610163600480360381019061015e91906114f6565b610341565b005b348015610170575f5ffd5b5061018b6004803603810190610186919061160a565b61076c565b005b348015610198575f5ffd5b506101a1610abf565b6040516101ae919061169d565b60405180910390f35b6101d160048036038101906101cc91906116b6565b610ac5565b005b3480156101de575f5ffd5b506101f960048036038101906101f491906116b6565b610d50565b6040516102069190611733565b60405180910390f35b34801561021a575f5ffd5b50610223610d8d565b6040516102309190611733565b60405180910390f35b348015610244575f5ffd5b5061024d610db2565b60405161025a919061176e565b60405180910390f35b34801561026e575f5ffd5b50610289600480360381019061028491906114f6565b610dcb565b60405161029a9594939291906117a5565b60405180910390f35b3480156102ae575f5ffd5b506102c960048036038101906102c491906117f6565b610e3b565b005b3480156102d6575f5ffd5b506102f160048036038101906102ec91906116b6565b6110a6565b6040516102fe919061169d565b60405180910390f35b348015610312575f5ffd5b5061032d6004803603810190610328919061160a565b6110c6565b005b5f5f9054906101000a900461ffff1681565b5f60045f8381526020019081526020015f2090505f816003015403610392576040517f642e3ad700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b42816004015f9054906101000a900467ffffffffffffffff1667ffffffffffffffff16106103ec576040517f3355482c00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f815f015490505f826001015f9054906101000a900460801b90505f836002015f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690505f8460030154905060045f8781526020019081526020015f205f5f82015f9055600182015f6101000a8154906fffffffffffffffffffffffffffffffff0219169055600282015f6101000a81549073ffffffffffffffffffffffffffffffffffffffff0219169055600382015f9055600482015f6101000a81549067ffffffffffffffff021916905550508060055f8681526020019081526020015f205f856fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f82825461050e9190611894565b925050819055508060035f8681526020019081526020015f205f856fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f20541015610595576040517fc4d7ebda00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b8060035f8681526020019081526020015f205f856fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f8282546105ea9190611894565b92505081905550826fffffffffffffffffffffffffffffffff191684877f5bad7ce9103a04027245449328134e980faffde663decc58fde44536d70e04cb85856040516106389291906118c7565b60405180910390a45f8273ffffffffffffffffffffffffffffffffffffffff16826040516106659061191b565b5f6040518083038185875af1925050503d805f811461069f576040519150601f19603f3d011682016040523d82523d5f602084013e6106a4565b606091505b50509050806106df576040517f90b8ec1800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60025f8781526020019081526020015f205f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff16021790555050505050505050565b5f60029054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff16146107f2576040517f5aa309bb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60035f8781526020019081526020015f205f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205490505f8103610875576040517fcbca5aa200000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60035f8881526020019081526020015f205f876fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f20819055505f60025f8881526020019081526020015f205f876fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690505f5f73ffffffffffffffffffffffffffffffffffffffff168360405161094e9061191b565b5f6040518083038185875af1925050503d805f8114610988576040519150601f19603f3d011682016040523d82523d5f602084013e61098d565b606091505b50509050806109c8576040517f90b8ec1800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60025f8a81526020019081526020015f205f896fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff1602179055508173ffffffffffffffffffffffffffffffffffffffff16876fffffffffffffffffffffffffffffffff1916897f59fb97be32a1253f478d846d76623d23d63de327cedb08d6ccacf150ff91fd7f868a8a8a604051610aad9493929190611989565b60405180910390a45050505050505050565b60015481565b600154341015610b01576040517f5945ea5600000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60025f8481526020019081526020015f205f836fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690505f73ffffffffffffffffffffffffffffffffffffffff168173ffffffffffffffffffffffffffffffffffffffff1603610c26573360025f8581526020019081526020015f205f846fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff160217905550610c8c565b3373ffffffffffffffffffffffffffffffffffffffff168173ffffffffffffffffffffffffffffffffffffffff1614610c8b576040517f9ea26eb800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5b3460035f8581526020019081526020015f205f846fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f828254610ce191906119c7565b925050819055503373ffffffffffffffffffffffffffffffffffffffff16826fffffffffffffffffffffffffffffffff1916847fe07d9525c767a77f1c54bc2ceffdade4dfdadae69f96c94dcdb65a232463236934604051610d43919061169d565b60405180910390a4505050565b6002602052815f5260405f20602052805f5260405f205f915091509054906101000a900473ffffffffffffffffffffffffffffffffffffffff1681565b5f60029054906101000a900473ffffffffffffffffffffffffffffffffffffffff1681565b5f60169054906101000a900467ffffffffffffffff1681565b6004602052805f5260405f205f91509050805f015490806001015f9054906101000a900460801b90806002015f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690806003015490806004015f9054906101000a900467ffffffffffffffff16905085565b5f60029054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610ec1576040517f5aa309bb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60045f8681526020019081526020015f2090505f816003015403610f12576040517f642e3ad700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b42816004015f9054906101000a900467ffffffffffffffff1667ffffffffffffffff161015610f6d576040517ffc9e5c0200000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b806003015460055f835f015481526020019081526020015f205f836001015f9054906101000a900460801b6fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f828254610fd99190611894565b92505081905550847f6067048e78883441f7c7f1b3ad8f94a88b1567a486029be8a830e9455f61edb9858585604051611014939291906119fa565b60405180910390a260045f8681526020019081526020015f205f5f82015f9055600182015f6101000a8154906fffffffffffffffffffffffffffffffff0219169055600282015f6101000a81549073ffffffffffffffffffffffffffffffffffffffff0219169055600382015f9055600482015f6101000a81549067ffffffffffffffff021916905550505050505050565b6003602052815f5260405f20602052805f5260405f205f91509150505481565b3373ffffffffffffffffffffffffffffffffffffffff1660025f8781526020019081526020015f205f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1614611190576040517f9ea26eb800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60035f8781526020019081526020015f205f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205490505f60055f8881526020019081526020015f205f876fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205490505f81836112319190611894565b90505f810361126c576040517fcbca5aa200000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f60169054906101000a900467ffffffffffffffff164261128e9190611a2a565b90506040518060a001604052808a8152602001896fffffffffffffffffffffffffffffffff191681526020013373ffffffffffffffffffffffffffffffffffffffff1681526020018381526020018267ffffffffffffffff1681525060045f60065f81546112fb90611a65565b91905081905581526020019081526020015f205f820151815f01556020820151816001015f6101000a8154816fffffffffffffffffffffffffffffffff021916908360801c02179055506040820151816002015f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff160217905550606082015181600301556080820151816004015f6101000a81548167ffffffffffffffff021916908367ffffffffffffffff1602179055509050508160055f8b81526020019081526020015f205f8a6fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f82825461141b91906119c7565b92505081905550876fffffffffffffffffffffffffffffffff1916896006547ffd3d4f1c9a32bf51e721c588fa66ff4eb6635b376d5689cc9fe4361ab901a17f3386868d8d8d60405161147396959493929190611aac565b60405180910390a4505050505050505050565b5f61ffff82169050919050565b61149c81611486565b82525050565b5f6020820190506114b55f830184611493565b92915050565b5f5ffd5b5f5ffd5b5f819050919050565b6114d5816114c3565b81146114df575f5ffd5b50565b5f813590506114f0816114cc565b92915050565b5f6020828403121561150b5761150a6114bb565b5b5f611518848285016114e2565b91505092915050565b5f819050919050565b61153381611521565b811461153d575f5ffd5b50565b5f8135905061154e8161152a565b92915050565b5f7fffffffffffffffffffffffffffffffff0000000000000000000000000000000082169050919050565b61158881611554565b8114611592575f5ffd5b50565b5f813590506115a38161157f565b92915050565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f83601f8401126115ca576115c96115a9565b5b8235905067ffffffffffffffff8111156115e7576115e66115ad565b5b602083019150836001820283011115611603576116026115b1565b5b9250929050565b5f5f5f5f5f60808688031215611623576116226114bb565b5b5f61163088828901611540565b955050602061164188828901611595565b945050604086013567ffffffffffffffff811115611662576116616114bf565b5b61166e888289016115b5565b9350935050606061168188828901611595565b9150509295509295909350565b611697816114c3565b82525050565b5f6020820190506116b05f83018461168e565b92915050565b5f5f604083850312156116cc576116cb6114bb565b5b5f6116d985828601611540565b92505060206116ea85828601611595565b9150509250929050565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f61171d826116f4565b9050919050565b61172d81611713565b82525050565b5f6020820190506117465f830184611724565b92915050565b5f67ffffffffffffffff82169050919050565b6117688161174c565b82525050565b5f6020820190506117815f83018461175f565b92915050565b61179081611521565b82525050565b61179f81611554565b82525050565b5f60a0820190506117b85f830188611787565b6117c56020830187611796565b6117d26040830186611724565b6117df606083018561168e565b6117ec608083018461175f565b9695505050505050565b5f5f5f5f6060858703121561180e5761180d6114bb565b5b5f61181b878288016114e2565b945050602085013567ffffffffffffffff81111561183c5761183b6114bf565b5b611848878288016115b5565b9350935050604061185b87828801611595565b91505092959194509250565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f61189e826114c3565b91506118a9836114c3565b92508282039050818111156118c1576118c0611867565b5b92915050565b5f6040820190506118da5f830185611724565b6118e7602083018461168e565b9392505050565b5f81905092915050565b50565b5f6119065f836118ee565b9150611911826118f8565b5f82019050919050565b5f611925826118fb565b9150819050919050565b5f82825260208201905092915050565b828183375f83830152505050565b5f601f19601f8301169050919050565b5f611968838561192f565b935061197583858461193f565b61197e8361194d565b840190509392505050565b5f60608201905061199c5f83018761168e565b81810360208301526119af81858761195d565b90506119be6040830184611796565b95945050505050565b5f6119d1826114c3565b91506119dc836114c3565b92508282019050808211156119f4576119f3611867565b5b92915050565b5f6040820190508181035f830152611a1381858761195d565b9050611a226020830184611796565b949350505050565b5f611a348261174c565b9150611a3f8361174c565b9250828201905067ffffffffffffffff811115611a5f57611a5e611867565b5b92915050565b5f611a6f826114c3565b91507fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff8203611aa157611aa0611867565b5b600182019050919050565b5f60a082019050611abf5f830189611724565b611acc602083018861168e565b611ad9604083018761175f565b8181036060830152611aec81858761195d565b9050611afb6080830184611796565b97965050505050505056fea2646970667358221220b356ba6b541d23cd2d1cd8e5f34aabd27299f62b95ce37385aa4fae4695e21c964736f6c634300081c0033"
    )]
    Collateral,
    "./src/Collateral.json"
);

#[derive(Debug, Clone)]
pub struct Reclaim {
    pub hotkey: u32,
    pub executor_id: u16,
    pub miner: Address,
    pub amount: U256,
    pub deny_timeout: u64,
}

impl From<(FixedBytes<32>, FixedBytes<16>, Address, U256, u64)> for Reclaim {
    fn from(tuple: (FixedBytes<32>, FixedBytes<16>, Address, U256, u64)) -> Self {
        Self {
            hotkey: u32::from_be_bytes(tuple.0[0..4].try_into().unwrap()),
            executor_id: u16::from_be_bytes(tuple.1[0..2].try_into().unwrap()),
            miner: tuple.2,
            amount: tuple.3,
            deny_timeout: tuple.4,
        }
    }
}

// get the collateral contract instance
pub async fn get_collateral(
    private_key: &str,
) -> Result<Collateral::CollateralInstance<impl alloy_provider::Provider>, anyhow::Error> {
    let mut signer: PrivateKeySigner = private_key.parse()?;
    signer.set_chain_id(Some(CHAIN_ID));

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(RPC_URL)
        .await?;

    let contract = Collateral::new(COLLATERAL_ADDRESS, provider);

    Ok(contract)
}

// transactions
pub async fn deposit(
    private_key: &str,
    hotkey: [u8; 32],
    executor_id: u128,
    amount: U256,
) -> Result<(), anyhow::Error> {
    let mut signer: PrivateKeySigner = private_key.parse()?;

    signer.set_chain_id(Some(CHAIN_ID));

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(RPC_URL)
        .await?;

    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);

    let executor_bytes = executor_id.to_be_bytes();
    let tx = contract
        .deposit(
            FixedBytes::from_slice(&hotkey),
            FixedBytes::from_slice(&executor_bytes),
        )
        .value(amount);
    let tx = tx.send().await?;
    let receipt = tx.get_receipt().await?;
    tracing::info!("{receipt:?}");
    Ok(())
}

pub async fn reclaim_collateral(
    private_key: &str,
    hotkey: [u8; 32],
    executor_id: u128,
    url: &str,
    url_content_md5_checksum: u128,
) -> Result<(), anyhow::Error> {
    let mut signer: PrivateKeySigner = private_key.parse()?;
    signer.set_chain_id(Some(CHAIN_ID));

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(RPC_URL)
        .await?;

    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);

    let executor_bytes = executor_id.to_be_bytes();
    let tx = contract.reclaimCollateral(
        FixedBytes::from_slice(&hotkey),
        FixedBytes::from_slice(&executor_bytes),
        url.to_string(),
        FixedBytes::from_slice(&url_content_md5_checksum.to_be_bytes()),
    );
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

pub async fn finalize_reclaim(
    private_key: &str,
    reclaim_request_id: U256,
) -> Result<(), anyhow::Error> {
    let mut signer: PrivateKeySigner = private_key.parse()?;
    signer.set_chain_id(Some(CHAIN_ID));

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(RPC_URL)
        .await?;

    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);

    let tx = contract.finalizeReclaim(reclaim_request_id);
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

pub async fn deny_reclaim(
    private_key: &str,
    reclaim_request_id: U256,
    url: &str,
    url_content_md5_checksum: u128,
) -> Result<(), anyhow::Error> {
    let mut signer: PrivateKeySigner = private_key.parse()?;
    signer.set_chain_id(Some(CHAIN_ID));

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(RPC_URL)
        .await?;

    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);

    let tx = contract.denyReclaimRequest(
        reclaim_request_id,
        url.to_string(),
        FixedBytes::from_slice(&url_content_md5_checksum.to_be_bytes()),
    );
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

pub async fn slash_collateral(
    private_key: &str,
    hotkey: [u8; 32],
    executor_id: u128,
    url: &str,
    url_content_md5_checksum: u128,
) -> Result<(), anyhow::Error> {
    let mut signer: PrivateKeySigner = private_key.parse()?;
    signer.set_chain_id(Some(CHAIN_ID));

    let provider = ProviderBuilder::new()
        .wallet(signer)
        .connect(RPC_URL)
        .await?;

    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);

    let executor_bytes = executor_id.to_be_bytes();
    let tx = contract.slashCollateral(
        FixedBytes::from_slice(&hotkey),
        FixedBytes::from_slice(&executor_bytes),
        url.to_string(),
        FixedBytes::from_slice(&url_content_md5_checksum.to_be_bytes()),
    );
    let tx = tx.send().await?;
    tx.get_receipt().await?;
    Ok(())
}

// Get methods

pub async fn netuid() -> Result<u16, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let netuid = contract.NETUID().call().await?;
    Ok(netuid)
}

pub async fn trustee() -> Result<Address, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let trustee = contract.TRUSTEE().call().await?;
    Ok(trustee)
}

pub async fn decision_timeout() -> Result<u64, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let decision_timeout = contract.DECISION_TIMEOUT().call().await?;
    Ok(decision_timeout)
}

pub async fn min_collateral_increase() -> Result<U256, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let min_collateral_increase = contract.MIN_COLLATERAL_INCREASE().call().await?;
    Ok(min_collateral_increase)
}

pub async fn executor_to_miner(
    hotkey: [u8; 32],
    executor_id: u128,
) -> Result<Address, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let executor_bytes = executor_id.to_be_bytes();
    let executor_to_miner = contract
        .executorToMiner(
            FixedBytes::from_slice(&hotkey),
            FixedBytes::from_slice(&executor_bytes),
        )
        .call()
        .await?;
    Ok(executor_to_miner)
}

pub async fn collaterals(hotkey: [u8; 32], executor_id: u128) -> Result<U256, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let executor_bytes = executor_id.to_be_bytes();
    let collaterals = contract
        .collaterals(
            FixedBytes::from_slice(&hotkey),
            FixedBytes::from_slice(&executor_bytes),
        )
        .call()
        .await?;
    Ok(collaterals)
}

pub async fn reclaims(reclaim_request_id: U256) -> Result<Reclaim, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let result = contract.reclaims(reclaim_request_id).call().await?;
    let reclaim = Reclaim::from((
        result.hotkey,
        result.executorId,
        result.miner,
        result.amount,
        result.denyTimeout,
    ));
    Ok(reclaim)
}

#[cfg(test)]
// The unit tests are for testing against local network
// Just can be executed if local subtensor node is running
mod test {
    use super::*;
    use bittensor::api::api::{self as bittensorapi};
    use subxt::{OnlineClient, PolkadotConfig};
    use subxt_signer::sr25519::dev;
    const LOCAL_RPC_URL: &str = "http://localhost:9944";
    const LOCAL_WS_URL: &str = "ws://localhost:9944";

    #[allow(dead_code)]
    async fn disable_whitelist() -> Result<(), anyhow::Error> {
        // Connect to local node
        let client = OnlineClient::<PolkadotConfig>::from_url(LOCAL_WS_URL).await?;

        // Create signer from Alice's dev account
        let signer = dev::alice();

        let inner_call = bittensorapi::runtime_types::pallet_evm::pallet::Call::disable_whitelist {
            disabled: true,
        };

        let runtime_call =
            bittensorapi::runtime_types::node_subtensor_runtime::RuntimeCall::EVM(inner_call);

        let call = bittensorapi::tx().sudo().sudo(runtime_call);

        client
            .tx()
            .sign_and_submit_then_watch_default(&call, &signer)
            .await?;

        let storage_query = bittensorapi::storage().evm().disable_whitelist_check();

        let result = client
            .storage()
            .at_latest()
            .await?
            .fetch(&storage_query)
            .await?;

        println!("Value: {result:?}");

        Ok(())
    }

    #[tokio::test]
    // to test against local network, must get the metadata for local network
    // ./scripts/generate-metadata.sh local
    // export BITTENSOR_NETWORK=local
    #[ignore]
    async fn test_collateral_deploy() {
        const LOCAL_CHAIN_ID: u64 = 42;
        disable_whitelist().await.unwrap();

        // get sudo alice signer
        let alithe_private_key = "5fb92d6e98884f76de468fa3f6278f8807c48bebc13595d45af5bdc4da702133";
        let mut signer: PrivateKeySigner = alithe_private_key.parse().unwrap();
        signer.set_chain_id(Some(LOCAL_CHAIN_ID));

        let provider = ProviderBuilder::new()
            .wallet(signer.clone())
            .connect(LOCAL_RPC_URL)
            .await
            .unwrap();

        let netuid = 1;
        let trustee = signer.address();
        let min_collateral_increase = U256::from(1_000_000_000_000_000_000u128); // 1 TAO
        let decision_timeout = 3600u64; // 1 hour

        let contract = Collateral::deploy(
            &provider,
            netuid,
            trustee,
            min_collateral_increase,
            decision_timeout,
        )
        .await
        .unwrap();

        println!("Deployed contract at: {:?}", contract.address());

        // Test deposit
        let hotkey = [1u8; 32];
        let executor_id = 1u128;
        let amount = U256::from(2_000_000_000_000_000_000u128); // 2 TAO

        let tx = contract
            .deposit(
                FixedBytes::from_slice(&hotkey),
                FixedBytes::from_slice(&executor_id.to_be_bytes()),
            )
            .value(amount);
        let tx = tx.send().await.unwrap();
        let receipt = tx.get_receipt().await.unwrap();
        println!("Deposit receipt: {:?}", receipt);

        // Test get methods
        let netuid_result = contract.NETUID().call().await.unwrap();
        assert_eq!(netuid_result, netuid);

        let trustee_result = contract.TRUSTEE().call().await.unwrap();
        assert_eq!(trustee_result, trustee);

        let min_collateral_increase_result =
            contract.MIN_COLLATERAL_INCREASE().call().await.unwrap();
        assert_eq!(min_collateral_increase_result, min_collateral_increase);

        let decision_timeout_result = contract.DECISION_TIMEOUT().call().await.unwrap();
        assert_eq!(decision_timeout_result, decision_timeout);

        let executor_to_miner_result = contract
            .executorToMiner(
                FixedBytes::from_slice(&hotkey),
                FixedBytes::from_slice(&executor_id.to_be_bytes()),
            )
            .call()
            .await
            .unwrap();
        assert_eq!(executor_to_miner_result, signer.address());

        let collaterals_result = contract
            .collaterals(
                FixedBytes::from_slice(&hotkey),
                FixedBytes::from_slice(&executor_id.to_be_bytes()),
            )
            .call()
            .await
            .unwrap();
        assert_eq!(collaterals_result, amount);
    }
}
