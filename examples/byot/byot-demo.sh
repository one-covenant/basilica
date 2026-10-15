#!/usr/bin/env bash
# Basilica BYOT (bring your own trainer): one-command staging demo.
#
#   curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-demo.sh | bash
#
# You keep the trainer. Basilica runs the rollout fleet. This script plays the
# trainer's side of that contract from your laptop: it registers a model
# lineage against YOUR bucket, starts a private vLLM rollout session on a
# rented A100, publishes full weights (an anchor) and then a sparse update (a
# patch) straight to your bucket, and shows the fleet loading each one
# digest-verified, serving it, refusing stale requests, metering usage, and
# parking and resuming without losing a count. Before it deletes anything it
# prints the session's usage and your account balance change. Cleans up after
# itself, also on Ctrl-C. About 45-60 minutes, one A100 for that long.
#
# Needs: python3, a Basilica staging API key, and a Cloudflare R2 bucket with
# an R2 API token that can write to it (R2 is the storage backend in this
# version). Local files (saved answers, logs) live in BYOT_DEMO_DIR (default
# ~/.basilica-byot-demo). Settings: BYOT_DEMO_GPU (default A100),
# BYOT_DEMO_GPU_COUNT (default 1), BYOT_DEMO_MODEL, BYOT_DEMO_SKIP_PARK=1,
# BYOT_SKIP_GPU_CHECK=1 (start even when no matching GPU is listed). Keep the
# machine awake while it runs: on macOS the script holds a caffeinate
# assertion, but closing the lid still sleeps a laptop on battery.
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
"${PIP[@]}" "basilica-sdk[publisher]>=0.36.7" huggingface_hub

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

# A sleeping laptop suspends this script, and a long sleep loses the run's
# connections: keep the machine awake for as long as the demo runs.
AWAKE=()
if command -v caffeinate >/dev/null 2>&1; then
  AWAKE=(caffeinate -i -s)
  echo "== Keeping this Mac awake while the demo runs (caffeinate). Keep the lid open: a closed lid still sleeps a laptop on battery."
else
  echo "== Keep this machine awake while the demo runs (no sleep, no suspend): a sleeping machine stalls the run."
fi

exec ${AWAKE[@]+"${AWAKE[@]}"} "$DEMO_DIR/venv/bin/python" - <<'PYEOF'
import glob, hashlib, os, re, secrets, signal, subprocess, sys, time
from importlib.metadata import version

HERE = os.environ.get("BYOT_DEMO_DIR") or os.path.expanduser("~/.basilica-byot-demo")
API = os.environ.get("BASILICA_API_URL", "https://api-staging.basilica.ai")
MODEL = os.environ.get("BYOT_DEMO_MODEL", "Qwen/Qwen2.5-1.5B-Instruct")
GPU = os.environ.get("BYOT_DEMO_GPU", "A100")
GPU_COUNT = int(os.environ.get("BYOT_DEMO_GPU_COUNT", "1"))  # >1 serves with tensor parallelism
SKIP_PARK = os.environ.get("BYOT_DEMO_SKIP_PARK") == "1"
SKIP_GPU_CHECK = os.environ.get("BYOT_SKIP_GPU_CHECK") == "1"
# Wall-clock time, not a monotonic clock: on macOS the monotonic clock stops
# while the machine sleeps, which would hide a sleep from the timestamps.
t0 = time.time()
os.makedirs(f"{HERE}/logs", exist_ok=True)
LOG_PATH = f"{HERE}/logs/byot-demo-{time.strftime('%Y%m%d-%H%M%S')}.log"
log_file = open(LOG_PATH, "a", buffering=1)

def say(msg):
    line = f"[{(time.time() - t0) / 60:5.1f}m] {msg}"
    log_file.write(line + "\n")
    try:
        print(line, flush=True)
    except OSError:
        pass  # the terminal went away; the log file keeps the record

def short(e):
    return " ".join(str(e).split())[:200] or type(e).__name__

def transient(e):
    """A failure that the same call can get past a little later: a dropped
    connection, a DNS error, a timeout, a 502/503/504, a rate limit."""
    if isinstance(e, (ConnectionError, TimeoutError)):
        return True
    return isinstance(e, RuntimeError) and bool(re.search(
        r"status 50[234]|temporarily unavailable|Request timeout|Rate limit", str(e), re.I))

