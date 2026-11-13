"""Render a committed aggregate score file; no uncommitted sweep inputs are needed.
python3 -I tools/as_compare/build_report.py SCORES.json OUT.html
"""
import html
import json
from pathlib import Path
import sys

scores, out = sys.argv[1:3]
# The committed raw sweep the scores came from, named in the report (optional).
sweep = sys.argv[3] if len(sys.argv) > 3 else None
e = html.escape
metrics = ['recognition_pct', 'claimed_face_pct', 'false_positive_pct', 'false_negative_pct']
names = {'qr': 'quiddity', 'as': 'Analysis Situs'}


def number(value):
    return '—' if value is None else f'{value:.2f}'


data = json.loads(Path(scores).read_text())
tools = [t for t in ('qr', 'as') if any(t in v for v in data['comparisons'].values())]
rows = []
for comparison, by_tool in data['comparisons'].items():
    for tool, counts in by_tool.items():
        rows.append('<tr><td>' + e(comparison) + '</td><td>' + names[tool] + '</td>' +
                    ''.join('<td>' + number(counts.get(k)) + '</td>' for k in metrics) +
                    f'<td>{counts.get("claimed_faces", "—")}</td></tr>')
class_rows = []
for name, values in data.get('per_class_claimed_face_pct', {}).items():
    class_rows.append('<tr><td>' + e(name) + '</td><td>' + str(data['per_class_faces'][name]) +
                      '</td>' + ''.join('<td>' + number(values.get(t)) + '</td>' for t in tools) +
                      '</tr>')
timing = []
for tool, stages in data.get('cpu', {}).items():
    for stage, values in stages.items():
        timing.append(f'<tr><td>{names[tool]}</td><td>{e(stage)}</td><td>{values["n"]}</td>'
                      f'<td>{values["mean_ms"]}</td><td>{values["median_ms"]}</td>'
                      f'<td>{values["p99_ms"]}</td><td>{values["max_s"]}</td></tr>')
provenance = data.get('provenance', {})
revision = provenance.get('quiddity_revision', 'unknown')

Path(out).write_text(f'''<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MFInstSeg face coverage</title>
<style>body{{font:16px/1.5 system-ui;margin:2rem auto;padding:0 1rem;max-width:75rem}}
table{{border-collapse:collapse;font-size:0.85rem}}td,th{{padding:.4rem;border:1px solid #aaa;text-align:right}}
td:first-child,th:first-child{{text-align:left}}.scroll{{overflow:auto}}pre{{white-space:pre-wrap}}h2{{margin-top:2rem}}</style>
</head><body><h1>MFInstSeg face coverage: quiddity {e(revision[:8])}</h1>
<p>Generated from <code>{e(Path(scores).name)}</code>: {data['models']:,} models from the MFInstSeg
test split. Health: <code>{e(json.dumps(data['health']))}</code>. {f'The raw sweep is committed as <code>{e(sweep)}</code>; its' if sweep else 'The raw sweep&#39;s'} SHA-256, the scorer's
SHA-256 and the external-label fingerprint are in the provenance below.</p>
<p>Recognition here means a labelled instance has at least half its faces in a tool's union
of claimed faces, regardless of predicted instance identity. No-face instances have no face claimed.
Claimed faces measures target-face recall; false positives are claims outside the target classes
as a share of all claims. Dimensions and feature typing are not evaluated by this union metric.</p>
<p><strong>These numbers are not comparable with <code>mfinstseg_baseline.json</code>.</strong>
The repo scorer matches instances one to one at IoU ≥ 0.5; its <code>any_feature</code> excludes
only <code>step_levels</code> and <code>risers</code>. This comparison's Any feature excludes twelve
families: flats, freeform_surfaces, plates, risers, step_levels, thin_wall_bodies, sheet_metal_bodies,
polygonal_stock, circular_face_patterns, repeating_radial_profiles, cylindrical_profiles, turned_steps.</p>
<p>Scoring skips <code>superseded_by</code> records, retains <code>source</code> records,
and selects class-1 holes with <code>bottom == through</code>; class 12 selects flat, drill_point
or unknown bottoms. Pooled Holes takes their union. Stock uses quiddity's unclaimed faces (the
complement of the selected union) as a proxy. Shafts uses only <code>turned_steps</code>; MFInstSeg
has no shaft target class, so that row counts claims outside the target taxonomy, not geometric errors.</p>
<details><summary>Provenance</summary><pre>{e(json.dumps(provenance, indent=2))}</pre></details>
<div class="scroll"><table><tr><th>Comparison</th><th>Tool</th><th>Recognition %</th>
<th>Claimed faces %</th><th>False positives %</th><th>No-face instances %</th><th>Claimed faces</th></tr>
{''.join(rows)}</table></div>
<h2>Any feature: claimed faces by class</h2><div class="scroll"><table>
<tr><th>Class</th><th>Labelled faces</th>{''.join('<th>' + names[t] + ' %</th>' for t in tools)}</tr>
{''.join(class_rows)}</table></div>
<h2>Thread CPU time</h2>
<p>Calling-thread CPU time per model, read and recognition measured separately; JSON
serialization is excluded. Thread CPU time is a weaker measure than instructions retired: it
varies with the machine, its load and the run. These figures describe this run only and are not
a speed comparison with any other tool.</p>
<div class="scroll"><table><tr><th>Tool</th><th>Stage</th><th>Models</th>
<th>Mean ms</th><th>Median ms</th><th>p99 ms</th><th>Max s</th></tr>{''.join(timing)}</table></div>
</body></html>''')
