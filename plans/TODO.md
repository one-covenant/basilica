# Exo planning tracker

- [x] Investigate upstream Exo, the Runta PDF, and Basilica deployment/backend capabilities.
- [x] Inspect and incorporate the sibling `basilica-site/` frontend checkout.
- [x] Consolidate the three documents into one self-contained multi-agent plan.
- [x] Assign exclusive ownership, shared contracts, and a capacity-aware parallel schedule.
- [x] Independently review parallel-work boundaries and resolve the recovery handoff gap.
- [x] Move planning work to `docs/exo-implementation-plan` in a dedicated worktree created from `origin/main`; preserve the original branch and its unrelated changes.
- [ ] Complete implementation gates G0–G4; status and evidence live only in the [unified plan](EXO-IMPLEMENTATION-PLAN.md).

This file tracks planning; it is not an additional specification. No application implementation or paid resources were created by the planning work.

The documentation branch also carries the existing implementation branch's
security dependency fix so its required CI checks use the patched TLS graph.

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
- [x] Verify the complete protected-delivery path with real physical generation/lease fencing.

## Protected host execution increment

- [x] Implement durable root-owned host fencing and exact protected input replay.
- [x] Implement strict container identity, stop/absence and isolated startup policy; verify owned store/Engine-fault/packet cases.
- [x] Verify the owned root-helper/real-Engine runtime startup, explicit restart, identity rotation and replacement path; hosted acceptance remains separate.
- [x] Deliver through pinned SSH under current lifecycle authority; verify owned failure/retry cases.
- [x] Publish the host-control component evidence and remaining hosted/controller requirements.
- [ ] Verify complete issuance/SSH/root-helper execution and hosted acceptance before launch.

## Protected runtime bundle increment

- [x] Persist one immutable, context-bound encrypted bundle per operation/generation atomically with runtime and chat grants.
- [x] Derive resource/model identity from owned current state, retain exact input on lease takeover and reject revoked/expired grants without silent replacement.
- [x] Verify concurrency, failed writes, stale authority, tampering and real host input-parser compatibility on owned fixtures; complete request/response digest interoperability remains separate.
- [x] Publish bundle/issuance evidence.
- [x] Resolve the current-main schema conflict and verify the combined tree through exact-head full CI.
- [ ] Finish real host execution and controller integration using the verified pinned delivery/wire contracts.


## Protected host wire increment

- [x] Bind version 2 apply request digests to exact retained bytes and resources from the verified owned offering.
- [x] Verify real Rust/Python digest and price-bit interoperability, strict acknowledgements and retained protocol fences.
- [x] Verify Linux root/runtime-UID storage for both versions and preserve packet isolation.
- [x] Finish exact-head Clippy and owned database replay validation; publish the final evidence.
- [x] Finish trusted image installation and authority-checked cleanup delivery using the verified pinned apply contract.

## Trusted host helper increment

- [x] Embed exact bounded helper/installer source and fixed isolated install/run commands.
- [x] Verify atomic root-owned publication, exact replay, interrupted staging, tamper rejection and actual Linux sudo/stdin/runtime-UID separation.
- [x] Finish exact-head CI and publish the installer evidence.
- [x] Integrate pinned SSH with current lease/grant/target checks and bounded protected transport.
- [ ] Complete immutable runtime image installation, real Engine execution and the remaining controller/G0–G4 gates.

## Protected SSH delivery increment

- [x] Add retained-only active worker snapshots with exact owned host identity and fresh database timing.
- [x] Deliver fixed install/run commands over bounded pinned SSH, preserving stdin and cancellation cleanup.
- [x] Verify real loopback transport and owned database authority/failure/replay boundaries.
- [x] Publish verification, including the mandatory SSH-log exclusion; keep controller launch and remaining physical/hosted gates closed.

## Protected host retirement increment

- [x] Add a separate retained-cleanup authority snapshot without live runtime grants or model credentials.
- [x] Deliver strict retirement requests over the shared pinned transport and verify terminal acknowledgements.
- [x] Verify stale authority, identity drift, revoked grants, takeover/replay and actual host retirement fencing on owned fixtures.
- [x] Publish validation while keeping provider absence, billing settlement and remaining controller/hosted gates separate.

## Immutable runtime image installation increment

- [x] Add bounded digest-only Engine image acquisition under current host authority, with exact post-pull inspection and no retirement dependency.
- [x] Verify cached/missing/error/timeout image paths, fencing order and embedded artifact limits on owned fixtures.
- [x] Exercise image acquisition and complete privileged helper execution against an owned real Engine.
- [x] Publish validation and remaining controller/hosted gates.

## Complete owned protected-delivery integration

- [x] Connect actual retained bundle issuance and database authority to pinned OpenSSH, the root helper and a real Engine on an owned isolated host.
- [x] Verify exact replay, worker takeover, stale authority, physical replacement and terminal retirement with retained state.
- [x] Require the complete path in CI and publish its evidence without claiming hosted/model readiness.

## Passive chat readiness increment

- [x] Add a controller-only observation of the exact existing runtime chat session under current operation/generation authority.
- [x] Verify that observation never replaces or renews sockets, grants or worker leases, including expiry/revocation and actual transport cases.
- [x] Publish exact-head validation and CI evidence.
- [x] Integrate passive observation into the remaining controller readiness work.

## Frontend managed-agent product increment

- [x] Build authenticated instance list, quoted Exo creation and inline saved model connections in the existing shell.
- [x] Build the persistent workspace, scoped text chat and capability-controlled lifecycle management.
- [x] Verify local contracts, replay/reconnect, accessible responsive states and preserved navigation; publish production-build evidence separately from hosted acceptance.
- [x] Publish exact-head frontend CI and preview-build status.
- [x] Add model-key rotation UI with exact non-secret retry identity and owned browser verification.
- [ ] Complete artifact download and hosted rotation/web/CLI/OpenClaw acceptance.

