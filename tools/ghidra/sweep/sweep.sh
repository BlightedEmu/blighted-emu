#!/usr/bin/env bash
# Resume the read-only Ghidra sweep until every function is written.

set -u

SWEEP_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SWEEP_DIR/../../.." && pwd)"
# The Ghidra install and the analysed project are on the user's machine.
GHIDRA="${GHIDRA_DIR:?set GHIDRA_DIR to the Ghidra install folder}"
PROJECT_DIR="${NW_GHIDRA_PROJECT_DIR:?set NW_GHIDRA_PROJECT_DIR to the Ghidra project folder}"
PROJECT_NAME="${NW_GHIDRA_PROJECT_NAME:-NewWorld25810808}"
PROGRAM_NAME="NewWorld.exe"
# The sweep output is decompiled client code: it stays in the ignored local-data folder.
OUTDIR="${NW_SWEEP_OUTDIR:-$REPO/local-data/re-sweep}"
SCRIPT_DIR="$REPO/tools/ghidra/scripts"
SWEEP_SCRIPT="FullSweep.java"

INITIAL_BATCH=20000
MIN_BATCH=500
GOOD_BATCHES_TO_RESTORE=3
THREAD_COUNT=10
MEM_MIN_KB=$((20 * 1024 * 1024))
MEM_WAIT_SECONDS=60
SETTLE_SECONDS=20
HEAP="16G"

batch_size=$INITIAL_BATCH
good_streak=0

mkdir -p "$OUTDIR/funcs" "$OUTDIR/logs"

if [[ ! -f "$OUTDIR/run.json" ]]; then
    OUTDIR="$OUTDIR" python3 - << 'PY'
import datetime
import json
import os
from pathlib import Path
path = Path(os.environ["OUTDIR"]) / "run.json"
stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
path.write_text(json.dumps({"started_at": stamp}) + "\n", encoding="utf-8")
PY
fi

mem_available_kb() {
    awk '/MemAvailable:/ { print $2 }' /proc/meminfo
}

wait_for_memory() {
    while true; do
        local available
        available=$(mem_available_kb)
        if [[ -n "$available" && "$available" -ge "$MEM_MIN_KB" ]]; then
            echo "memory_ok_kb=$available"
            return 0
        fi
        echo "memory_low_kb=${available:-0} need_kb=$MEM_MIN_KB"
        sleep "$MEM_WAIT_SECONDS"
    done
}

read_progress() {
    OUTDIR="$OUTDIR" python3 - << 'PY'
import json
import os
import sys
from pathlib import Path
out = Path(os.environ["OUTDIR"])
next_index = 0
total = 0
meta = out / "meta.json"
checkpoint = out / "checkpoint.json"
if meta.is_file():
    try:
        total = int(json.loads(meta.read_text(encoding="utf-8")).get("function_count") or 0)
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print("bad meta.json: {}".format(exc), file=sys.stderr)
        sys.exit(1)
if checkpoint.is_file():
    try:
        data = json.loads(checkpoint.read_text(encoding="utf-8"))
    except (OSError, ValueError, json.JSONDecodeError) as exc:
        print("bad checkpoint.json: {}".format(exc), file=sys.stderr)
        sys.exit(1)
    next_index = int(data.get("next_index") or 0)
    if data.get("total"):
        total = int(data["total"])
print("{} {}".format(next_index, total))
PY
}

skip_index() {
    local start="$1"
    OUTDIR="$OUTDIR" START="$start" python3 - << 'PY'
import datetime
import json
import os
from pathlib import Path
out = Path(os.environ["OUTDIR"])
start = int(os.environ["START"])
checkpoint = out / "checkpoint.json"
meta = out / "meta.json"
total = 0
if checkpoint.is_file():
    data = json.loads(checkpoint.read_text(encoding="utf-8"))
    total = int(data.get("total") or 0)
elif meta.is_file():
    total = int(json.loads(meta.read_text(encoding="utf-8")).get("function_count") or 0)
stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
body = {
    "next_index": start + 1,
    "total": total,
    "done_at": stamp,
}
tmp = checkpoint.with_suffix(".json.tmp")
tmp.write_text(json.dumps(body) + "\n", encoding="utf-8")
os.replace(tmp, checkpoint)
with (out / "skipped.txt").open("a", encoding="utf-8") as handle:
    handle.write("{}\n".format(start))
PY
}

