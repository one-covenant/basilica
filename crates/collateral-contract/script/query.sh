# query
export CONTRACT_ADDRESS=0x970951a12F975E6762482ACA81E57D5A2A4e73F4

collateral-cli --network local --contract-address $CONTRACT_ADDRESS query trustee
collateral-cli --network local --contract-address $CONTRACT_ADDRESS query min-collateral-increase
collateral-cli --network local --contract-address $CONTRACT_ADDRESS query decision-timeout
collateral-cli --network local --contract-address $CONTRACT_ADDRESS query netuid


