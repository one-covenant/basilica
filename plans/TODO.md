# Exo planning tracker

- [x] Investigate upstream Exo, the Runta PDF, and Basilica deployment/backend capabilities.
- [x] Inspect and incorporate the sibling `basilica-site/` frontend checkout.
- [x] Consolidate the three documents into one self-contained multi-agent plan.
- [x] Assign exclusive ownership, shared contracts, and a capacity-aware parallel schedule.
- [x] Independently review parallel-work boundaries and resolve the recovery handoff gap.
- [x] Move planning work to `docs/exo-implementation-plan` in a dedicated worktree created from `origin/main`; preserve the original branch and its unrelated changes.
- [ ] Complete implementation gates G0–G4; status and evidence live only in the [unified plan](EXO-IMPLEMENTATION-PLAN.md).

This file tracks planning; it is not an additional specification. No application implementation or paid resources were created by the planning work.

## Active implementation increment

- [x] Add explicit managed-agent CLI routing, quoted launch, operations and safe model-connection input.
- [x] Verify parsing, resource isolation, replay inputs, terminal outcomes and redaction; run CLI/SDK checks.
- [x] Record tested CLI scope and remaining integration requirements in the unified plan.

## Chat service increment

- [x] Implement scoped sessions/runtime pairing and durable, fenced text delivery.
- [x] Wire first-frame authenticated WebSockets, owner session issuance, configuration and OpenAPI.
- [x] Verify PostgreSQL concurrency/revocation/reconnect and actual local WebSocket behavior; record limits and CI.

## Managed runtime chat adapter increment

- [x] Implement strict scoped transport, durable inbound identity, bounded reconnect/renewal and persisted outbound acknowledgements.
- [x] Add protected chat input and repeat-safe canonical adapter registration; preserve edited/deleted state.
- [x] Verify worker, actual compiled setup, local transport integration and pinned patch/image checks; update evidence.

## Managed catalog and quote increment

- [x] Record the FUSE incompatibility and CPU-VM fallback contract without weakening runtime persistence checks.
- [x] Implement approved offers/catalog and owner-scoped repeat-safe quotes using existing CPU pricing and balance checks.
- [x] Verify quote expiry, catalog changes, ownership, retries, wire contracts and fresh offering checks; record remaining allocation work.

## Durable CPU allocation increment

- [x] Add stable allocation preparation and durable single-dispatch tracking in the existing aggregator/rental database; migration 039 reserved by CO.
- [x] Reconcile uncertain provider responses without buying a second machine; verify ownership, concurrency, interruption and persistence failure against owned PostgreSQL/provider fixtures.
- [x] Record the allocator contract, integration limits and validation.
- [ ] Connect allocation to the lifecycle controller with billing, protected host/runtime delivery, fencing and verified cleanup.

## Allocation authority increment

- [x] Require transaction-scoped launch authority for CPU preparation and submission.
- [x] Bind the stable rental to the owned instance and recheck lease, generation, quote and connection before commit.
- [x] Verify stale/deleted workers, lock waits and rollback; record billing registration gaps and remaining controller work.
- [x] Implement atomic managed secure-rental billing registration and preserve original dispatch time, with immutable retry receipts and versioned client acknowledgment.
- [x] Align catalog/allocation admission with marked-up billing rate precision and the stored markup range before provider dispatch.
- [x] Connect durable managed metering from dispatch through verified cleanup to existing credit operations, and make finalization atomic/retry-safe before accepting settlement.


## Durable managed billing increment

- [x] Add atomic cumulative metering and retained settlement identity over existing rental/credit ledgers.
- [x] Fence generic billing writers and verify concurrency, interruption, drift, retention and ordinary compatibility on owned PostgreSQL.
- [x] Wire a versioned internal RPC/client and lifecycle metering/cleanup.
- [ ] Resolve insufficient-credit retention/export and unpaid settlement policy before launch.


## Managed metering transport increment

- [x] Add the backend-private versioned RPC, production billing server registration and strict shared-client acknowledgment checks.
- [x] Verify actual RPC/storage behavior, lost acknowledgment/restart, malformed input/response and bounded single-attempt transport.
- [x] Record private routing assumptions and add durable lifecycle billing delivery/reconciliation functions; background controller integration remains below.


## Lifecycle billing delivery increment

- [x] Freeze registration from observed allocation/accepted quote and persist exact pending calls under a separate billing lease.
- [x] Preserve cleanup time independently of payment and require retained billing coverage/settlement for readiness/cleanup.
- [x] Verify delivery across lease expiry, lost acknowledgment, completed launch, pending ticks and funding; publish evidence and rollout contract.
- [ ] Integrate the background lifecycle controller, trusted provider absence proof and retention/export policy before paid launch.

## Provider cleanup integration increment

- [x] Add strict provider observation and termination, with immutable cleanup intent and lifecycle authority.
- [x] Connect verified absence to the frozen billing boundary; retain pending cleanup on uncertainty and preserve ordinary rental behavior.
- [x] Verify loopback provider behavior, database concurrency/lease/generation boundaries and billing ordering; publish evidence and rollout contract.

- [ ] Complete controller coordination, including protected host/runtime delivery and state-preservation policy.

## Settled rental archival increment

- [x] Atomically archive the owned managed rental with verified cleanup and retain allocation/billing tombstones.
- [x] Verify settled and never-dispatched cleanup, identity conflicts, retries and rollback under expired authority.
- [x] Record validation and remaining controller work.

## Fresh purchase balance increment

- [x] Require a distinct balance-checked guard for provider submission while keeping preparation/reconciliation independent.
- [x] Verify actual billing admission, expired evidence, stale authority, term changes and no purchase on failure.
- [x] Publish validation and remaining controller requirements.

- [x] Add durable encrypted per-allocation platform SSH keys and isolated provider registration.
- [x] Implement retained server bootstrap, protected single-create payload and pinned host authentication.
- [x] Finish bootstrap validation and resolve the generated-schema conflict with current main.
- [x] Finish post-merge integration validation and repair the transient CI dependency-download failure.
- [ ] Deliver protected runtime inputs with physical generation/lease fencing.

## Protected host execution increment

- [x] Implement durable root-owned host fencing and exact protected input replay.
- [x] Implement strict container identity, stop/absence and isolated startup policy; verify owned store/Engine-fault/packet cases.
- [ ] Verify the complete root-helper/real-Engine runtime startup, restart, identity rotation and replacement path.
- [ ] Deliver through pinned SSH under current lifecycle authority; verify owned failure/retry cases.
- [x] Publish the host-control component evidence and remaining hosted/controller requirements.
- [ ] Finish protected issuance/SSH integration evidence and hosted acceptance before launch.