def nap(seconds):
    """Sleep, and say so if the machine slept far longer than asked."""
    start = time.time()
    time.sleep(seconds)
    overslept = time.time() - start - seconds
    if overslept > 90:
        say(f"   this computer was asleep or suspended for about {overslept / 60:.0f} min; "
            "the demo carries on, but keep the machine awake (lid open, plugged in)")

def retrying(desc, fn, budget_s=300):
    """Call fn, riding out transient network failures for up to budget_s."""
    deadline, attempt = time.time() + budget_s, 0
    while True:
        attempt += 1
        try:
            return fn()
        except Exception as e:
            if not transient(e) or time.time() >= deadline:
                raise
            wait = min(30, 2 ** attempt)
            say(f"   {desc}: network problem ({short(e)}); retrying in {wait}s")
            nap(wait)

def wait_for(desc, check, timeout_s, every=15):
    end = time.time() + timeout_s
    while time.time() < end:
        got = retrying(desc, check)
        if got:
            return got
        nap(every)
    raise SystemExit(f"\nStopped: timed out waiting for {desc}.")

# A closed terminal (SIGHUP) or a killed job (SIGTERM) cleans up like Ctrl-C.
signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
signal.signal(signal.SIGHUP, lambda *_: sys.exit(129))

sdk = version("basilica-sdk")
if tuple(int(x) for x in re.findall(r"\d+", sdk)[:3]) < (0, 36, 1):
    raise SystemExit(f"basilica-sdk {sdk} is too old for this demo; it needs 0.36.1 or newer.")

from basilica import BasilicaClient
from basilica.publisher import PolicyStorage
from basilica.session import SessionServingError, StaleRevisionError

storage = dict(bucket=os.environ["BYOT_BUCKET"], endpoint=os.environ["BYOT_ENDPOINT"],
               access_key_id=os.environ["BYOT_ACCESS_KEY_ID"],
               secret_access_key=os.environ["BYOT_SECRET_ACCESS_KEY"])
client = BasilicaClient(base_url=API, api_key=os.environ["BASILICA_STAGING_API_KEY"])
rl = client.rl
say(f"basilica-sdk {sdk}; full log: {LOG_PATH}")

try:
    retrying("checking the API key", lambda: rl.get_session("00000000-0000-0000-0000-000000000000"))
except PermissionError:
    raise SystemExit(f"That API key was rejected. Remove the line from {HERE}/env and re-run.")
except Exception:
    pass  # a 404 for the probe id means the key works and BYOT is reachable

def balance():
    try:
        return float(retrying("reading the balance", client.get_balance)["balance"])
    except Exception:
        return None

def gpu_check():
    """Before anything is created: is the requested GPU listed as available?
    Session GPUs are rented from the same offerings this lists."""
    try:
        offers = retrying("listing GPU offerings", client.list_secure_cloud_gpus)
    except Exception as e:
        say(f"   GPU check skipped: could not list offerings ({short(e)})")
        return
    usable = [o for o in offers if o.availability and not o.is_spot]
    match = [o for o in usable if o.gpu_type == GPU and o.gpu_count >= GPU_COUNT]
    if match:
        cheapest = min(float(o.hourly_rate) * o.gpu_count for o in match)
        say(f"   GPU check: {len(match)} offering(s) with {GPU_COUNT}+ {GPU} listed as available "
            f"(from ${cheapest:.2f}/hr); the platform rents one of these for the session")
        return
    alts = sorted({o.gpu_type for o in usable if o.gpu_count >= GPU_COUNT
                   and float(o.gpu_memory_gb_per_gpu or 0) >= 40})
    msg = (f"GPU check: no {GPU_COUNT}x {GPU} offering is listed as available right now, so the "
           f"session could wait a long time for a GPU. Available now: {', '.join(alts) or 'none'}.")
    if SKIP_GPU_CHECK:
        say(f"   {msg} Going ahead anyway (BYOT_SKIP_GPU_CHECK=1).")
        return
    raise SystemExit(f"\n{msg}\nNothing was created. Re-run with BYOT_DEMO_GPU=<one of those> "
                     "(A100 is the usual choice), wait and try again, or set BYOT_SKIP_GPU_CHECK=1.")

