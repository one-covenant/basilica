# Managed Exo CLI

The Exo integration is under development on `feat/exo`. These commands speak the
managed-agent API; their presence does not enable Exo on a server. Catalog,
quotes, runtime reconciliation, authenticated chat and hosted acceptance must be
available before the launch flow below can run end to end. Local protocol tests
are not evidence of a live deployment. No default size or price is hard-coded.

Use `basilica login` (or `basilica login --device-code` remotely) and an API server
with the managed-agent services enabled. `basilica agents templates` shows the
server's tested models, recommended size, regions and persistence contract.

## Connect a model

Create a reusable owner-scoped connection. Omit `--key-env` in an interactive
terminal to enter the provider key without echoing it. For automation, have your
secret manager set an environment variable and pass its **name**, never its value:

```bash
basilica agents connections create \
  --name personal --provider openai --model "$TESTED_MODEL" \
  --key-env EXO_PROVIDER_KEY --idempotency-key connect-personal-0001 --json
basilica agents connections ls --json
```

The response contains safe metadata and a connection ID, never the provider key.
Keys are sent to the API over HTTPS and are not saved in CLI config. Rotate a key
with `agents connections rotate CONNECTION_ID --key-env VARIABLE
--idempotency-key NEW_KEY`; active connections cannot be deleted. Preserve a
mutation's key and inputs when retrying an uncertain request. Use a new key for a
new intent, including a later credential rotation.

## Review and launch

```bash
basilica agents quote --connection "$CONNECTION_ID" \
  --region "$REGION" --idempotency-key research-quote-0001 --json
```

`--size` overrides the server's recommended size. When the selected size offers
one region, `--region` may be omitted. The quote separates hourly compute and
storage from model usage billed by the connected provider. Review the quote's
expiry and persistence description, preserved paths, limitations and deletion
policy. The supported lifetime is `until-deleted`; billing continues until
confirmed cleanup. There is no pause, automatic trial expiry or memory restore.

Launch using the reviewed quote ID:

```bash
basilica summon exo --name research --connection "$CONNECTION_ID" \
  --quote-id "$QUOTE_ID" --idempotency-key research-launch-0001 \
  --yes --detach --json
```

Without `--quote-id`, the command obtains and displays a fresh quote and asks for
confirmation. `--yes` accepts those terms without prompting; JSON/noninteractive
runs require it. `--quote-id` requires `--yes` because its terms were reviewed in
the earlier quote response. An expired new launch fails; the CLI never silently
replaces a submitted quote. `--quote-id` cannot be combined with `--size` or
`--region`.

Before submitting launch intent, the command writes its exact non-secret request
and idempotency key to stderr. Preserve these replay inputs. If the response is
lost, repeat with **the same name, connection, quote ID and idempotency key**;
using the quote ID avoids requesting a new price. Server-side replay works even
if that quote has since expired. Do not rerun an uncertain launch without its
original quote ID. No client-side automatic mutation retries are performed.

`--detach` returns the instance ID, operation ID and status URL once work is
accepted, not once the agent is ready. Without it the command waits up to
`--timeout` seconds (default 600), returning the terminal operation. Timeout,
network failure or closing the terminal does not cancel backend work. Inspect
the recorded operation instead of creating another agent. Add `--show-phases` to
print phase, operation state and cleanup/billing changes to stderr while waiting.

## Return and manage

```bash
basilica agents ls --json
basilica agents status "id:$INSTANCE_ID" --json
basilica agents status name:research --json
basilica agents operation "$OPERATION_ID" --json
basilica agents logs "id:$INSTANCE_ID" --limit 100 --json
basilica agents restart "id:$INSTANCE_ID" --idempotency-key research-restart-0001
basilica agents export "id:$INSTANCE_ID" --idempotency-key research-export-0001
basilica agents recover "id:$INSTANCE_ID" --checkpoint "$CHECKPOINT_ID" \
  --idempotency-key research-recover-0001
basilica agents delete "id:$INSTANCE_ID" --idempotency-key research-delete-0001 --yes
```

Status reports independent runtime/model/chat health and server capabilities.
The server rejects unavailable operations and incompatible recovery checkpoints.
Recovery replaces code while retaining canonical state; it is accessible without
chat. Export requests a consistent archive and reports its operation metadata;
artifact download and retention integration are still pending. Browser opening
and chat access are not implemented by this CLI increment: no access tokens or
upstream relay URLs are printed in general output.

Lists and logs return `next_cursor`; pass `--cursor` to continue. Name resolution
searches every managed-agent page and fails on ambiguous names. A bare UUID is
an ID; `name:UUID` explicitly selects a UUID-shaped name. Use IDs for mutations
and retries, especially after deletion (deleted records remain available by ID
but are omitted from lists). Operation results are historical; their phase describes the agent now, so a
successful old launch can show a later deleting/deleted phase. Operation failures
exit nonzero; a deletion is
successful only after the server reports deleted state, no pending cleanup and
finalized billing. Cleanup failure may mean continuing costs.

`agents` never searches rentals or deployments. Existing `summon ls/status/logs/
restart/delete` still address deployments, including OpenClaw; `ps/status/down`
still address rentals. Names may overlap across those resource kinds without
redirecting a command. `exo` is now a reserved `summon` template name; use a
qualified image name such as `registry.example/exo:tag` for a generic image.
Exo options belong after `exo`; GPU, replica, image and generic storage settings
do not apply. Unsupported pause/resume/terminal controls are absent.
