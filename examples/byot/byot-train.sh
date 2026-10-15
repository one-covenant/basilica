#!/usr/bin/env bash
# Basilica BYOT (bring your own trainer): REAL training, one command.
#
#   curl -fsSL https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot/byot-train.sh | bash
#
# Where byot-demo.sh shows the contract with a synthetic weight change, this
# one trains: it rents a GPU for YOUR trainer on Basilica staging, starts a
# 2-replica rollout session, and runs real GRPO steps (Qwen2.5-1.5B-Instruct on
# GSM8K). Every step samples through the session, takes an optimizer step
# against the sampler's own logprobs, and publishes the new weights as a
# sparse patch to your bucket. You watch the reward climb, see which revision
# served each batch, and get a verdict at the end.
#
# Cleans up after itself, also on Ctrl-C: the session and policy are deleted
# and the rented trainer GPU is stopped (its cost is printed, with the
# session's usage and your account balance change). About 35-45 minutes and
# three A100s (one trainer, two rollout replicas) for 20 steps; set
# BYOT_TRAIN_STEPS for more. Before renting anything it checks that the GPU
# types it needs are listed as available. Keep this machine awake while it
# runs (on macOS the script holds a caffeinate assertion; a closed lid still
# sleeps a laptop on battery). If it loses the trainer's output, the job keeps
# running on the box and cleans up after itself.
#
# Needs: python3 and ssh, the same staging API key and Cloudflare R2 bucket as
# byot-demo.sh (answers are shared between the two).
#
# Larger models: BYOT_TRAINER_GPUS=8 rents a multi-GPU trainer box and launches
# the trainer with torchrun (one process per GPU), e.g. with
# BYOT_TRAINER_PY=byot_grpo_fsdp.py for Qwen3-30B-A3B. BYOT_MODEL,
# BYOT_SESSION_GPUS, BYOT_REPLICAS, BYOT_MICRO_BATCH, BYOT_MAX_TOKENS and
# BYOT_CPU_OFFLOAD are passed through to the trainer, as are BYOT_TASK
# (gsm8k | countdown, byot_grpo_fsdp.py), BYOT_LR and BYOT_PROMPTS.
# BYOT_TRAIN_GPU picks the GPU type (default A100), BYOT_TRAIN_MIN_GPU_GB a
# minimum memory per GPU and BYOT_TRAIN_MIN_RAM_GB a minimum host RAM for the box.
# Spot offerings are skipped unless BYOT_ALLOW_SPOT=1 (they can be preempted
# mid-run). The trainer box is not rented above BYOT_MAX_HOURLY dollars per
# hour for the whole box (default 10); raise it on purpose. Local files
# (saved answers, SSH key, full logs) live in BYOT_DEMO_DIR (default
# ~/.basilica-byot-demo).
set -euo pipefail

DEMO_DIR="${BYOT_DEMO_DIR:-$HOME/.basilica-byot-demo}"
export BYOT_DEMO_DIR="$DEMO_DIR"
SRC_RAW="https://raw.githubusercontent.com/one-covenant/basilica/main/examples/byot"
mkdir -p "$DEMO_DIR"
umask 077

for tool in python3 ssh ssh-keygen curl; do
  command -v "$tool" >/dev/null 2>&1 || { echo "$tool is required."; exit 1; }
done

echo "== Setting up a small local environment in $DEMO_DIR (the training itself runs on the rented GPU)"
[ -x "$DEMO_DIR/venv-train/bin/python" ] || python3 -m venv "$DEMO_DIR/venv-train"
"$DEMO_DIR/venv-train/bin/pip" install --quiet --upgrade "basilica-sdk>=0.36.7"

# The trainer that runs on the rented GPU: next to this script when run from a
# checkout, otherwise fetched from this repository.
HERE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")" 2>/dev/null && pwd || echo "")"
if [ -n "${BYOT_TRAINER_PY:-}" ] && [ -f "${BYOT_TRAINER_PY}" ]; then
  :  # an explicit trainer file (e.g. byot_grpo_minimal.py)
elif [ -n "$HERE_DIR" ] && [ -f "$HERE_DIR/byot_grpo.py" ]; then
  BYOT_TRAINER_PY="$HERE_DIR/byot_grpo.py"
else
  curl -fsSL "$SRC_RAW/byot_grpo.py" -o "$DEMO_DIR/byot_grpo.py"
  BYOT_TRAINER_PY="$DEMO_DIR/byot_grpo.py"
fi
export BYOT_TRAINER_PY

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

# A sleeping laptop suspends the launcher and drops its connection to the
# trainer: keep the machine awake for as long as it runs.
AWAKE=()
if command -v caffeinate >/dev/null 2>&1; then
  AWAKE=(caffeinate -i -s)
  echo "== Keeping this Mac awake while the launcher runs (caffeinate). Keep the lid open: a closed lid still sleeps a laptop on battery."
