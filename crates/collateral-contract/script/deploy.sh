export NETUID=39
export TRUSTEE_ADDRESS=0xf24FF3a9CF04c71Dbc94D0b566f7A27B94566cac
export MIN_COLLATERAL=1
export DECISION_TIMEOUT=1
export ADMIN_ADDRESS=0xf24FF3a9CF04c71Dbc94D0b566f7A27B94566cac
export PRIVATE_KEY=0x5fb92d6e98884f76de468fa3f6278f8807c48bebc13595d45af5bdc4da702133

forge script script/DeployUpgradeable.s.sol \
 --rpc-url http://localhost:9944 \
 --private-key $PRIVATE_KEY \
 --broadcast


