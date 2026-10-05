#!/usr/bin/env bash
# Basilica BYOT (bring your own trainer): one-command staging demo.
#
#   curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-demo.sh | bash
#
# You keep the trainer. Basilica runs the rollout fleet. This script plays the
# trainer's side of that contract from your laptop: it registers a model
# lineage against YOUR bucket, starts a private vLLM rollout session on a
# rented H100, publishes full weights (an anchor) and then a sparse update (a
# patch) straight to your bucket, and shows the fleet loading each one
# digest-verified, serving it, refusing stale requests, metering usage, and
# parking and resuming without losing a count. Cleans up after itself, also
# on Ctrl-C. About 45-60 minutes, one H100 for that long.
#
# Needs: python3, a Basilica staging API key, and a Cloudflare R2 bucket with
# an R2 API token that can write to it (R2 is the storage backend in this
# version). Local files live in BYOT_DEMO_DIR (default ~/.basilica-byot-demo).
set -euo pipefail

DEMO_DIR="${BYOT_DEMO_DIR:-$HOME/.basilica-byot-demo}"
export BYOT_DEMO_DIR="$DEMO_DIR"
mkdir -p "$DEMO_DIR"
umask 077

command -v python3 >/dev/null 2>&1 || {
  echo "python3 is required. macOS: run 'xcode-select --install' first."; exit 1; }

echo "== Setting up an isolated environment in $DEMO_DIR (first run downloads ~1 GB of Python packages)"
[ -x "$DEMO_DIR/venv/bin/python" ] || python3 -m venv "$DEMO_DIR/venv"
PIP=("$DEMO_DIR/venv/bin/pip" install --quiet --upgrade)
if [ "$(uname -s)" = "Linux" ]; then
  # The publisher does all its tensor work on the CPU: skip the CUDA wheels.
  "${PIP[@]}" torch --index-url https://download.pytorch.org/whl/cpu
fi
"${PIP[@]}" "basilica-sdk[publisher]>=0.36.4" huggingface_hub

# Settings: environment > previously saved file > one-time prompt.
SAVED=
# shellcheck source=/dev/null
[ -f "$DEMO_DIR/env" ] && source "$DEMO_DIR/env"
ask() {  # var, prompt, hidden
  if [ -z "${!1:-}" ]; then
    if [ "$3" = hidden ]; then read -r -s -p "$2: " "$1" </dev/tty; echo
    else read -r -p "$2: " "$1" </dev/tty; fi
    [ -n "${!1}" ] || { echo "Nothing entered for $1; stopping."; exit 1; }
    printf 'export %s=%q\n' "$1" "${!1}" >> "$DEMO_DIR/env"
    SAVED=1
  fi
  # shellcheck disable=SC2163  # exports the variable NAMED by $1, on purpose
  export "$1"
}
ask BASILICA_STAGING_API_KEY "Basilica staging API key (input hidden)" hidden
ask BYOT_BUCKET "Your R2 bucket name" shown
ask BYOT_ENDPOINT "Its R2 endpoint URL (https://<account-id>.r2.cloudflarestorage.com)" shown
ask BYOT_ACCESS_KEY_ID "R2 access key id" shown
ask BYOT_SECRET_ACCESS_KEY "R2 secret access key (input hidden)" hidden
[ -n "${SAVED:-}" ] && echo "== Settings saved to $DEMO_DIR/env (readable only by you; delete it to forget them)."

exec "$DEMO_DIR/venv/bin/python" - <<'PYEOF'
import glob, hashlib, os, secrets, signal, subprocess, sys, time
from importlib.metadata import version

HERE = os.environ.get("BYOT_DEMO_DIR") or os.path.expanduser("~/.basilica-byot-demo")
API = os.environ.get("BASILICA_API_URL", "https://api-staging.basilica.ai")
MODEL = os.environ.get("BYOT_DEMO_MODEL", "Qwen/Qwen2.5-1.5B-Instruct")
GPU = os.environ.get("BYOT_DEMO_GPU", "H100")
GPU_COUNT = int(os.environ.get("BYOT_DEMO_GPU_COUNT", "1"))  # >1 serves with tensor parallelism
SKIP_PARK = os.environ.get("BYOT_DEMO_SKIP_PARK") == "1"
t0 = time.monotonic()

def say(msg):
    print(f"[{(time.monotonic() - t0) / 60:5.1f}m] {msg}", flush=True)

def wait_for(desc, check, timeout_s, every=15):
    end = time.monotonic() + timeout_s
    while time.monotonic() < end:
        got = check()
        if got:
            return got
        time.sleep(every)
    raise SystemExit(f"\nStopped: timed out waiting for {desc}.")

# SIGTERM (a closed terminal, a killed job) cleans up like Ctrl-C does.
signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))