def wait_serving(uid, timeout_s=3600):
    """Poll the session until it serves, saying what it is waiting for."""
    start, last_note, last_said = time.time(), None, 0.0
    while True:
        s = retrying("checking the session", lambda: rl.get_session(uid))
        state = s.get("state")
        if state in ("ready", "active"):
            return s
        conds = "; ".join(f"{c.get('reason')}: {c.get('message')}" for c in s.get("conditions") or [])
        if state == "failed":
            raise SystemExit(f"\nStopped: the session failed to start. {conds or 'No reason given.'}")
        mins = (time.time() - start) / 60
        reps = s.get("replicas") or []
        if reps:
            gpus = ", ".join(sorted({r.get("gpuProduct") or "a GPU" for r in reps}))
            note = (f"GPU node is up ({gpus}); pulling the serving image and starting vLLM "
                    "(usually 3 to 5 minutes)")
        elif mins < 12:
            note = ("waiting for a GPU node: renting one usually takes 5 to 9 minutes, then the "
                    "serving image pulls (about 3 minutes)")
        else:
            note = ("still no GPU node: the provider is slow or short on this GPU type right now. "
                    f"Waiting up to {timeout_s // 60} min; Ctrl-C stops and cleans up")
        if conds:
            note += f" [{conds}]"
        if note != last_note or time.time() - last_said >= 120:
            say(f"   {state}, {mins:.1f} min so far: {note}")
            last_note, last_said = note, time.time()
        if time.time() - start > timeout_s:
            raise SystemExit(f"\nStopped: the session was not serving after {timeout_s // 60} minutes.")
        nap(20)

def report_usage(uid):
    """Read usage BEFORE the session is deleted: it is readable only while the
    session exists."""
    try:
        u = retrying("reading usage", lambda: rl.session_usage(uid))
    except Exception as e:
        say(f"   usage: could not read it ({short(e)})")
        return
    cost = u.get("costUsd")
    say(f"   usage of session {uid}: {float(u.get('gpuHours') or 0):.3f} GPU hours, "
        f"requests={u.get('requests')}, promptTokens={u.get('promptTokens')}, "
        f"completionTokens={u.get('completionTokens')}, "
        + (f"cost ${float(cost):.2f}" if cost is not None
           else "no cost figure (costUsd is absent: no rate is configured here)"))

def manual(call):
    """The exact command to run a cleanup call by hand."""
    return (f"set -a; . {HERE}/env; set +a; {HERE}/venv/bin/python -c "
            f"\"import os; from basilica import BasilicaClient as C; C(base_url='{API}', "
            f"api_key=os.environ['BASILICA_STAGING_API_KEY']).rl.{call}\"")

cleanup_failed = []

def cleanup(desc, fn, call):
    for attempt in range(1, 6):
        try:
            fn()
            return True
        except KeyError:
            return True  # already gone
        except Exception as e:
            if attempt == 5:
                say(f"   COULD NOT {desc}: {short(e)}")
                say(f"   Do it by hand:  {manual(call)}")
                cleanup_failed.append(desc)
                return False
            say(f"   {desc} failed ({short(e)}); retrying in {10 * attempt}s")
            time.sleep(10 * attempt)

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

def weights(snap, nudge=False):
    """The model's bf16 tensors. With nudge=True one tensor moves slightly:
    a stand-in for one optimizer step, so the patch is a real weight change."""
    from safetensors.torch import load_file
    for f in sorted(glob.glob(f"{snap}/*.safetensors")):
        for name, t in load_file(f).items():
            if nudge and name == "model.norm.weight":
                t = (t.float() * 1.001).to(t.dtype)
            yield name, t

def publish(desc, fn, attempts=3):
    # The SDK retries uploads itself (0.36.8 and newer); this covers older
    # SDKs and an outage longer than the SDK's own retries. A failed publish
    # leaves the handle unchanged, so publishing again is safe.
    for i in range(1, attempts + 1):
        try:
            return fn()
        except Exception as e:
            text = str(e)
            hiccup = transient(e) or any(m in text for m in (
                "InvalidPart", "Connection was closed", "Could not connect", "timeout",
                "Read timed out", "dns error", "SlowDown", "ServiceUnavailable"))
            if not hiccup or i == attempts:
                raise
            say(f"   storage or network hiccup on {desc} ({short(e)}); "
                f"uploading again (attempt {i + 1}/{attempts})")
            nap(15 * i)

