"""One part, one quiddity version: warm-process timing (median of 3) and peak RSS.
usage: python child.py {old|new} PATH   -> one JSON line"""
import sys, time, json, resource, statistics
ver, path = sys.argv[1], sys.argv[2]
t0 = time.perf_counter()
import quiddity
if ver == "old":
    from quiddity import import_step_geometry, build_framed_recognition_result, FramedRecognitionResult
import_s = time.perf_counter() - t0
rss0 = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss

def counts_old(framed):
    out = {}
    res = framed.result
    import dataclasses
    for f in dataclasses.fields(res):
        v = getattr(res, f.name)
        if isinstance(v, (list, tuple)) and v:
            out[f.name] = len(v)
    return out

def run_old():
    a = time.perf_counter()
    part = import_step_geometry(path)
    b = time.perf_counter()
    framed = build_framed_recognition_result(part)
    c = time.perf_counter()
    ok = isinstance(framed, FramedRecognitionResult)
    return b - a, c - b, (counts_old(framed) if ok else None), ok

def run_new():
    a = time.perf_counter()
    text = quiddity.recognise(path)
    b = time.perf_counter()
    doc = json.loads(text)["document"]
    c = {}
    for r in doc["records"]:
        c[r["family"]] = c.get(r["family"], 0) + 1
    return None, b - a, c, True

fn = run_old if ver == "old" else run_new
rec = {"version": ver, "path": path, "import_s": import_s}
try:
    runs = [fn() for _ in range(3)]
    tot = [(r[0] or 0) + r[1] for r in runs]
    rec.update(ok=runs[0][3], total_s=statistics.median(tot), runs_s=tot,
               read_s=(statistics.median([r[0] for r in runs]) if runs[0][0] is not None else None),
               recog_s=statistics.median([r[1] for r in runs]), counts=runs[0][2])
except Exception as e:
    rec.update(ok=False, error=f"{type(e).__name__}: {str(e)[:200]}")
rec["peak_rss_kb"] = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
rec["rss_after_import_kb"] = rss0
print(json.dumps(rec))