sdk = version("basilica-sdk")
if tuple(int(x) for x in sdk.split(".")[:3]) < (0, 36, 1):
    raise SystemExit(f"basilica-sdk {sdk} is too old for this demo; it needs 0.36.1 or newer.")

from basilica import BasilicaClient
from basilica.publisher import PolicyStorage
from basilica.session import SessionServingError, StaleRevisionError

storage = dict(bucket=os.environ["BYOT_BUCKET"], endpoint=os.environ["BYOT_ENDPOINT"],
               access_key_id=os.environ["BYOT_ACCESS_KEY_ID"],
               secret_access_key=os.environ["BYOT_SECRET_ACCESS_KEY"])
client = BasilicaClient(base_url=API, api_key=os.environ["BASILICA_STAGING_API_KEY"])
rl = client.rl

try:
    rl.get_session("00000000-0000-0000-0000-000000000000")
except PermissionError:
    raise SystemExit(f"That API key was rejected. Remove the line from {HERE}/env and re-run.")
except Exception:
    pass  # a 404 for the probe id means the key works and BYOT is reachable

say(f"downloading {MODEL} (your trainer's starting weights, ~3 GB; skipped if already in your Hugging Face cache)")
def download_model():
    """Download the weights (into the standard Hugging Face cache, so a model
    you already have is not fetched again) in a child process under a
    watchdog. The client's Xet transfer path can stall with no timeout and no
    way to Ctrl-C out of it, so it is switched off in favor of plain HTTPS;
    and a child that still stops receiving data is killed and restarted, and
    the download resumes where it stopped."""
    code = ("import sys; from huggingface_hub import snapshot_download; "
            "print(snapshot_download(sys.argv[1], allow_patterns="
            "['*.safetensors', '*.json', 'tokenizer*', '*.txt']))")
    from huggingface_hub.constants import HF_HUB_CACHE
    cache = os.path.join(HF_HUB_CACHE, "models--" + MODEL.replace("/", "--"))
    def received():
        return sum(os.path.getsize(os.path.join(d, f))
                   for d, _, files in os.walk(cache) for f in files
                   if os.path.isfile(os.path.join(d, f)))
    for attempt in range(1, 6):
        child = subprocess.Popen([sys.executable, "-c", code, MODEL],
                                 stdout=subprocess.PIPE, text=True,
                                 env={**os.environ, "HF_HUB_DISABLE_XET": "1",
                                      "HF_HUB_DOWNLOAD_TIMEOUT": "30"})
        last, idle = received(), 0
        while child.poll() is None:
            time.sleep(5)
            now = received()
            idle = 0 if now != last else idle + 5
            last = now
            if idle >= 90:
                child.kill(); child.wait()
                if attempt < 5:
                    say(f"   download stalled (no data for 90 s); resuming (attempt {attempt + 1}/5)")
                break
        else:
            if child.returncode == 0:
                return child.stdout.read().strip().splitlines()[-1]
            if attempt < 5:
                say(f"   download failed (exit {child.returncode}); retrying (attempt {attempt + 1}/5)")
    raise SystemExit("\nStopped: the model download kept failing; check your connection and re-run.")

snap = download_model()
commit = os.path.basename(snap.rstrip("/"))
tok_digest = "sha256:" + hashlib.sha256(open(f"{snap}/tokenizer.json", "rb").read()).hexdigest()

def weights(nudge=False):
    """The model's bf16 tensors. With nudge=True one tensor moves slightly:
    a stand-in for one optimizer step, so the patch is a real weight change."""
    from safetensors.torch import load_file
    for f in sorted(glob.glob(f"{snap}/*.safetensors")):
        for name, t in load_file(f).items():
            if nudge and name == "model.norm.weight":
                t = (t.float() * 1.001).to(t.dtype)
            yield name, t

def publish(desc, fn, attempts=3):
    # R2 occasionally answers a multipart completion with InvalidPart for
    # parts it just accepted; a clean re-upload succeeds.
    for i in range(1, attempts + 1):
        try:
            return fn()
        except Exception as e:
            if "InvalidPart" not in str(e) or i == attempts:
                raise
            say(f"   storage hiccup on {desc} (attempt {i}/{attempts}), re-uploading")
            time.sleep(10)

