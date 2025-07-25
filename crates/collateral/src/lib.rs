use alloy_primitives::address;
use alloy_primitives::{Address, FixedBytes, U256};
use alloy_provider::ProviderBuilder;
use alloy_sol_types::sol;

use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;

sol!(
    #[allow(missing_docs)]
    #[sol(
        rpc,
        bytecode = "608060405234801561000f575f5ffd5b50604051611e44380380611e44833981810160405281019061003191906102c5565b5f73ffffffffffffffffffffffffffffffffffffffff168373ffffffffffffffffffffffffffffffffffffffff160361009f576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161009690610383565b60405180910390fd5b5f82116100e1576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016100d890610411565b60405180910390fd5b5f8167ffffffffffffffff161161012d576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016101249061049f565b60405180910390fd5b835f5f6101000a81548161ffff021916908361ffff160217905550825f60026101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff16021790555081600181905550805f60166101000a81548167ffffffffffffffff021916908367ffffffffffffffff160217905550505050506104bd565b5f5ffd5b5f61ffff82169050919050565b6101da816101c4565b81146101e4575f5ffd5b50565b5f815190506101f5816101d1565b92915050565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f610224826101fb565b9050919050565b6102348161021a565b811461023e575f5ffd5b50565b5f8151905061024f8161022b565b92915050565b5f819050919050565b61026781610255565b8114610271575f5ffd5b50565b5f815190506102828161025e565b92915050565b5f67ffffffffffffffff82169050919050565b6102a481610288565b81146102ae575f5ffd5b50565b5f815190506102bf8161029b565b92915050565b5f5f5f5f608085870312156102dd576102dc6101c0565b5b5f6102ea878288016101e7565b94505060206102fb87828801610241565b935050604061030c87828801610274565b925050606061031d878288016102b1565b91505092959194509250565b5f82825260208201905092915050565b7f547275737465652061646472657373206d757374206265206e6f6e2d7a65726f5f82015250565b5f61036d602083610329565b915061037882610339565b602082019050919050565b5f6020820190508181035f83015261039a81610361565b9050919050565b7f4d696e20636f6c6c61746572616c20696e637265617365206d757374206265205f8201527f67726561746572207468616e2030000000000000000000000000000000000000602082015250565b5f6103fb602e83610329565b9150610406826103a1565b604082019050919050565b5f6020820190508181035f830152610428816103ef565b9050919050565b7f4465636973696f6e2074696d656f7574206d75737420626520677265617465725f8201527f207468616e203000000000000000000000000000000000000000000000000000602082015250565b5f610489602783610329565b91506104948261042f565b604082019050919050565b5f6020820190508181035f8301526104b68161047d565b9050919050565b61197a806104ca5f395ff3fe6080604052600436106100aa575f3560e01c80639cf96318116100635780639cf96318146101ff578063b1f50e5b1461023e578063b4314e2b14610266578063f3f636081461028e578063f44e1119146102b6578063fdda13a1146102f2576100e1565b806306016f711461011357806307d867881461013d5780634a7393b21461016557806369fc2c341461018f578063881cf23b146101ab57806396c42a0a146101d5576100e1565b366100e1576040517f84ee6c0a00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b6040517f84ee6c0a00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b34801561011e575f5ffd5b5061012761032e565b604051610134919061138b565b60405180910390f35b348015610148575f5ffd5b50610163600480360381019061015e91906113df565b610340565b005b348015610170575f5ffd5b50610179610733565b6040516101869190611419565b60405180910390f35b6101a960048036038101906101a49190611487565b610739565b005b3480156101b6575f5ffd5b506101bf610995565b6040516101cc91906114f1565b60405180910390f35b3480156101e0575f5ffd5b506101e96109ba565b6040516101f6919061152c565b60405180910390f35b34801561020a575f5ffd5b50610225600480360381019061022091906113df565b6109d3565b6040516102359493929190611554565b60405180910390f35b348015610249575f5ffd5b50610264600480360381019061025f91906115f8565b610a3d565b005b348015610271575f5ffd5b5061028c60048036038101906102879190611669565b610dc4565b005b348015610299575f5ffd5b506102b460048036038101906102af91906115f8565b611015565b005b3480156102c1575f5ffd5b506102dc60048036038101906102d79190611487565b61132a565b6040516102e991906114f1565b60405180910390f35b3480156102fd575f5ffd5b5061031860048036038101906103139190611487565b61135a565b6040516103259190611419565b60405180910390f35b5f5f9054906101000a900461ffff1681565b5f60045f8381526020019081526020015f2090505f816002015403610391576040517f642e3ad700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b42816003015f9054906101000a900467ffffffffffffffff1667ffffffffffffffff16106103eb576040517f3355482c00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f815f015f9054906101000a900460801b90505f826001015f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690505f8360020154905060045f8681526020019081526020015f205f5f82015f6101000a8154906fffffffffffffffffffffffffffffffff0219169055600182015f6101000a81549073ffffffffffffffffffffffffffffffffffffffff0219169055600282015f9055600382015f6101000a81549067ffffffffffffffff021916905550508060055f856fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f8282546104ef9190611707565b925050819055508060035f856fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f20541015610567576040517fc4d7ebda00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b8060035f856fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f8282546105ad9190611707565b925050819055508173ffffffffffffffffffffffffffffffffffffffff16836fffffffffffffffffffffffffffffffff1916867fb3786058892dfb3d1a6df4822684e2c56509d4cbc7a8a2d9a6887565ec75e85a8460405161060f9190611419565b60405180910390a45f8273ffffffffffffffffffffffffffffffffffffffff168260405161063c90611767565b5f6040518083038185875af1925050503d805f8114610676576040519150601f19603f3d011682016040523d82523d5f602084013e61067b565b606091505b50509050806106b6576040517f90b8ec1800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60025f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff160217905550505050505050565b60015481565b600154341015610775576040517f5945ea5600000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60025f836fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690505f73ffffffffffffffffffffffffffffffffffffffff168173ffffffffffffffffffffffffffffffffffffffff160361087c573360025f846fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff1602179055506108e2565b3373ffffffffffffffffffffffffffffffffffffffff168173ffffffffffffffffffffffffffffffffffffffff16146108e1576040517f9ea26eb800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5b3460035f846fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f828254610928919061177b565b925050819055503373ffffffffffffffffffffffffffffffffffffffff16826fffffffffffffffffffffffffffffffff19167faf57399cabdfa7a1e4f15be945b1d2e8a56b0c649947b8de206d361c53389bfb346040516109899190611419565b60405180910390a35050565b5f60029054906101000a900473ffffffffffffffffffffffffffffffffffffffff1681565b5f60169054906101000a900467ffffffffffffffff1681565b6004602052805f5260405f205f91509050805f015f9054906101000a900460801b90806001015f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690806002015490806003015f9054906101000a900467ffffffffffffffff16905084565b3373ffffffffffffffffffffffffffffffffffffffff1660025f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1614610af8576040517f9ea26eb800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60035f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205490505f60055f876fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205490505f8183610b7b9190611707565b90505f8103610bb6576040517fcbca5aa200000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f5f60169054906101000a900467ffffffffffffffff1642610bd891906117ae565b90506040518060800160405280896fffffffffffffffffffffffffffffffff191681526020013373ffffffffffffffffffffffffffffffffffffffff1681526020018381526020018267ffffffffffffffff1681525060045f60065f8154610c3f906117e9565b91905081905581526020019081526020015f205f820151815f015f6101000a8154816fffffffffffffffffffffffffffffffff021916908360801c02179055506020820151816001015f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff160217905550604082015181600201556060820151816003015f6101000a81548167ffffffffffffffff021916908367ffffffffffffffff1602179055509050508160055f8a6fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f828254610d46919061177b565b925050819055503373ffffffffffffffffffffffffffffffffffffffff16886fffffffffffffffffffffffffffffffff19166006547f1df1879f6655222ebb5cda90ece4b31ca842a40b48a7a716c212b934cd36f6f185858c8c8c604051610db295949392919061188a565b60405180910390a45050505050505050565b5f60029054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff1614610e4a576040517f5aa309bb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60045f8681526020019081526020015f2090505f816002015403610e9b576040517f642e3ad700000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b42816003015f9054906101000a900467ffffffffffffffff1667ffffffffffffffff161015610ef6576040517ffc9e5c0200000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b806002015460055f835f015f9054906101000a900460801b6fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f828254610f4f9190611707565b92505081905550847f6067048e78883441f7c7f1b3ad8f94a88b1567a486029be8a830e9455f61edb9858585604051610f8a939291906118d6565b60405180910390a260045f8681526020019081526020015f205f5f82015f6101000a8154906fffffffffffffffffffffffffffffffff0219169055600182015f6101000a81549073ffffffffffffffffffffffffffffffffffffffff0219169055600282015f9055600382015f6101000a81549067ffffffffffffffff021916905550505050505050565b5f60029054906101000a900473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff161461109b576040517f5aa309bb00000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60035f866fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205490505f810361110f576040517fcbca5aa200000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60035f876fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f20819055505f60025f876fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f9054906101000a900473ffffffffffffffffffffffffffffffffffffffff1690505f5f73ffffffffffffffffffffffffffffffffffffffff16836040516111ca90611767565b5f6040518083038185875af1925050503d805f8114611204576040519150601f19603f3d011682016040523d82523d5f602084013e611209565b606091505b5050905080611244576040517f90b8ec1800000000000000000000000000000000000000000000000000000000815260040160405180910390fd5b5f60025f896fffffffffffffffffffffffffffffffff19166fffffffffffffffffffffffffffffffff191681526020019081526020015f205f6101000a81548173ffffffffffffffffffffffffffffffffffffffff021916908373ffffffffffffffffffffffffffffffffffffffff1602179055508173ffffffffffffffffffffffffffffffffffffffff16876fffffffffffffffffffffffffffffffff19167f5f263785028287c710e1befd68d34fada78fe6e93442c1bab80cfb778f731864858989896040516113199493929190611906565b60405180910390a350505050505050565b6002602052805f5260405f205f915054906101000a900473ffffffffffffffffffffffffffffffffffffffff1681565b6003602052805f5260405f205f915090505481565b5f61ffff82169050919050565b6113858161136f565b82525050565b5f60208201905061139e5f83018461137c565b92915050565b5f5ffd5b5f5ffd5b5f819050919050565b6113be816113ac565b81146113c8575f5ffd5b50565b5f813590506113d9816113b5565b92915050565b5f602082840312156113f4576113f36113a4565b5b5f611401848285016113cb565b91505092915050565b611413816113ac565b82525050565b5f60208201905061142c5f83018461140a565b92915050565b5f7fffffffffffffffffffffffffffffffff0000000000000000000000000000000082169050919050565b61146681611432565b8114611470575f5ffd5b50565b5f813590506114818161145d565b92915050565b5f6020828403121561149c5761149b6113a4565b5b5f6114a984828501611473565b91505092915050565b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f6114db826114b2565b9050919050565b6114eb816114d1565b82525050565b5f6020820190506115045f8301846114e2565b92915050565b5f67ffffffffffffffff82169050919050565b6115268161150a565b82525050565b5f60208201905061153f5f83018461151d565b92915050565b61154e81611432565b82525050565b5f6080820190506115675f830187611545565b61157460208301866114e2565b611581604083018561140a565b61158e606083018461151d565b95945050505050565b5f5ffd5b5f5ffd5b5f5ffd5b5f5f83601f8401126115b8576115b7611597565b5b8235905067ffffffffffffffff8111156115d5576115d461159b565b5b6020830191508360018202830111156115f1576115f061159f565b5b9250929050565b5f5f5f5f606085870312156116105761160f6113a4565b5b5f61161d87828801611473565b945050602085013567ffffffffffffffff81111561163e5761163d6113a8565b5b61164a878288016115a3565b9350935050604061165d87828801611473565b91505092959194509250565b5f5f5f5f60608587031215611681576116806113a4565b5b5f61168e878288016113cb565b945050602085013567ffffffffffffffff8111156116af576116ae6113a8565b5b6116bb878288016115a3565b935093505060406116ce87828801611473565b91505092959194509250565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f611711826113ac565b915061171c836113ac565b9250828203905081811115611734576117336116da565b5b92915050565b5f81905092915050565b50565b5f6117525f8361173a565b915061175d82611744565b5f82019050919050565b5f61177182611747565b9150819050919050565b5f611785826113ac565b9150611790836113ac565b92508282019050808211156117a8576117a76116da565b5b92915050565b5f6117b88261150a565b91506117c38361150a565b9250828201905067ffffffffffffffff8111156117e3576117e26116da565b5b92915050565b5f6117f3826113ac565b91507fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff8203611825576118246116da565b5b600182019050919050565b5f82825260208201905092915050565b828183375f83830152505050565b5f601f19601f8301169050919050565b5f6118698385611830565b9350611876838584611840565b61187f8361184e565b840190509392505050565b5f60808201905061189d5f83018861140a565b6118aa602083018761151d565b81810360408301526118bd81858761185e565b90506118cc6060830184611545565b9695505050505050565b5f6040820190508181035f8301526118ef81858761185e565b90506118fe6020830184611545565b949350505050565b5f6060820190506119195f83018761140a565b818103602083015261192c81858761185e565b905061193b6040830184611545565b9594505050505056fea2646970667358221220b0ee484c46f9ab66b412e84ee7b596091a9055332df31be39d0e74fac4ce933764736f6c634300081e0033"
    )]
    Collateral,
    "./src/collateral.json"
);

