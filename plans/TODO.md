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
