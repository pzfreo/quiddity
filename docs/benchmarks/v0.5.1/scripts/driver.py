"""Interleave old/new per part. usage: driver.py LIST OUT.jsonl OLD_PY NEW_PY"""
import sys, subprocess, json, os
lst, out, oldpy, newpy = sys.argv[1:5]
here = os.path.dirname(os.path.abspath(__file__))
paths = [l.strip() for l in open(lst) if l.strip()]
with open(out, "w") as f:
    for p in paths:
        for ver, py in (("old", oldpy), ("new", newpy)):
            try:
                r = subprocess.run([py, "-I", os.path.join(here, "child.py"), ver, p], capture_output=True, text=True, timeout=900)
                line = r.stdout.strip().splitlines()[-1] if r.stdout.strip() else json.dumps({"version": ver, "path": p, "ok": False, "error": f"exit {r.returncode}: {r.stderr[-200:]}"})
            except subprocess.TimeoutExpired:
                line = json.dumps({"version": ver, "path": p, "ok": False, "error": "timeout 900s"})
            f.write(line + "\n"); f.flush()
