"""Face-level comparison of quiddity and Analysis Situs on every MFInstSeg model.
python3 -I score.py LABELS_DIR QR.jsonl AS.jsonl OUT.json

Per comparison and tool (a failed or misaligned model counts as claiming nothing):
  recognition %   labelled instances with at least half their faces claimed
  claimed face %  labelled target faces claimed
  false pos %     claimed faces outside the target classes, of all claimed faces
  false neg %     labelled instances with no face claimed at all
"""
import gzip
import hashlib
import argparse
import json, statistics
from pathlib import Path
from collections import Counter, defaultdict

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("labels_dir")
parser.add_argument("qr_path", help="JSONL, or - for unavailable")
parser.add_argument("as_path", help="JSONL, or - for unavailable")
parser.add_argument("out_path")
parser.add_argument("--quiddity-revision", required=True)
args = parser.parse_args()
labels_dir, qr_path, as_path, out_path = args.labels_dir, args.qr_path, args.as_path, args.out_path
TOOLS = [t for t, p in (("qr", qr_path), ("as", as_path)) if p != "-"]
if not TOOLS:
    parser.error("at least one sweep is required")
NAMES = ["Chamfer", "Through hole", "Triangular passage", "Rectangular passage", "6-sided passage",
         "Triangular through slot", "Rectangular through slot", "Circular through slot",
         "Rectangular through step", "2-sided through step", "Slanted through step", "O-ring",
         "Blind hole", "Triangular pocket", "Rectangular pocket", "6-sided pocket",
         "Circular end pocket", "Rectangular blind slot", "Vertical circular end blind slot",
         "Horizontal circular end blind slot", "Triangular blind step", "Circular blind step",
         "Rectangular blind step", "Round", "Stock"]
STOCK = 24
# Families describing the body rather than a machined feature.
BODY = {"flats", "freeform_surfaces", "plates", "risers", "step_levels", "thin_wall_bodies",
        "sheet_metal_bodies", "polygonal_stock", "circular_face_patterns",
        "repeating_radial_profiles", "cylindrical_profiles", "turned_steps"}
# name: (target classes, qr families / None = every feature family / "UNCLAIMED", AS key or derived key)
COMPARISONS = {
    "Holes": (frozenset({1, 12}), {"holes"}, "holes"),
    "Through holes (class 1)": (frozenset({1}), {"holes"}, "holes"),
    "Blind holes (class 12)": (frozenset({12}), {"holes"}, "holes"),
    "Rounds": (frozenset({23}), {"blends", "fillets"}, "blends"),
    "Any feature": (frozenset(range(24)), None, "cavities"),
    "Stock (convex hull)": (frozenset({STOCK}), "UNCLAIMED", "hull"),
    "Shafts": (frozenset(), {"turned_steps"}, "shafts"),
    "Overall: holes+blends+cavities": (frozenset(range(24)), None, "UNION3"),
    "Overall: + off-hull faces": (frozenset(range(24)), None, "UNION_ALL"),
}


def load(p):
    if p == "-":
        return {}
    opener = gzip.open if p.endswith(".gz") else open
    with opener(p, "rt") as source:
        rows = [json.loads(line) for line in source if line.strip()]
    result = {r["id"]: r for r in rows}
    if len(result) != len(rows):
        raise ValueError(f"duplicate model id in {p}")
    return result


qr, asr = load(qr_path), load(as_path)
ids = sorted(set(qr) | set(asr))


def qr_claim(r, fams, classes=None):
    if fams == "UNCLAIMED":
        return set(range(r["faces"])) - qr_claim(r, None)
    claimed = set()
    for family, recs in (r.get("defining") or {}).items():
        if not (family in fams if fams is not None else family not in BODY):
            continue
        records = r["records"][family]
        if len(records) != len(recs):
            raise ValueError(f"unaligned record metadata: {r['id']}/{family}")
        for faces, record in zip(recs, records):
            if record.get("superseded_by") is not None:
                continue
            if family == "holes" and fams == {"holes"}:
                # Union of the repo's class-1 and class-12 selectors for pooled Holes.
                bottom = record["bottom"]
                selected = ((1 in classes and bottom == "through") or
                            (12 in classes and bottom in {"flat", "drill_point", "unknown"}))
                if not selected:
                    continue
            claimed.update(faces)
    return claimed


def as_claim(r, key):
    if key == "UNION3":
        return as_claim(r, "holes") | as_claim(r, "blends") | as_claim(r, "cavities")
    if key == "UNION_ALL":
        return as_claim(r, "UNION3") | (set(range(r["faces"])) - as_claim(r, "hull"))
    v = r.get(key)
    return set(v) if isinstance(v, list) else set()


def truth(i):
    lab = json.load(open(f"{labels_dir}/{i}.json"))[0][1]
    seg = [lab["seg"][str(k)] for k in range(len(lab["inst"]))]
    insts = {tuple(j for j, x in enumerate(row) if x) for row in lab["inst"]}
    return seg, [set(t) for t in insts if t]