else
  echo "== Keep this machine awake while the launcher runs: a sleeping machine loses the trainer's output (the job itself keeps running)."
fi

exec ${AWAKE[@]+"${AWAKE[@]}"} "$DEMO_DIR/venv-train/bin/python" - <<'PYEOF'
import os, re, secrets, shlex, signal, subprocess, sys, time

HERE = os.environ.get("BYOT_DEMO_DIR") or os.path.expanduser("~/.basilica-byot-demo")
API = os.environ.get("BASILICA_API_URL", "https://api-staging.basilica.ai")
STEPS = int(os.environ.get("BYOT_TRAIN_STEPS", "20"))
GPU = os.environ.get("BYOT_TRAIN_GPU", "A100")
NGPU = int(os.environ.get("BYOT_TRAINER_GPUS", "1"))
MAX_HOURLY = float(os.environ.get("BYOT_MAX_HOURLY", "10"))
MIN_GPU_GB = float(os.environ.get("BYOT_TRAIN_MIN_GPU_GB", "0"))
MIN_RAM_GB = float(os.environ.get("BYOT_TRAIN_MIN_RAM_GB", "0"))
MAX_RUNTIME_H = float(os.environ.get("BYOT_MAX_RUNTIME_HOURS", "8"))  # hard cap on the rented job
ALLOW_SPOT = os.environ.get("BYOT_ALLOW_SPOT") == "1"
# Trainer settings forwarded as-is when set (see the trainer file for each).
PASS_THROUGH = ("BYOT_MODEL", "BYOT_SESSION_GPUS", "BYOT_REPLICAS", "BYOT_MICRO_BATCH",
                "BYOT_MAX_TOKENS", "BYOT_CPU_OFFLOAD", "BYOT_TRAINER_ONLY",
                "BYOT_SESSION_GPU", "BYOT_SESSION_MEMORY_GIB", "BYOT_SESSION_CPU_CORES", "BYOT_SESSION_MIN_GPU_GB", "BYOT_EVAL_PROMPTS", "BYOT_EVAL_EVERY", "BYOT_MIN_FREE_GB", "BYOT_PUBLISH_DIR", "BYOT_K3_SYNC_EVERY", "BYOT_RECORD",
                "BYOT_TASK", "BYOT_LR", "BYOT_PROMPTS",
                "BYOT_PATCH_FRACTION", "BYOT_PROFILE_APPLY_ONLY")
KEY = f"{HERE}/ssh/id_ed25519"
KNOWN = f"{HERE}/ssh/known_hosts"
# Wall-clock time throughout: on macOS the monotonic clock stops while the
# machine sleeps, so a deadline on it could run for hours of real time.
t0 = time.time()

def say(msg):
    try:
        print(f"[{(time.time() - t0) / 60:5.1f}m] {msg}", flush=True)
    except OSError:
        pass  # the terminal went away; the job and its log on the box carry on

def short(e):
    return " ".join(str(e).split())[:200] or type(e).__name__

def transient(e):
    """A failure the same call can get past a little later: a dropped
    connection, a DNS error, a timeout, a 502/503/504, a rate limit."""
    if isinstance(e, (ConnectionError, TimeoutError)):
        return True
    return isinstance(e, RuntimeError) and bool(re.search(
        r"status 50[234]|temporarily unavailable|Request timeout|Rate limit", str(e), re.I))

def retrying(desc, fn, budget_s=300):
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
            time.sleep(wait)

# SIGTERM (a closed terminal, a killed job) cleans up like Ctrl-C does.
signal.signal(signal.SIGTERM, lambda *_: sys.exit(143))
# A local network drop or a closed terminal can end this launcher while the
# job keeps running on the box; record when and how, so a silent exit is
# never a mystery. (HUP is logged and then handled as SIGTERM.)
import atexit
atexit.register(lambda: print(f"[{time.strftime('%H:%M:%S')}] launcher exiting", flush=True))
def _on_hup(*_):
    print(f"[{time.strftime('%H:%M:%S')}] launcher got SIGHUP", flush=True)
    sys.exit(129)
signal.signal(signal.SIGHUP, _on_hup)
def _quiet_ctrl_c(kind, value, tb):  # the cleanup above it already ran
    if issubclass(kind, KeyboardInterrupt):
        print("interrupted (Ctrl-C); cleaned up", flush=True)
        return
    sys.__excepthook__(kind, value, tb)
sys.excepthook = _quiet_ctrl_c

from basilica import BasilicaClient
client = BasilicaClient(base_url=API, api_key=os.environ["BASILICA_STAGING_API_KEY"])
rl = client.rl