policy = f"byot-demo-{secrets.token_hex(3)}"
uid = None
try:
    say(f"1/7 registering policy '{policy}': {MODEL} @ {commit[:12]}, weights live in YOUR bucket")
    rl.create_policy(policy, repo=MODEL, commit=commit, tokenizer_digest=tok_digest, **storage)

    say(f"2/7 starting a private rollout session on {GPU_COUNT}x {GPU} (renting the GPU takes a few minutes)")
    sess = rl.create_session(policy, gpu_model=GPU, gpu_count=GPU_COUNT, replicas=1)
    uid, url, token = sess["sessionUid"], sess["url"], sess.get("token", "")
    say(f"   session {uid}")
    say(f"   url {url} (private: only your session token gets in)")
    def serving():
        st = rl.get_session(uid).get("state")
        if st == "failed":
            raise SystemExit("\nStopped: the session failed to start (see the conditions above).")
        return st in ("ready", "active")
    wait_for("the session to serve", serving, timeout_s=3600, every=20)
    say("   serving")

    say("3/7 publishing the anchor: full bf16 weights, uploaded from here straight to your bucket")
    handle = rl.policy(policy, storage=PolicyStorage(**storage), work_dir=f"{HERE}/publish")
    anchor = "anchor-" + secrets.token_hex(3)
    publish("the anchor", lambda: handle.publish_anchor(weights(), revision=anchor))
    say(f"   {anchor} uploaded; the fleet now fetches it, checks its digest, and loads it")
    rec = handle.wait_until_active(anchor, timeout=3600)
    say(f"   {anchor} is {rec.get('state')}: every replica loaded it and confirmed the digest")

    session = rl.open_session(url, token, publisher=handle, session_uid=uid)
    out = session.generate(prompt="The capital of France is", max_tokens=16, temperature=0)
    say(f"4/7 generating: 'The capital of France is' ... {(out.texts or [''])[0]!r}")
    say(f"   servedRevision={out.served_revision} (every response says which weights produced it)")
    out = session.generate(token_ids=[[785, 6722, 315, 9625, 374]], n=4, seed=7, max_tokens=8)
    say(f"   training dialect: token ids in and out, a group of {len(out.token_ids)} samples, "
        f"sampler logprobs for each token")

    say("5/7 one training step later: publishing a sparse patch (only what changed)")
    patch = "step-" + secrets.token_hex(3)
    publish("the patch", lambda: session.publish(weights(nudge=True), revision=patch))
    session.wait_until_active(patch, timeout=3600)
    out = session.generate(prompt="2+2=", max_tokens=4, temperature=0, revision=patch)
    say(f"   {patch} is Active and serving (asserted revision={patch}, served {out.served_revision})")
    try:
        session.generate(prompt="x", max_tokens=1, revision=anchor)
        raise SystemExit("\nStopped: a stale revision assertion was not refused.")
    except StaleRevisionError:
        say(f"   asking for the old {anchor} is refused with StaleRevisionError; nothing is sampled")
    t_gen = time.time()

    say("6/7 usage, as the platform meters it (the ledger refreshes about every minute)")
    def fresh():
        u = rl.session_usage(uid)
        return u if (u.get("observedAt") or 0) >= t_gen and u.get("completionTokens") else None
    u = wait_for("a usage reading covering the generations", fresh, timeout_s=300)
    say(f"   requests={u.get('requests')} promptTokens={u['promptTokens']} "
        f"completionTokens={u['completionTokens']} gpuHours={u['gpuHours']:.3f} "
        f"unaccountedReplicaSeconds={u.get('unaccountedReplicaSeconds')}")

    if SKIP_PARK:
        say("7/7 park/resume skipped (BYOT_DEMO_SKIP_PARK=1)")
    else:
        say("7/7 park (the GPU is released; the replica hands over its final counts) and resume")
        rl.park_session(uid)
        wait_for("the session to park", lambda: rl.get_session(uid).get("state") == "parked",
                 timeout_s=900)
        before = (u["promptTokens"], u["completionTokens"])
        closed = wait_for("the parked replica's final counts",
                          lambda: (lambda v: v if v.get("replicasReporting", 1) == 0 else None)(
                              rl.session_usage(uid)), timeout_s=600)
        say(f"   parked; counts unchanged {before == (closed['promptTokens'], closed['completionTokens'])}, "
            f"unaccountedReplicaSeconds={closed.get('unaccountedReplicaSeconds')}")
        rl.resume_session(uid)
        say("   resuming: a fresh replica rents a GPU and replays anchor + patch")
        wait_for("the session to serve again",
                 lambda: rl.get_session(uid).get("state") in ("ready", "active"),
                 timeout_s=3600, every=20)
        def serves_patch():
            try:
                o = session.generate(prompt="Hello", max_tokens=4, temperature=0)
            except SessionServingError:
                return None  # the new replica can answer 502/503 for a few seconds
            return o if o.served_revision == patch else None
        wait_for("the resumed replica to serve the patch", serves_patch, timeout_s=1800, every=20)
        say(f"   the resumed replica serves {patch}: same lineage, nothing re-published")

    say("done. Your trainer published two revisions; the fleet loaded, verified, served and")
    say("metered them, and survived a park/resume. Cleaning up.")
finally:
    try:
        if uid:
            rl.delete_session(uid)
            say(f"cleanup: session {uid} deleted (GPU released)")
        rl.delete_policy(policy)
        say(f"cleanup: policy {policy} deleted (the weights in your bucket stay yours)")
    except Exception as e:
        say(f"cleanup problem: {e}; delete session {uid} and policy {policy} by hand")
PYEOF