z = lambda: defaultdict(int)
stats = {c: {t: z() for t in TOOLS} for c in COMPARISONS}
per_class = {t: defaultdict(lambda: [0, 0]) for t in TOOLS}
health = {t: Counter() for t in TOOLS}
cpu = {t: {"read": [], "recognise": []} for t in TOOLS}
for i in ids:
    seg, insts = truth(i)
    recs = {"qr": qr.get(i, {}), "as": asr.get(i, {})}
    usable = {}
    for t in TOOLS:
        r = recs[t]
        st = r.get("status", "missing")
        if st == "ok" and r.get("faces") != len(seg):
            st = "face_count_mismatch"
        if st == "ok" and t == "as" and any(not isinstance(r.get(k), list) for k in ("holes", "blends", "cavities", "hull", "shafts")):
            health[t]["recogniser_refused_or_threw"] += 1  # still scored: that key claims nothing
        health[t][st] += 1
        usable[t] = st == "ok"
        if st == "ok" and "read_cpu_s" in r:
            cpu[t]["read"].append(r["read_cpu_s"])
            cpu[t]["recognise"].append(r["recognise_cpu_s"])
    for c, (classes, fams, key) in COMPARISONS.items():
        target = {j for j, s in enumerate(seg) if s in classes}
        tinsts = [t for t in insts if seg[min(t)] in classes]
        if STOCK in classes:  # the stock is one body, not labelled instances
            tinsts = [target] if target else []
        for t in TOOLS:
            claim = set()
            if usable[t]:
                claim = qr_claim(recs[t], fams, classes) if t == "qr" else as_claim(recs[t], key)
            s = stats[c][t]
            s["labelled_faces"] += len(target)
            s["claimed_faces"] += len(claim)
            s["true_faces"] += len(claim & target)
            s["false_faces"] += len(claim - target)
            s["false_stock_faces"] += sum(seg[j] == STOCK for j in claim)
            s["instances"] += len(tinsts)
            s["recognised"] += sum(2 * len(x & claim) >= len(x) for x in tinsts)
            s["missed"] += sum(not (x & claim) for x in tinsts)
            s["models_with_claims"] += bool(claim)
            if c == "Any feature":
                for j in target:
                    per_class[t][seg[j]][0] += 1
                    per_class[t][seg[j]][1] += j in claim

pct = lambda n, d: round(100 * n / d, 2) if d else None
report = {"provenance": {
    "quiddity_revision": args.quiddity_revision,
    "sweep_sha256": {t: hashlib.sha256(Path(p).read_bytes()).hexdigest()
                     for t, p in (("qr", qr_path), ("as", as_path)) if p != "-"},
    "scorer_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    "status": "current rerun",
    "scorer": "tools/as_compare/score.py; superseded records skipped; class-1/12 bottom selectors",
    "metric": "Union face coverage: recognition >= half of instance faces; no one-to-one IoU matching. Not comparable with mfinstseg_baseline.json.",
    "any_feature_excludes": sorted(BODY),
    "timing": "quiddity: calling-thread CPU, read and recognition separately; JSON serialization excluded. Analysis Situs: process CPU; recognition includes AAG construction.",
    "shafts": "quiddity proxy is turned_steps only, never the union of feature families; no target shaft class in MFInstSeg.",
}, "models": len(ids), "health": {t: dict(h) for t, h in health.items()}, "comparisons": {}}
for c in COMPARISONS:
    report["comparisons"][c] = {}
    for t in TOOLS:
        s = stats[c][t]
        report["comparisons"][c][t] = {
            **s,
            "recognition_pct": pct(s["recognised"], s["instances"]),
            "claimed_face_pct": pct(s["true_faces"], s["labelled_faces"]),
            "false_positive_pct": pct(s["false_faces"], s["claimed_faces"]),
            "false_negative_pct": pct(s["missed"], s["instances"]),
        }
report["per_class_claimed_face_pct"] = {
    NAMES[k]: {t: pct(per_class[t][k][1], per_class[t][k][0]) for t in TOOLS}
    for k in sorted(set().union(*(set(per_class[t]) for t in TOOLS)))}
report["per_class_faces"] = {NAMES[k]: per_class[TOOLS[0]][k][0] for k in sorted(set().union(*(set(per_class[t]) for t in TOOLS)))}


def summ(v):
    v = sorted(v)
    return {"n": len(v), "total_s": round(sum(v), 1), "mean_ms": round(1e3 * statistics.mean(v), 2),
            "median_ms": round(1e3 * statistics.median(v), 2),
            "p99_ms": round(1e3 * v[int(0.99 * (len(v) - 1))], 1), "max_s": round(v[-1], 2)}


report["cpu"] = {t: {k: summ(v) for k, v in d.items() if v} for t, d in cpu.items()}
Path(out_path).write_text(json.dumps(report, indent=2) + "\n")
print(json.dumps({k: report[k] for k in ("models", "health", "cpu")}, indent=1))
for c, d in report["comparisons"].items():
    print(f"\n== {c}")
    for t in TOOLS:
        x = d[t]
        if c == "Shafts":
            print(f"  {t}: claimed faces {x['claimed_faces']} in {x['models_with_claims']} models (no shafts in MFInstSeg)"); continue
        print(f"  {t}: recognition {x['recognition_pct']}%  claimed {x['claimed_face_pct']}%  "
              f"FP {x['false_positive_pct']}%  "
              f"FN {x['false_negative_pct']}%  [instances {x['instances']}, faces {x['labelled_faces']}]")