# Two launchers from one machine print into the same terminal or log and
# are easy to mistake for one: say so when another one is still running.
os.makedirs(f"{HERE}/run", exist_ok=True)
for pidfile in sorted(os.listdir(f"{HERE}/run")):
    try:
        other = int(open(f"{HERE}/run/{pidfile}").read().split()[0])
        os.kill(other, 0)
        say(f"note: another byot-train launcher is still running on this machine (pid {other}, "
            f"{pidfile[:-4]}); its output is not this run's")
    except (OSError, ValueError, IndexError):
        try:
            os.remove(f"{HERE}/run/{pidfile}")  # a launcher that is gone
        except OSError:
            pass

def balance():
    try:
        return float(retrying("reading the balance", client.get_balance)["balance"])
    except Exception:
        return None

def manual(call):
    """The exact command to run a cleanup call by hand."""
    return (f"set -a; . {HERE}/env; set +a; {HERE}/venv-train/bin/python -c "
            f"\"import os; from basilica import BasilicaClient as C; C(base_url='{API}', "
            f"api_key=os.environ['BASILICA_STAGING_API_KEY']).{call}\"")

def cleanup(desc, fn, call):
    """Run a cleanup call with retries; on failure print how to do it by hand."""
    for attempt in range(1, 6):
        try:
            return fn() or True
        except KeyError:
            return True  # already gone
        except Exception as e:
            if attempt == 5:
                say(f"   COULD NOT {desc}: {short(e)}")
                say(f"   Do it by hand:  {manual(call)}")
                return None
            time.sleep(10 * attempt)

# -- 1. an SSH key the rented machine will accept ---------------------------
os.makedirs(os.path.dirname(KEY), exist_ok=True)
if not os.path.exists(KEY):
    subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-C", "basilica-byot-demo", "-f", KEY],
                   check=True)
mine = open(KEY + ".pub").read().split()[1]
registered = client.get_ssh_key()
if registered is None:
    client.register_ssh_key("byot-demo", public_key=open(KEY + ".pub").read().strip())
    say("registered this machine's demo SSH key with your Basilica account")
elif registered.public_key.split()[1] != mine:
    # One key per account: use the local private key that matches, if any.
    match = None
    for pub in sorted(os.listdir(os.path.expanduser("~/.ssh"))) if os.path.isdir(os.path.expanduser("~/.ssh")) else []:
        path = os.path.expanduser(f"~/.ssh/{pub}")
        if pub.endswith(".pub") and os.path.exists(path[:-4]):
            try:
                if open(path).read().split()[1] == registered.public_key.split()[1]:
                    match = path[:-4]
                    break
            except (IndexError, OSError):
                pass
    if match is None:
        raise SystemExit(
            f"\nYour Basilica account already has an SSH key registered ({registered.name!r}), and no "
            "private key on this machine matches it. Basilica allows one key per account. Either run "
            "this on the machine that has that key, or remove it (Python: "
            "BasilicaClient(...).delete_ssh_key(), or `basilica ssh-keys delete`) and re-run.")
    KEY = match
    say(f"using your registered SSH key ({registered.name}, {KEY})")

def ssh_base(target, port):
    return ["ssh", "-i", KEY, "-p", str(port), "-o", "BatchMode=yes", "-o", "ConnectTimeout=15",
            "-o", "StrictHostKeyChecking=accept-new", "-o", f"UserKnownHostsFile={KNOWN}",
            "-o", "ServerAliveInterval=30", target]

tag = secrets.token_hex(3)
rental_name, policy, session_uid = f"byot-trainer-{tag}", f"byot-train-{tag}", None
# One known_hosts per rental: providers reuse IPs, and a new box behind a
# reused IP has a new host key that a shared file would reject forever.
KNOWN = f"{HERE}/ssh/known_hosts-{rental_name}"
rented = False
job_owns_cleanup = False  # set once a started job may still be running on the box
box_cost = None  # (hours, dollars) once the trainer box has stopped
launched = False  # set once the trainer job may have started
pid_path = f"{HERE}/run/{rental_name}.pid"
open(pid_path, "w").write(f"{os.getpid()}\n")
atexit.register(lambda: os.path.exists(pid_path) and os.remove(pid_path))

def find_rental():
    """This run's rental as the API lists it now (None when it is not listed)."""
    return next((x for x in retrying("checking the rental", client.list_secure_cloud_rentals).rentals
                 if x.name == rental_name), None)

def ssh_target(r):
    m = re.search(r"(\S+@\S+)", r.ssh_command)
    p = re.search(r"-p\s+(\d+)", r.ssh_command)
    return (m.group(1) if m else f"ubuntu@{r.ip_address}"), (int(p.group(1)) if p else 22)

