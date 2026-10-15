# BYOT beta: the short guide for testers

Bring your own trainer (BYOT): you keep the trainer, Basilica runs a private
vLLM rollout fleet that serves the weights you publish to your own bucket.
This page is the quick version: how to run it, what it costs, what to do when
something goes wrong. [`BYOT-GUIDE.md`](BYOT-GUIDE.md) has the full story.

## Quickstart

You need `python3`, a Basilica **staging** API key, and a Cloudflare R2
bucket with an R2 API token that can write to it (bucket name, endpoint
`https://<account-id>.r2.cloudflarestorage.com`, access key id, secret
access key). The scripts ask for these once and save them in
`~/.basilica-byot-demo/env`, readable only by you.

```bash
# The contract end to end from your laptop (about 45 to 60 minutes, one A100)
curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-demo.sh | bash

# Real GRPO training on a rented trainer GPU (about 35 to 45 minutes, three A100s)
curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-train.sh | bash
```

**Keep the machine awake.** On macOS both scripts hold a `caffeinate`
assertion, but closing the lid still sleeps a laptop on battery: keep it
open and plugged in. A sleeping laptop stalls the demo, and the launcher
loses the trainer's output (the training job itself keeps running on the
rented box and cleans up after itself).

Both scripts clean up on exit, also on Ctrl-C: the session and policy are
deleted and a rented trainer box is stopped. Press Ctrl-C once and let the
cleanup finish.

## What each step does and how long it takes

`byot-demo.sh`:

| Step | What happens | Typical time |
|---|---|---|
| Setup | A private Python environment in `~/.basilica-byot-demo` | 1 to 3 min the first time |
| GPU check | Lists the GPU offerings; stops before creating anything if the requested type is not available | seconds |
| Model download | Qwen2.5-1.5B-Instruct, about 3 GB, into your Hugging Face cache | 2 to 25 min the first time, then skipped |
| 1. Policy | Registers the model lineage against your bucket | seconds |
| 2. Session | Rents a GPU node (5 to 9 min), pulls the serving image (about 3 min), starts vLLM. Progress is printed while it waits | about 10 min |
| 3. Anchor | Uploads the full bf16 weights (3 GB) from your laptop to your bucket; the fleet fetches, digest-checks and loads them | upload 2 to 10 min, load 1 to 7 min |
| 4. Generate | Text and token-id generation, every response stamped with its revision | seconds |
| 5. Patch | A sparse update, Active about a minute after publish; an assertion of the old revision is refused | about 1 min |
| 6. Usage | The platform's metered usage for the session | 1 to 2 min |
| 7. Park and resume | Releases the GPU, then a fresh replica replays the lineage (skip with `BYOT_DEMO_SKIP_PARK=1`) | about 10 to 15 min |
| Cleanup | Prints usage, deletes the session and policy, prints the balance change | seconds |

`byot-train.sh`: GPU check, then it rents a trainer box (1 to 5 min to come
up), installs PyTorch and the SDK there (1 to 2 min), and runs
`byot_grpo.py` on it, which starts a 2-replica session (about 10 min) and
trains 20 GRPO steps (15 to 20 min). The job runs detached on the box and
copies its log to `s3://<your bucket>/byot-runs/<rental>/run.log` every 2
minutes.

## What it costs and how to see it

There are three meters:

- **The rollout session**: GPU hours of its replicas. Read it with
  `session.usage()` (open the session with
  `client.rl.open_session(url, token, session_uid=uid)`) or
  `client.rl.session_usage(uid)`. It is readable only while the session
  exists, so read it **before** you delete the session; every script here
  prints it right before cleanup. `costUsd` appears only where the platform
  has a rate configured; on staging it is absent, so use `gpuHours`.
- **The trainer box** (`byot-train.sh` only): billed by the hour at the
  offering's rate (an A100 is about $1.50 an hour on staging). The cost is
  printed when the box stops, by the launcher or by the job on the box
  (`self-stop: rental stopped after 0.60 h, $0.89`).
- **Your account balance**: `client.get_balance()["balance"]`. The scripts
  print it at the start and the end, with the difference. It is
  **account-wide**: if several testers share one API key (or one account),
  everyone's sessions and boxes land in the same balance, and billing can lag
  a few minutes.

**Your own key.** The Basilica CLI creates API keys for the account you are
logged into: `basilica login`, then `basilica tokens create <name>`
(`basilica tokens list` and `basilica tokens revoke <name>` manage them).
Keys of one account share that account's balance, so for spend you can tell
apart, each tester needs their own account and key. The CLI points at
production by default; during the beta ask the Basilica team for a staging
account and key (the staging login settings are in the internal copy of
this guide). The Python SDK has no call to create keys.

Newer SDKs are adding `client.rl.list_usage()` to read usage after a
session is deleted; until your SDK has it, read usage before the delete.

## Troubleshooting

