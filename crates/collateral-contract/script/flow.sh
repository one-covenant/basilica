# the whole collateral flow to verify everything
export NETWORK=local
export CONTRACT_ADDRESS=0x970951a12F975E6762482ACA81E57D5A2A4e73F4
export HOTKEY=0x0000000000000000000000000000000000000000000000000000000000000001
export EXECUTOR_ID=6339ba4f-60f9-45c2-9d95-2b755bb57ca6
export PRIVATE_KEY=0x0000000000000000000000000000000000000000000000000000000000000000
# deposit
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS tx deposit \
--private-key $PRIVATE_KEY \
--hotkey $HOTKEY \
--amount 10 \
--executor-id $EXECUTOR_ID

# check the executor to miner, miner is not zero if deposit is successful
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS query \
executor-to-miner --hotkey $HOTKEY \
--executor-id $EXECUTOR_ID

# check the collaterals should be 10
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS query collaterals \
--hotkey $HOTKEY \
--executor-id $EXECUTOR_ID

# reclaim collateral
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS tx reclaim-collateral \
--private-key $PRIVATE_KEY \
--hotkey $HOTKEY \
--executor-id $EXECUTOR_ID \
--url https://www.tplr.ai/ \
--url-content-md5-checksum 269ff519d1140a175941ea4b00ccbe0d

# check the reclaims should include the content
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS query reclaims \
--reclaim-request-id 0 

# finalize the reclaim
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS tx finalize-reclaim \
--private-key $PRIVATE_KEY \
--reclaim-request-id 0

# deny the reclaim
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS tx deny-reclaim \
--private-key $PRIVATE_KEY \
--reclaim-request-id 0 \
--url https://www.tplr.ai/ \
--url-content-md5-checksum 269ff519d1140a175941ea4b00ccbe0d

# check the reclaims should be deleted after finalize, all items are 0
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS query reclaims \
--reclaim-request-id 0 

# check the collaterals should be 0
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS query collaterals \
--hotkey $HOTKEY \
--executor-id $EXECUTOR_ID

# slash the collateral
collateral-cli --network $NETWORK --contract-address $CONTRACT_ADDRESS tx slash-collateral \
--private-key $PRIVATE_KEY \
--hotkey $HOTKEY \
--executor-id $EXECUTOR_ID \
--url https://www.tplr.ai/ \
--url-content-md5-checksum 269ff519d1140a175941ea4b00ccbe0d

