"""Face-level comparison of quiddity and Analysis Situs on every MFInstSeg model.
python3 -I score.py LABELS_DIR QR.jsonl AS.jsonl OUT.json

Per comparison and tool (a failed or misaligned model counts as claiming nothing):
  recognition %   labelled instances with at least half their faces claimed
  claimed face %  labelled target faces claimed
  false pos %     claimed faces outside the target classes, of all claimed faces
  false neg %     labelled instances with no face claimed at all
"""
import json, statistics, sys
from collections import Counter, defaultdict

labels_dir, qr_path, as_path, out_path = sys.argv[1:5]
NAMES = ["Chamfer", "Through hole", "Triangular passage", "Rectangular passage", "6-sided passage",
         "Triangular through slot", "Rectangular through slot", "Circular through slot",
         "Rectangular through step", "2-sided through step", "Slanted through step", "O-ring",
         "Blind hole", "Triangular pocket", "Rectangular pocket", "6-sided pocket",
         "Circular end pocket", "Rectangular blind slot", "Vertical circular end blind slot",
         "Horizontal circular end blind slot", "Triangular blind step", "Circular blind step",
         "Rectangular blind step", "Round", "Stock"]
STOCK, ORING = 24, 11
# Families describing the body rather than a machined feature.
BODY = {"flats", "freeform_surfaces", "plates", "risers", "step_levels", "thin_wall_bodies",
        "sheet_metal_bodies", "polygonal_stock", "circular_face_patterns",
        "repeating_radial_profiles", "cylindrical_profiles", "turned_steps"}
# name: (target classes, qr families / None = every feature family / "UNCLAIMED", AS key or derived key)
COMPARISONS = {
    "Holes": (frozenset({1, 12}), {"holes"}, "holes"),
    "Rounds": (frozenset({23}), {"blends", "fillets"}, "blends"),
    "Any feature": (frozenset(range(24)), None, "cavities"),
    "Stock (convex hull)": (frozenset({STOCK}), "UNCLAIMED", "hull"),
    "Shafts": (frozenset(), None, "shafts"),
    "Overall: holes+blends+cavities": (frozenset(range(24)), None, "UNION3"),
    "Overall: + off-hull faces": (frozenset(range(24)), None, "UNION_ALL"),
}


def load(p):
    return {r["id"]: r for r in map(json.loads, open(p))}


qr, asr = load(qr_path), load(as_path)
ids = sorted(set(qr) | set(asr))


def qr_claim(r, fams):
    if fams == "UNCLAIMED":
        return set(range(r["faces"])) - qr_claim(r, None)
    d = r.get("defining") or {}
    return {f for k, recs in d.items() if (k in fams if fams else k not in BODY)
            for rec in recs for f in rec}


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
stats = {c: {t: z() for t in ("qr", "as")} for c in COMPARISONS}
per_class = {t: defaultdict(lambda: [0, 0]) for t in ("qr", "as")}
health = {t: Counter() for t in ("qr", "as")}
cpu = {t: {"read": [], "recognise": []} for t in ("qr", "as")}
for i in ids:
    seg, insts = truth(i)
    recs = {"qr": qr.get(i, {}), "as": asr.get(i, {})}
    usable = {}
    for t, r in recs.items():
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
        for t in ("qr", "as"):
            claim = set()
            if usable[t]:
                claim = qr_claim(recs[t], fams) if t == "qr" else as_claim(recs[t], key)
            s = stats[c][t]
            s["labelled_faces"] += len(target)
            s["claimed_faces"] += len(claim)
            s["true_faces"] += len(claim & target)
            s["false_faces"] += len(claim - target)
            s["false_faces_oring_neutral"] += sum(seg[j] not in classes and seg[j] != ORING for j in claim)
            s["claimed_faces_oring_neutral"] += sum(seg[j] != ORING or j in target for j in claim)
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
report = {"models": len(ids), "health": {t: dict(h) for t, h in health.items()}, "comparisons": {}}
for c in COMPARISONS:
    report["comparisons"][c] = {}
    for t in ("qr", "as"):
        s = stats[c][t]
        report["comparisons"][c][t] = {
            **s,
            "recognition_pct": pct(s["recognised"], s["instances"]),
            "claimed_face_pct": pct(s["true_faces"], s["labelled_faces"]),
            "false_positive_pct": pct(s["false_faces"], s["claimed_faces"]),
            "false_positive_pct_oring_neutral": pct(s["false_faces_oring_neutral"], s["claimed_faces_oring_neutral"]),
            "false_negative_pct": pct(s["missed"], s["instances"]),
        }
report["per_class_claimed_face_pct"] = {
    NAMES[k]: {t: pct(per_class[t][k][1], per_class[t][k][0]) for t in ("qr", "as")}
    for k in sorted(per_class["qr"])}
report["per_class_faces"] = {NAMES[k]: per_class["qr"][k][0] for k in sorted(per_class["qr"])}


def summ(v):
    v = sorted(v)
    return {"n": len(v), "total_s": round(sum(v), 1), "mean_ms": round(1e3 * statistics.mean(v), 2),
            "median_ms": round(1e3 * statistics.median(v), 2),
            "p99_ms": round(1e3 * v[int(0.99 * (len(v) - 1))], 1), "max_s": round(v[-1], 2)}


report["cpu"] = {t: {k: summ(v) for k, v in d.items() if v} for t, d in cpu.items()}
json.dump(report, open(out_path, "w"), indent=2)
print(json.dumps({k: report[k] for k in ("models", "health", "cpu")}, indent=1))
for c, d in report["comparisons"].items():
    print(f"\n== {c}")
    for t in ("qr", "as"):
        x = d[t]
        if c == "Shafts":
            print(f"  {t}: claimed faces {x['claimed_faces']} in {x['models_with_claims']} models (no shafts in MFInstSeg)"); continue
        print(f"  {t}: recognition {x['recognition_pct']}%  claimed {x['claimed_face_pct']}%  "
              f"FP {x['false_positive_pct']}% (O-ring neutral {x['false_positive_pct_oring_neutral']}%)  "
              f"FN {x['false_negative_pct']}%  [instances {x['instances']}, faces {x['labelled_faces']}]")