// Deployed Collateral contract address
const COLLATERAL_ADDRESS: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const CHAIN_ID: u64 = 42;
const RPC_URL: &str = "http://localhost:9944";

#[derive(Debug, Clone)]
pub struct Reclaim {
    pub executor_id: u16,
    pub miner: Address,
    pub amount: U256,
    pub deny_timeout: u64,
}

impl From<(FixedBytes<16>, Address, U256, u64)> for Reclaim {
    fn from(tuple: (FixedBytes<16>, Address, U256, u64)) -> Self {
        Self {
            executor_id: u16::from_be_bytes(tuple.0[0..2].try_into().unwrap()),
            miner: tuple.1,
            amount: tuple.2,
            deny_timeout: tuple.3,
        }
    }
}

// transactions
pub async fn deposit(
    private_key: &str,
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
        .deposit(FixedBytes::from_slice(&executor_bytes))
        .value(amount);
    let tx = tx.send().await?;
    let receipt = tx.get_receipt().await?;
    tracing::info!("{receipt:?}");
    Ok(())
}

pub async fn reclaim_collateral(
    private_key: &str,
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

    let tx = contract.reclaimCollateral(
        FixedBytes::from_slice(&executor_id.to_be_bytes()),
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

    let tx = contract.slashCollateral(
        FixedBytes::from_slice(&executor_id.to_be_bytes()),
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

pub async fn executor_to_miner(executor_id: u128) -> Result<Address, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let executor_to_miner = contract
        .executorToMiner(FixedBytes::from_slice(&executor_id.to_be_bytes()))
        .call()
        .await?;
    Ok(executor_to_miner)
}

pub async fn collaterals(executor_id: u128) -> Result<U256, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let collaterals = contract
        .collaterals(FixedBytes::from_slice(&executor_id.to_be_bytes()))
        .call()
        .await?;
    Ok(collaterals)
}

pub async fn reclaims(reclaim_request_id: U256) -> Result<Reclaim, anyhow::Error> {
    let provider = ProviderBuilder::new().connect(RPC_URL).await?;
    let contract = Collateral::new(COLLATERAL_ADDRESS, &provider);
    let result = contract.reclaims(reclaim_request_id).call().await?;
    let reclaim = Reclaim::from((
        result.executorId,
        result.miner,
        result.amount,
        result.denyTimeout,
    ));
    Ok(reclaim)
}

#[cfg(test)]
mod test {
    use super::*;
    use bittensor::api::api::{self as bittensorapi};
    use subxt::{OnlineClient, PolkadotConfig};
    use subxt_signer::sr25519::dev;

    #[allow(dead_code)]
    async fn disable_whitelist() -> Result<(), anyhow::Error> {
        // Connect to local node
        let client = OnlineClient::<PolkadotConfig>::from_url("ws://127.0.0.1:9944").await?;

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

        println!("Value: {:?}", result);

        Ok(())
    }

    #[tokio::test]
    // to test against local network, must get the metadata for local network
    // ./scripts/generate-metadata.sh local
    // export BITTENSOR_NETWORK=local
    #[ignore]
    async fn test_collateral_deploy() {
        disable_whitelist().await.unwrap();

        // get sudo alice signer
        let alithe_private_key = "5fb92d6e98884f76de468fa3f6278f8807c48bebc13595d45af5bdc4da702133";
        let mut signer: PrivateKeySigner = alithe_private_key.parse().unwrap();
        signer.set_chain_id(Some(CHAIN_ID));

        let provider = ProviderBuilder::new()
            .wallet(signer.clone())
            .connect("http://localhost:9944")
            .await
            .unwrap();

        let netuid = 1;
        let trustee = signer.address();
        let min_collateral_increase = 1;
        let decision_timeout = 1_u64;

        let contract = Collateral::deploy(
            &provider,
            netuid,
            trustee,
            U256::from(min_collateral_increase),
            decision_timeout,
        )
        .await
        .unwrap();

        println!("Contract deployed at: {:?}", contract.address());

        // check all get methods
        contract.TRUSTEE().call().await.unwrap();
        contract.NETUID().call().await.unwrap();
        contract.DECISION_TIMEOUT().call().await.unwrap();
        contract.MIN_COLLATERAL_INCREASE().call().await.unwrap();
    }
}