log_progress() {
    local next_index="$1"
    local total="$2"
    local advanced="$3"
    local elapsed="$4"
    OUTDIR="$OUTDIR" NEXT="$next_index" TOTAL="$total" ADVANCED="$advanced" ELAPSED="$elapsed" python3 - << 'PY'
import datetime
import os
from pathlib import Path
out = Path(os.environ["OUTDIR"])
next_index = int(os.environ["NEXT"])
total = int(os.environ["TOTAL"])
advanced = int(os.environ["ADVANCED"])
elapsed = int(os.environ["ELAPSED"])
if elapsed < 1:
    elapsed = 1
fps = advanced / elapsed
percent = 0.0
if total > 0:
    percent = 100.0 * next_index / total
remain = max(total - next_index, 0)
if fps > 0 and total > 0:
    eta = "{:.2f}h".format(remain / fps / 3600.0)
else:
    eta = "unknown"
stamp = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
line = "{} {} {} {:.2f} {:.2f} {}\n".format(stamp, next_index, total, percent, fps, eta)
with (out / "progress.log").open("a", encoding="utf-8") as handle:
    handle.write(line)
print(line, end="")
PY
}

run_batch() {
    local start="$1"
    local count="$2"
    local log_file="$OUTDIR/logs/batch-${start}.log"
    MAXMEM="$HEAP" \
    GHIDRA_MAXMEM="$HEAP" \
    GHIDRA_HEADLESS_MAXMEM="$HEAP" \
    nice -n 10 ionice -c3 \
        "$GHIDRA/support/analyzeHeadless" \
        "$PROJECT_DIR" "$PROJECT_NAME" \
        -process "$PROGRAM_NAME" -noanalysis -readOnly \
        -scriptPath "$SCRIPT_DIR" \
        -postScript "$SWEEP_SCRIPT" "$OUTDIR" "$start" "$count" "$THREAD_COUNT" \
        > "$log_file" 2>&1
    return $?
}

# A batch failed when the JVM exits non-zero or the checkpoint does not move.
batch_failed() {
    local before="$1"
    local after="$2"
    local exit_code="$3"
    if [[ "$exit_code" -ne 0 || "$after" -le "$before" ]]; then
        return 0
    fi
    return 1
}

note_failure() {
    local start="$1"
    local exit_code="$2"
    local reason="$3"
    printf '%s index=%s exit=%s reason=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$start" "$exit_code" "$reason" \
        >> "$OUTDIR/logs/failures.log"
}

finish() {
    echo "sweep_complete"
    python3 "$SWEEP_DIR/resolve_unknowns.py" "$OUTDIR" || exit 1
    python3 "$SWEEP_DIR/make_notes.py" "$OUTDIR" || exit 1
    date -u +"%Y-%m-%dT%H:%M:%SZ" > "$OUTDIR/COMPLETE"
    exit 0
}

while true; do
    progress=$(read_progress) || exit 1
    next_index=${progress%% *}
    total=${progress##* }
    if [[ "$total" -gt 0 && "$next_index" -ge "$total" ]]; then
        finish
    fi
    wait_for_memory
    before=$next_index
    count=$batch_size
    if [[ "$total" -gt 0 && $((before + count)) -gt "$total" ]]; then
        count=$((total - before))
    fi
    if [[ "$count" -lt 1 ]]; then
        finish
    fi
    log_file="$OUTDIR/logs/batch-${before}.log"
    start_s=$(date +%s)
    run_batch "$before" "$count"
    exit_code=$?
    if [[ -f "$log_file" ]] && grep -q 'Unable to lock project' "$log_file"; then
        echo "project_locked index=$before"
        note_failure "$before" "$exit_code" "lock"
        sleep "$MEM_WAIT_SECONDS"
        continue
    fi
    end_s=$(date +%s)
    progress=$(read_progress) || exit 1
    after=${progress%% *}
    total=${progress##* }
    elapsed=$((end_s - start_s))
    if batch_failed "$before" "$after" "$exit_code"; then
        note_failure "$before" "$exit_code" "first"
        echo "batch_failed index=$before size=$batch_size exit=$exit_code"
        run_batch "$before" "$count"
        retry_code=$?
        progress=$(read_progress) || exit 1
        after=${progress%% *}
        total=${progress##* }
        end_s=$(date +%s)
        elapsed=$((end_s - start_s))
        if batch_failed "$before" "$after" "$retry_code"; then
            note_failure "$before" "$retry_code" "retry"
            good_streak=0
            if [[ "$batch_size" -gt "$MIN_BATCH" ]]; then
                batch_size=$((batch_size / 2))
                if [[ "$batch_size" -lt "$MIN_BATCH" ]]; then
                    batch_size=$MIN_BATCH
                fi
                echo "batch_halved size=$batch_size"
            else
                echo "skip_index=$before"
                skip_index "$before" || exit 1
                batch_size=$INITIAL_BATCH
                progress=$(read_progress) || exit 1
                after=${progress%% *}
                total=${progress##* }
            fi
        else
            good_streak=1
        fi
    else
        good_streak=$((good_streak + 1))
        if [[ "$good_streak" -ge "$GOOD_BATCHES_TO_RESTORE" ]]; then
            batch_size=$INITIAL_BATCH
        fi
    fi
    advanced=$((after - before))
    if [[ "$advanced" -lt 0 ]]; then
        advanced=0
    fi
    log_progress "$after" "$total" "$advanced" "$elapsed" || exit 1
    sleep "$SETTLE_SECONDS"
done