# Statuses of a rental that is no longer a running machine.
GONE = ("stopped", "stopping", "terminated", "terminating", "deleted", "error", "failed")
b0 = balance()
try:
    # -- 2. rent the trainer GPU ------------------------------------------------
    def box_rate(o):  # offerings are priced per GPU
        return float(o.hourly_rate or 1e9) * o.gpu_count
    say("0/5 GPU check: which offerings are available right now")
    all_offers = retrying("listing GPU offerings", client.list_secure_cloud_gpus)
    listed = [o for o in all_offers if o.availability and (ALLOW_SPOT or not o.is_spot)]
    offers = [o for o in listed
              if o.gpu_type == GPU and o.gpu_count == NGPU
              and float(o.gpu_memory_gb_per_gpu or 0) >= MIN_GPU_GB
              and float(o.system_memory_gb or 0) >= MIN_RAM_GB]
    offers.sort(key=box_rate)
    box = f"{NGPU}x {GPU}"

    def alternatives():
        """Other GPU types with a box of this shape listed now, cheapest first."""
        best = {}
        for o in listed:
            if (o.gpu_type != GPU and o.gpu_count == NGPU and float(o.gpu_memory_gb_per_gpu or 0)
                    >= max(MIN_GPU_GB, 40) and float(o.system_memory_gb or 0) >= MIN_RAM_GB):
                best[o.gpu_type] = min(best.get(o.gpu_type, 1e9), box_rate(o))
        return ", ".join(f"BYOT_TRAIN_GPU={t} (from ${r:.2f}/hr)"
                         for t, r in sorted(best.items(), key=lambda kv: kv[1])[:4]) or "none listed"
    if not offers:
        raise SystemExit(f"\nNo {box} box is listed as available on staging right now. Nothing was "
                         f"created. Try again later, or another GPU type: {alternatives()}.")
    if box_rate(offers[0]) > MAX_HOURLY:
        raise SystemExit(f"\nThe cheapest {box} box costs ${box_rate(offers[0]):.2f}/hr, above "
                         f"BYOT_MAX_HOURLY=${MAX_HOURLY:.2f}. Nothing was created; set BYOT_MAX_HOURLY to go ahead.")
    offers = [o for o in offers if box_rate(o) <= MAX_HOURLY]
    say(f"   trainer: {len(offers)} offering(s) for {box} at or under ${MAX_HOURLY:g}/hr")
    # The session's GPUs come from the same offerings (the platform rents
    # them): a type with nothing listed would leave the session waiting.
    sgpu = os.environ.get("BYOT_SESSION_GPU") or "A100"
    if not any(o.gpu_type == sgpu for o in listed):
        say(f"   rollout session: no {sgpu} offering is listed right now, so the session may wait a long "
            f"time for its GPUs (set BYOT_SESSION_GPU to a listed type if your trainer reads it)")
    else:
        say(f"   rollout session: {sgpu} offerings are listed")
    if b0 is not None:
        say(f"   account balance: ${b0:.2f} (account-wide: shared by everyone using this API key's account)")
    say(f"1/5 renting {box} for your trainer (cheapest available: ${box_rate(offers[0]):.2f}/hr, "
        f"{offers[0].gpu_memory_gb_per_gpu:g} GB per GPU, {offers[0].system_memory_gb} GB RAM)")
    last_err = None
    for i, o in enumerate(offers[:5]):
        try:
            client.start_secure_cloud_rental(o.id, name=rental_name)
            rented = True
            say(f"   rented {rental_name} from {o.provider} ({o.region}, ${box_rate(o):.2f}/hr)")
            break
        except Exception as e:  # provider hiccup: try the next offering
            last_err = e
            why = ("the provider has no capacity for that offering at the moment (normal on a busy "
                   "day; nothing is wrong with your account or settings)"
                   if "Upstream provider" in str(e) else short(e)[:120])
            more = "trying the next offering" if i + 1 < min(len(offers), 5) else "no more offerings to try"
            say(f"   {o.provider} {o.region}: {why}; {more}")
    if not rented:
        raise SystemExit(f"\nCould not rent {box}: every offering tried was refused (last: {short(last_err)}). "
                         f"Nothing is running. Try again in a few minutes, or another GPU type: {alternatives()}.")

    target, port, deadline, last_ssh_error = None, 22, time.time() + 1800, ""
    while time.time() < deadline:
        # A transient API error (a dropped connection, a 5xx) must not end the
        # launch: the box is rented and billing, so keep waiting for it.
        try:
            r = find_rental()
        except Exception as e:  # noqa: BLE001
            say(f"   checking the rental failed ({short(e)[:120]}); retrying")
            time.sleep(15)
            continue
        if r and r.status in GONE:
            raise SystemExit(f"\nThe trainer box {rental_name} ended up '{r.status}' before it came up. "
                             "Re-run to rent another one.")
        if r and r.ip_address and r.ssh_command:
            target, port = ssh_target(r)
            probe = subprocess.run(ssh_base(target, port) + ["true"], capture_output=True, text=True)
            if probe.returncode == 0:
                break
            last_ssh_error = probe.stderr.strip()[-300:]
        time.sleep(15)
    else:
        raise SystemExit("\nThe trainer GPU did not become reachable within 30 minutes. "
                         f"Last SSH error: {last_ssh_error or 'none (no address yet)'}")
    say(f"   {rental_name} is up ({target})")

    # -- 3. set it up ---------------------------------------------------------------
    say("2/5 installing the trainer's software on it (PyTorch, transformers, the Basilica SDK; ~1 min)")
    setup = r'''set -euo pipefail
export PATH="$HOME/.local/bin:$PATH"
command -v uv >/dev/null || curl -LsSf https://astral.sh/uv/install.sh | sh >/dev/null
mkdir -p ~/byot && cd ~/byot
[ -x venv/bin/python ] || uv venv -q --python 3.12 venv
drv=$(nvidia-smi --query-gpu=driver_version --format=csv,noheader | head -1 | cut -d. -f1)
if [ "$drv" -ge 570 ]; then idx=cu128; else idx=cu126; fi
uv pip install -q --python venv/bin/python torch --index-url "https://download.pytorch.org/whl/$idx"
uv pip install -q --python venv/bin/python "basilica-sdk[publisher]>=0.36.7" "transformers>=4.51,<5" datasets huggingface_hub accelerate
venv/bin/python -c "import torch; assert torch.cuda.is_available(), 'no CUDA'; print('   trainer ready:', torch.cuda.device_count(), 'x', torch.cuda.get_device_name(0), 'torch', torch.__version__)"
df -h ~ | awk 'NR==2 {print "   disk free:", $4}'
'''
    # A freshly booted box can drop the first connections or hit a network
    # hiccup mid-install: the setup is idempotent, so retry it.
    for attempt in range(1, 4):
        out = subprocess.run(ssh_base(target, port) + ["bash -s"], input=setup, text=True, capture_output=True)
        if out.returncode == 0:
            break
        err = (out.stdout + out.stderr).strip()[-1500:] or f"no output (exit code {out.returncode})"
        say(f"   setup attempt {attempt}/3 failed: {err[-300:]}")
        time.sleep(20)
    else:
        raise SystemExit(f"\nSetting up the trainer failed:\n{err}")
    print(out.stdout.strip(), flush=True)
    with open(os.environ["BYOT_TRAINER_PY"]) as f:
        subprocess.run(ssh_base(target, port) + ["cat > ~/byot/byot_grpo.py"], stdin=f, check=True)
    # Optional extra source tree for the trainer (e.g. a pulse-verl checkout),
    # unpacked to ~/byot/extra and put on the trainer's PYTHONPATH.
    extra = os.environ.get("BYOT_TRAINER_EXTRA_DIR")
    if extra:
        tar = subprocess.run(["tar", "-C", extra, "--exclude", ".venv*", "--exclude", "__pycache__", "-cz", "."],
                             capture_output=True, check=True).stdout
        subprocess.run(ssh_base(target, port) + ["rm -rf ~/byot/extra && mkdir -p ~/byot/extra && tar -xz -C ~/byot/extra"],
                       input=tar, check=True)

    # -- 4. train -------------------------------------------------------------------
    replicas = os.environ.get("BYOT_REPLICAS", "2" if NGPU == 1 else "1")
    say(f"3/5 training: {STEPS} GRPO steps against a {replicas}-replica rollout session (session start ~10 min)")
    say(f"   each step: sample {os.environ.get('BYOT_PROMPTS') or 8} prompts x 8 through the session, "
        "score, update, publish a sparse patch")
    creds = "".join(f"{k}={shlex.quote(v)}\n" for k, v in {
        "BASILICA_STAGING_API_KEY": os.environ["BASILICA_STAGING_API_KEY"],
        "BASILICA_API_URL": API,
        "BYOT_BUCKET": os.environ["BYOT_BUCKET"], "BYOT_ENDPOINT": os.environ["BYOT_ENDPOINT"],
        "BYOT_ACCESS_KEY_ID": os.environ["BYOT_ACCESS_KEY_ID"],
        "BYOT_SECRET_ACCESS_KEY": os.environ["BYOT_SECRET_ACCESS_KEY"],
        "BYOT_POLICY": policy, "BYOT_TRAIN_STEPS": str(STEPS), "PYTHONUNBUFFERED": "1",
        "BYOT_RENTAL_NAME": rental_name,
        **{k: os.environ[k] for k in PASS_THROUGH if os.environ.get(k)},
    }.items())
    launch = (f"./venv/bin/torchrun --standalone --nproc-per-node {NGPU} byot_grpo.py" if NGPU > 1
              else "./venv/bin/python -u byot_grpo.py")
    # Credentials travel on stdin and live only in the job's environment. The
    # job runs detached from this SSH connection, capped at MAX_RUNTIME_HOURS,
    # and when it ends deletes the session and policy and stops its OWN rental:
    # if this machine goes away mid-run (a closed laptop, a killed terminal)
    # neither the rollout session nor the rented GPU can keep billing.
    # The job also copies run.log to your bucket (byot-runs/<rental>/run.log)
    # every 2 minutes and once at the end, so the results survive the box
    # stopping itself and any loss of this machine's connection.
    job = """set -a; . ./creds.env; set +a; rm -f ./creds.env
cat > upload_log.py <<'PY'
import os
import boto3
from botocore.config import Config
s3 = boto3.client("s3", endpoint_url=os.environ["BYOT_ENDPOINT"], region_name="auto",
                  aws_access_key_id=os.environ["BYOT_ACCESS_KEY_ID"],
                  aws_secret_access_key=os.environ["BYOT_SECRET_ACCESS_KEY"],
                  config=Config(request_checksum_calculation="when_required",
                                response_checksum_validation="when_required",
                                connect_timeout=10, read_timeout=60, retries={"max_attempts": 3}))
s3.upload_file("run.log", os.environ["BYOT_BUCKET"], f"byot-runs/{os.environ['BYOT_RENTAL_NAME']}/run.log")
PY
( while sleep 120; do ./venv/bin/python upload_log.py > /dev/null 2>&1; done ) &
UPLOADER=$!
timeout --signal=INT --kill-after=300 @MAXS@s @LAUNCH@ > run.log 2>&1
echo "BYOT-EXIT $?" >> run.log
kill $UPLOADER 2>/dev/null
./venv/bin/python upload_log.py > /dev/null 2>&1
./venv/bin/python - >> run.log 2>&1 <<'PY'
# A trainer killed at the time cap (or one without its own cleanup) leaves its
# session and policy behind: delete them here (harmless if already gone).
# Each call is retried through network blips (harmless if already gone).
import os, re, time
from basilica import BasilicaClient
rl = BasilicaClient(base_url=os.environ["BASILICA_API_URL"], api_key=os.environ["BASILICA_STAGING_API_KEY"]).rl
def gone(what, call):
    for attempt in range(1, 6):
        try:
            call()
            print(f"job cleanup: {what} deleted")
            return
        except KeyError:
            return
        except Exception as e:
            if attempt == 5:
                print(f"job cleanup: could not delete {what}: {e}")
            time.sleep(10 * attempt)
for uid in sorted(set(re.findall(r"session ([0-9a-f-]{36})", open("run.log").read()))):
    gone(f"session {uid}", lambda: rl.delete_session(uid))
gone(f"policy {os.environ['BYOT_POLICY']}", lambda: rl.delete_policy(os.environ["BYOT_POLICY"]))
PY
./venv/bin/python - >> run.log 2>&1 <<'PY'
# The box must not outlive its job: keep trying through network blips for
# about 20 minutes before giving up.
import os, time
from basilica import BasilicaClient
c = BasilicaClient(base_url=os.environ["BASILICA_API_URL"], api_key=os.environ["BASILICA_STAGING_API_KEY"])
for attempt in range(1, 13):
    try:
        s = c.stop_secure_cloud_rental(os.environ["BYOT_RENTAL_NAME"])
        print(f"self-stop: rental stopped after {s.duration_hours:.2f} h, ${s.total_cost:.2f}")
        break
    except Exception as e:
        print(f"self-stop attempt {attempt} failed: {e}", flush=True)
        time.sleep(min(180, 15 * attempt))
PY
./venv/bin/python upload_log.py > /dev/null 2>&1
""".replace("@MAXS@", str(int(MAX_RUNTIME_H * 3600))).replace("@LAUNCH@", launch)
    # Start the job in its own SSH call (retried: a freshly booted box can
    # refuse the first connections) and only then follow its log, so "never
    # started" and "started, stream dropped" can be told apart.
    start = ("umask 077; cd ~/byot && cat > creds.env && cat > job.sh <<'JOB'\n" + job + "JOB\n"
             ": > run.log; setsid nohup bash job.sh > /dev/null 2>&1 < /dev/null & echo STARTED $!")
    job_pid = None
    launched = True  # from here on a policy and session may exist (the trainer creates them)
    for attempt in range(1, 4):
        try:
            out = subprocess.run(ssh_base(target, port) + [start], input=creds, text=True,
                                 capture_output=True, timeout=120)
            m = re.search(r"^STARTED (\d+)$", out.stdout, re.M)
            if m:
                job_pid = int(m.group(1))
                break
            err = (out.stderr or out.stdout).strip()[-300:]
        except subprocess.TimeoutExpired:
            err = "timed out"
        say(f"   starting the trainer job failed (attempt {attempt}/3): {err}")
        time.sleep(15)
    if job_pid is None:
        raise SystemExit("\nCould not start the trainer job on the rented machine.")

    def job_alive():
        """True/False when the box answers, None when it cannot be reached."""
        try:
            out = subprocess.run(ssh_base(target, port) + [f"kill -0 {job_pid} 2>/dev/null && echo ALIVE || echo GONE"],
                                 text=True, capture_output=True, timeout=60)
        except subprocess.TimeoutExpired:
            return None
        return True if "ALIVE" in out.stdout else False if "GONE" in out.stdout else None

    # SSH's words for "this address now belongs to a different machine": a
    # box that stopped itself gives its IP back, and the provider can hand it
    # to someone else's machine within minutes.
    host_changed = re.compile(r"Host key verification failed|REMOTE HOST IDENTIFICATION HAS CHANGED|"
                              r"Host key for \S+ has changed")

    keep = re.compile(r"^\[ *[0-9.]+m\]|^step +[0-9]+|^usage:|^cleaned up|^session [0-9a-f-]{36}|"
                      r"^loaded |^anchor |^eval step-|^mismatch step|^rank \d+: stopping|^step gather:|"
                      r"====|reward:|numerics|async lag|time to Active|anchors published|"
                      r"patch size|revision states|usage expected|\[(PASS|FAIL)\]|ALL PASS|SOME CHECKS|FAILED:")
    tail, rc, seen, alive = [], None, 0, True
    # The whole trainer log, kept locally: the box (and its run.log) is gone
    # once it stops itself, and the filtered console output drops tracebacks.
    os.makedirs(f"{HERE}/logs", exist_ok=True)
    full_log_path = f"{HERE}/logs/{rental_name}.log"
    full_log = open(full_log_path, "a", buffering=1)
    first_tb = []  # the first Python traceback, for the failure summary
    box_gone = None  # the rental's status once the API says the box is no longer running
    lost_since, attempt = None, 0
    while True:
        attempt += 1
        follow = f"tail -n +{seen + 1} --pid={job_pid} -f ~/byot/run.log"
        proc = subprocess.Popen(ssh_base(target, port) + [follow], stdout=subprocess.PIPE,
                                stderr=subprocess.PIPE, text=True, bufsize=1)
        got = 0
        for line in proc.stdout:
            seen += 1
            got += 1
            line = line.rstrip("\n")
            full_log.write(line + "\n")
            tail = (tail + [line])[-40:]
            if (first_tb and len(first_tb) < 40) or (not first_tb and "Traceback (most recent call last)" in line):
                first_tb.append(line)
            if line.startswith("BYOT-EXIT "):
                rc = int(line.split()[1])
                continue
            if line.startswith("self-stop:"):
                rented = False  # the box already stopped itself
                m = re.search(r"after ([0-9.]+) h, \$([0-9.]+)", line)
                box_cost = (float(m.group(1)), float(m.group(2))) if m else box_cost
                say(f"   {line}")
                continue
            m = re.search(r"session ([0-9a-f-]{36})", line)
            if m:
                session_uid = m.group(1)
            if keep.search(line):
                print(line.replace("[", "   [", 1) if line.startswith("[") else line, flush=True)
        proc.wait()
        if rc is not None:
            break
        err = (proc.stderr.read() or "").strip()
        if got or lost_since is None:
            lost_since, attempt = time.time(), 1
        # Ask the API before SSH: once the box has stopped (its job ended, or
        # the time cap), its address may already be another machine's.
        try:
            r = find_rental()
        except Exception:  # noqa: BLE001 - our own network may be what is down
            r = "unknown"
        if r is None or (r != "unknown" and r.status in GONE):
            box_gone = "no longer listed" if r is None else r.status
            break
        if r != "unknown" and r.ip_address and r.ssh_command and ssh_target(r) != (target, port):
            say(f"   the trainer box now answers at {ssh_target(r)[0]} (was {target}); following it there")
            target, port = ssh_target(r)
        if host_changed.search(err):
            # The rental is still listed as running, yet the address answers
            # with another machine's key: never connect to it.
            alive = None
            err = f"the machine at {target} is not this run's box (its SSH host key changed)"
        else:
            alive = job_alive()
            if alive is False:
                break  # the job ended without its exit line
        lost_min = (time.time() - lost_since) / 60
        if lost_min >= 30:
            break
        say(f"   lost the trainer's output stream ({err[-200:] or 'no error'}); job "
            f"{'still running' if alive else 'unreachable'}, reconnecting ({lost_min:.0f} of 30 min)")
        time.sleep(min(60, 10 * attempt))  # rides out a local network drop
    if rc is None and box_gone:
        # Stopped by its own job (cleanup done) or from outside (cleanup not
        # done): either way the session and policy are deleted below, which is
        # harmless when the job already did it.
        rented = False
        say(f"4/5 the trainer box has stopped (rental {box_gone}) and this launcher missed the end of "
            "its output: the job finished and stopped its own rental, or hit the "
            f"{MAX_RUNTIME_H:g} h cap. Its full log is in your bucket (below).")
    elif rc is None:
        if alive is not False:
            # Running, or just unreachable from here (possibly OUR network): the
            # job owns its cleanup. It deletes its session and policy and stops
            # its own rental when it ends, within MAX_RUNTIME_HOURS.
            say("4/5 could not follow the trainer; it keeps running on the box and cleans up "
                f"its session, policy and rental itself when it ends (at most {MAX_RUNTIME_H:g} h)")
            say(f"   to stop it now instead:  {manual(f'stop_secure_cloud_rental({rental_name!r})')}")
            rented, job_owns_cleanup = False, True
        else:
            say("4/5 the trainer job is gone or unreachable; stopping its rental from here. Last output:")
            print("\n".join(tail[-15:]), flush=True)
    elif rc != 0:
        if first_tb:
            say(f"4/5 the trainer exited with code {rc}; first error:")
            print("\n".join(first_tb), flush=True)
        else:
            say(f"4/5 the trainer exited with code {rc}; last output:")
            print("\n".join(tail[-15:]), flush=True)
        say(f"   full trainer log: {full_log_path}")
    else:
        say("4/5 training finished")