## Lifecycle execution ownership

- [x] Match the complete operation identity during heartbeat renewal, with fresh expiry after lock waits and before commit.
- [x] Own reconciliation futures inside a bounded heartbeat/cancellation runner; cancel work on authority loss, database uncertainty, shutdown or deadline.
- [x] Verify real database deletion/takeover/lock-wait cases and cancellation/drop behavior without live provider calls.
- [ ] Integrate the runner with complete launch/maintenance/cleanup coordination and runtime/model readiness; keep hosted gates open.
- [x] Reconcile cold-image work with fixed authority deadlines through frozen pre-delivery cache fill; verify actual protected delivery under its unchanged short deadline.

## Explicit delete coordinator

- [x] Coordinate claimed delete intent through allocation reconciliation, direct provider absence, retained billing settlement and terminal archival under renewed lifecycle authority.
- [x] Keep never-dispatched deletion local, preserve uncertain purchases, scope billing delivery to the target instance, and resume interruption without another purchase or changed settlement boundary.
- [x] Verify the coordinator with owned PostgreSQL/provider/billing fixtures, including stale authority, lost replies, insufficient credit and terminal-write interruption.
- [ ] Wire the completed coordinator into the service worker alongside launch/maintenance/readiness and the remaining preservation policy.


## Managed runtime readiness

- [x] Probe the actual owned scheduler and adapter processes with fresh challenges and bounded typed state validation.
- [x] Expose observations through the trusted supervisor; invalidate evidence on replacement, exit, drain or probe failure.
- [x] Verify real compiled runners, stale/replayed responses, incompatible state, cancellation and preserved canonical data.
- [x] Integrate trusted host observations, model access and passive chat into the guarded controller Ready transition.


## Trusted host runtime observation

- [x] Observe only the exact existing journal/container under current protected delivery authority, without replaying apply or changing runtime state.
- [x] Bound fixed unprivileged Engine exec output and verify exit, container incarnation, declared schema and complete response identity.
- [x] Verify wire/SSH authority, lost replies, malformed frames, drift and actual owned Engine execution.
- [x] Compose host observation with model/chat/billing checks and an atomic Ready update.

## Guarded controller readiness

- [x] Compose retained host authority, fresh model metadata access, exact passive chat presence and recent billing coverage.
- [x] Complete Ready atomically with final locked evidence, frozen freshness deadlines and complete attempt fencing; keep unsupported maintenance/recovery capabilities disabled.
- [x] Verify actual metadata transport and owned PostgreSQL/SSH races, cancellation, expiry, credential rotation and write rollback.
- [ ] Integrate this transition with complete service dispatch, verified checkpoints and remaining maintenance/launch gates.

## Create/launch coordinator

- [x] Compose stable allocation, fresh funding, retained bootstrap, billing coverage, protected apply, Starting and guarded Ready under one owned reconciliation future.
- [x] Preserve attempt/grant/resource identity across pending work and uncertainty, expose safe waiting status, and stop on deletion or lost authority.
- [x] Verify the complete launch sequence and interruption/replay/authority cases with owned database, provider/billing and host/metadata fixtures.
- [ ] Connect service dispatch, verified artifact preservation and remaining maintenance/hosted gates before enabling launch.

## Cold images under short runtime authority

- [x] Reproduce cold image failure under the actual lease/billing deadline and reject longer stale authority as a workaround.
- [x] Freeze an anonymous bounded image-cache command into new versioned host bootstraps, retaining exact legacy payload compatibility.
- [x] Verify the generated command against an empty owned Engine, followed by current-authority protected delivery and preserved-state takeover/retirement.
- [x] Publish final database, physical runtime and exact-head CI evidence; keep hosted cloud-init and remaining launch gates open.

## Documentation security checks

- [x] Identify the public repository's existing Rustls advisory in documentation CI.
- [x] Reuse the implementation branch's security fix without changing toolchain files or backend dependency selection.
- [x] Verify the patched dependency graph, CLI/SDK compilation and miner Dockerfile locally.
- Track required remote security and build validation in [PR 570 checks](https://github.com/one-covenant/basilica/pull/570/checks) and its exact-head evidence in the PR description.

## Restart coordinator

- [x] Compose an owned restart through current billing, retained runtime replacement and guarded Ready on the existing host.
- [x] Preserve retry identity and use the existing state-preserving delivery path; reject conflicting resources, expired authority and deletion without purchasing replacement compute.
- [x] Add per-attempt protected apply receipts (API migration 045), so Starting takeover re-establishes its physical fence before readiness.
- [x] Verify restart, lost acknowledgements, pending readiness and stale-worker cases with owned fixtures; record remaining maintenance and service-dispatch gates.
- [x] Complete owned physical new-generation restart with explicit journaled credential rotation.
- [ ] Integrate the service worker and hosted acceptance before enabling the public restart capability.

## Physical restart acceptance

- [x] Extend the owned host fixture through actual restart coordination, new-generation access rotation, preserved user state and guarded readiness.
- [x] Verify stale access, exact old-container removal, replay stability and terminal retirement after restart.
- [x] Record local physical/runtime evidence, retaining the separate hosted/model/chat/service-dispatch gates.
- Required exact-head CI and final remote evidence are tracked in [backend PR 1872](https://github.com/one-covenant/basilica-backend/pull/1872) and [plan PR 570](https://github.com/one-covenant/basilica/pull/570).