def main():
    policy = f"byot-demo-{secrets.token_hex(3)}"
    uid = None
    say(f"0/7 checking that {GPU} GPUs are available and reading your account balance")
    gpu_check()
    b0 = balance()
    if b0 is not None:
        say(f"   account balance: ${b0:.2f} (account-wide: shared by everyone using this API key's account)")

    say(f"downloading {MODEL} (your trainer's starting weights, ~3 GB; skipped if already in your Hugging Face cache)")
    snap = download_model()
    commit = os.path.basename(snap.rstrip("/"))
    tok_digest = "sha256:" + hashlib.sha256(open(f"{snap}/tokenizer.json", "rb").read()).hexdigest()
    try:
        say(f"1/7 registering policy '{policy}': {MODEL} @ {commit[:12]}, weights live in YOUR bucket")
        rl.create_policy(policy, repo=MODEL, commit=commit, tokenizer_digest=tok_digest, **storage)

        say(f"2/7 starting a private rollout session on {GPU_COUNT}x {GPU}. Cold start is about "
            "10 minutes (GPU node 5-9 min, image pull ~3 min); progress is printed while it waits")
        sess = rl.create_session(policy, gpu_model=GPU, gpu_count=GPU_COUNT, replicas=1)
        uid, url, token = sess["sessionUid"], sess["url"], sess.get("token", "")
        say(f"   session {uid}")
        say(f"   url {url} (private: only your session token gets in)")
        wait_serving(uid)
        say("   serving")

        say("3/7 publishing the anchor: full bf16 weights (~3 GB), uploaded from here straight to "
            "your bucket (a few minutes on a home connection)")
        handle = retrying("opening the policy", lambda: rl.policy(
            policy, storage=PolicyStorage(**storage), work_dir=f"{HERE}/publish"))
        anchor = "anchor-" + secrets.token_hex(3)
        publish("the anchor", lambda: handle.publish_anchor(weights(snap), revision=anchor))
        say(f"   {anchor} uploaded; the fleet now fetches it, checks its digest, and loads it")
        rec = retrying(f"waiting for {anchor}",
                       lambda: handle.wait_until_active(anchor, timeout=3600, session=uid))
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
        publish("the patch", lambda: session.publish(weights(snap, nudge=True), revision=patch))
        retrying(f"waiting for {patch}", lambda: session.wait_until_active(patch, timeout=3600, session=uid))
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
            say("   resuming: a fresh replica rents a GPU and replays anchor + patch (another ~10 min cold start)")
            wait_serving(uid)
            def serves_patch():
                try:
                    o = session.generate(prompt="Hello", max_tokens=4, temperature=0)
                except SessionServingError:
                    return None  # the new replica can answer 502/503 for a few seconds
                return o if o.served_revision == patch else None
            wait_for("the resumed replica to serve the patch", serves_patch, timeout_s=1800, every=20)
            say(f"   the resumed replica serves {patch}: same lineage, nothing re-published")

        say("done. Your trainer published two revisions; the fleet loaded, verified, served and")
        say("metered them" + ("" if SKIP_PARK else ", and survived a park/resume") + ". Cleaning up.")
    finally:
        # Cleanup must finish even if Ctrl-C is pressed again or the terminal
        # closes: nothing may keep billing.
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            signal.signal(sig, lambda *_: say("   still cleaning up so nothing keeps billing; one moment"))
        say("cleanup")
        if uid:
            report_usage(uid)
            if cleanup(f"delete session {uid}", lambda: rl.delete_session(uid), f"delete_session('{uid}')"):
                say(f"   session {uid} deleted (GPU released)")
        if cleanup(f"delete policy {policy}", lambda: rl.delete_policy(policy), f"delete_policy('{policy}')"):
            say(f"   policy {policy} deleted (the weights in your bucket stay yours)")
        b1 = balance()
        if b0 is not None and b1 is not None:
            say(f"   account balance: ${b0:.2f} at the start, ${b1:.2f} now ({b1 - b0:+.2f}). Account-wide: "
                "anything else billed to this account meanwhile (another tester on the same key) is "
                "included, and billing can lag a few minutes")
        say(f"   full log: {LOG_PATH}")

try:
    main()
except KeyboardInterrupt:
    say("interrupted (Ctrl-C); " + ("cleanup INCOMPLETE, see above" if cleanup_failed else "cleaned up"))
    sys.exit(130)
if cleanup_failed:
    raise SystemExit(f"\nCleanup incomplete ({'; '.join(cleanup_failed)}): run the commands above.")
PYEOF