finally:
    # Cleanup must finish even if Ctrl-C is pressed again or the terminal
    # closes: nothing may keep billing.
    for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
        signal.signal(sig, lambda *_: say("   still cleaning up so nothing keeps billing; one moment"))
    say("5/5 cleaning up")
    if globals().get("job_pid"):
        say(f"   the trainer's full log is copied to your bucket: "
            f"s3://{os.environ['BYOT_BUCKET']}/byot-runs/{rental_name}/run.log")
    if job_owns_cleanup:
        say(f"   left to the job on {rental_name}: it deletes session {session_uid} and policy {policy} "
            "and stops its own rental when it ends")
    if session_uid and not job_owns_cleanup:
        # Usage is readable only while the session exists: read it first.
        try:
            u = retrying("reading usage", lambda: rl.session_usage(session_uid))
            cost = u.get("costUsd")
            say(f"   session usage: {float(u.get('gpuHours') or 0):.3f} GPU hours, requests={u.get('requests')}, "
                f"completionTokens={u.get('completionTokens')}, "
                + (f"cost ${float(cost):.2f}" if cost is not None else "no cost figure (no rate configured here)"))
        except KeyError:
            pass  # the trainer already deleted it (and printed its usage above)
        except Exception as e:
            say(f"   session usage: could not read it ({short(e)})")
        if cleanup(f"delete session {session_uid}", lambda: rl.delete_session(session_uid),
                   f"rl.delete_session({session_uid!r})"):
            say(f"   session {session_uid} deleted (or already gone)")
    if launched and not job_owns_cleanup:
        if cleanup(f"delete policy {policy}", lambda: rl.delete_policy(policy), f"rl.delete_policy({policy!r})"):
            say(f"   policy {policy} deleted or already gone (the weights in your bucket stay yours)")
    if rented:
        def stop():
            r = find_rental()
            if r is None or r.status in GONE:
                return ("already", r)
            return client.stop_secure_cloud_rental(rental_name)
        res = cleanup(f"stop the trainer box {rental_name}", stop,
                      f"stop_secure_cloud_rental({rental_name!r})")
        if isinstance(res, tuple):
            acc = getattr(res[1], "accumulated_cost", None) if res[1] is not None else None
            say(f"   trainer box {rental_name} had already stopped" + (f" (cost ${float(acc):.2f})" if acc else ""))
        elif res is not None and hasattr(res, "total_cost"):
            box_cost = (res.duration_hours, res.total_cost)
            say(f"   trainer box {rental_name} stopped: {res.duration_hours:.2f} h, ${res.total_cost:.2f}")
    b1 = balance()
    if b0 is not None and b1 is not None:
        say(f"   account balance: ${b0:.2f} at the start, ${b1:.2f} now ({b1 - b0:+.2f})"
            + (f"; the trainer box was ${box_cost[1]:.2f} of that" if box_cost else "")
            + ". Account-wide: anything else billed to this account meanwhile (another tester on the "
            "same key) is included, and billing can lag a few minutes")
PYEOF
