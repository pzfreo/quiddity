"""Run as_recognise on every id in IDS_FILE: python3 -I run_as.py STEPS IDS OUT.jsonl WORKERS AS_RECOGNISE."""
import json, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor
steps, ids_file, out_path, workers, driver = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4]), sys.argv[5]
ids = [l.strip() for l in open(ids_file) if l.strip()]
def one(i):
    t = time.time()
    try:
        p = subprocess.run([driver, i, f"{steps}/{i}.step"],
                           capture_output=True, text=True, timeout=300)
        lines = [l for l in p.stdout.splitlines() if l.startswith("{")]
        rec = json.loads(lines[-1]) if lines else {"id": i, "status": f"crashed:{p.returncode}"}
    except subprocess.TimeoutExpired:
        rec = {"id": i, "status": "timeout"}
    rec["seconds"] = time.time() - t
    return rec
with open(out_path, "w") as out, ThreadPoolExecutor(workers) as pool:
    for n, rec in enumerate(pool.map(one, ids)):
        out.write(json.dumps(rec) + "\n"); out.flush()
        if n % 1000 == 0: print(n, file=sys.stderr, flush=True)