| You see | What it means | What to do |
|---|---|---|
| `ConnectionError: error sending request ... dns error` (or another connection error) | A network blip between you and the API | Nothing: the scripts retry, and SDKs after 0.36.7 retry reads and deletes for up to 2 minutes (`BASILICA_RETRY_BUDGET_S`). If it keeps failing, check your connection; the cleanup prints the exact command for anything it could not delete |
| `ConnectionClosedError` or `InvalidPart` while uploading the anchor | The upload to your bucket dropped, or R2 refused a multipart completion | The scripts upload again; SDKs after 0.36.7 retry each part and restart the upload (`BASILICA_UPLOAD_RESTARTS`). On a slow or flaky uplink, use `byot-train.sh`, which uploads from a datacenter box |
| Long wait at `2/7 starting a private rollout session` | Cold start: renting a GPU node (5 to 9 min), then a 3 min image pull | Watch the progress lines. Past about 20 minutes the provider is short on that GPU type: Ctrl-C (it cleans up) and re-run with another type, for example `BYOT_DEMO_GPU=A100` |
| `GPU check: no 1x <GPU> offering is listed as available` | Nothing of that type can be rented right now | Nothing was created. Re-run with one of the types it lists, wait and retry, or set `BYOT_SKIP_GPU_CHECK=1` to start anyway |
| `Bad request: Upstream provider request failed` when renting | The provider has no capacity for that offering at this moment; not a problem with your account | The launcher tries the next offerings. If all are refused, retry in a few minutes or pick another type it suggests (`BYOT_TRAIN_GPU=...`) |
| `lost the trainer's output stream` | Your connection to the box dropped (often a sleeping laptop) | The job keeps running and cleans up after itself. The launcher reconnects for 30 minutes; the full log is in your bucket at `byot-runs/<rental>/run.log` |
| `Host key for <ip> has changed` | The box stopped itself when its job ended and the provider gave its address to another machine | Do not remove the key and reconnect. The launcher now checks the rental, stops following and cleans up; read the log from your bucket |
| `this computer was asleep or suspended for about N min` | The machine slept during the run | Keep it awake (lid open, plugged in) |
| `That API key was rejected` | Wrong or revoked key | Delete its line from `~/.basilica-byot-demo/env` and re-run |
| `Your Basilica account already has an SSH key registered` | One SSH key per account; this machine does not have its private half | Run on the machine that has it, or delete the registered key and re-run |
| `COULD NOT delete session ...` or `COULD NOT stop the trainer box ...` | A cleanup call kept failing | Run the command printed under it (it reads your saved key from `~/.basilica-byot-demo/env`). Check for leftover boxes with the snippet below |
| `StaleRevisionError` | You asserted a revision the fleet no longer serves | Expected in the demo. In a trainer: re-request without the assertion or with the current revision |
| `RevisionRejected: ... <reason>` | The fleet refused a revision (digest or apply failure); the previous one keeps serving | The next publish is a full anchor automatically; the reason names the cause |
| No `costUsd` in usage | Staging has no rate configured | Use `gpuHours` and the balance change |

Leftover check (a rented trainer box that is still running):

```bash
set -a; . ~/.basilica-byot-demo/env; set +a
~/.basilica-byot-demo/venv-train/bin/python -c "
import os; from basilica import BasilicaClient
c = BasilicaClient(base_url='https://api-staging.basilica.ai', api_key=os.environ['BASILICA_STAGING_API_KEY'])
for r in c.list_secure_cloud_rentals().rentals: print(r.name, r.status, r.gpu_type, r.ip_address)"
```

Stop one with `c.stop_secure_cloud_rental("<name>")`. The SDK has no call
that lists rollout sessions, so keep the session uid the scripts print.

## Agent hints

Paste this into your coding agent when it writes or runs BYOT code:

```text
Basilica BYOT rules (staging: base_url="https://api-staging.basilica.ai"):
- Install basilica-sdk[publisher]>=0.36.7. Create the client with
  BasilicaClient(base_url=..., api_key=os.environ["BASILICA_STAGING_API_KEY"]).
- Before creating a session, call client.list_secure_cloud_gpus() and check
  the GPU type you want is listed with availability=True; A100 is the usual
  choice on staging.
- Record client.get_balance()["balance"] at the start and the end, and
  label the difference as account-wide.
- Open sessions with client.rl.open_session(url, token, publisher=handle,
  session_uid=uid) so session.usage(), park() and resume() work.
- Read usage (session.usage() or client.rl.session_usage(uid)) BEFORE
  client.rl.delete_session(uid): it is not readable after the delete.
- Wrap the whole run in try/finally. In the finally: read usage, then delete
  the session, then the policy, each in its own try with a few retries, and
  print the exact cleanup call if one still fails. Never leave a session or a
  rented GPU running; stop any box you rented with
  client.stop_secure_cloud_rental(name).
- Retry transient errors (ConnectionError, TimeoutError, RuntimeError
  mentioning status 502/503/504) on reads and deletes with backoff. Never
  blindly retry create_session: a lost response may have created one.
- A failed publish() leaves the handle unchanged, so publishing again is
  safe; after RevisionRejected the next publish is an anchor.
- A session cold start takes about 10 minutes; poll get_session(uid) every
  20 s and report state, replicas and conditions while waiting.
- Keep the machine awake for the whole run (caffeinate -i on macOS).
```
