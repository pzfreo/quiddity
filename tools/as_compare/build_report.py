"""Render committed aggregate scores; no uncommitted sweep inputs are needed.
python3 -I tools/as_compare/build_report.py HISTORICAL.json FULL.json CURRENT.json OUT.html
Use - for CURRENT.json to render only historical results.
"""
import html
import json
from pathlib import Path
import sys

historical, full, current, out = sys.argv[1:5]
e = html.escape
metrics = ['recognition_pct', 'claimed_face_pct', 'false_positive_pct', 'false_negative_pct']

def number(value):
    return '—' if value is None else f'{value:.2f}'

def section(path, title):
    data = json.loads(Path(path).read_text())
    rows = []
    for comparison, tools in data['comparisons'].items():
        for tool, counts in tools.items():
            rows.append('<tr><td>' + e(comparison) + '</td><td>' +
                        ('quiddity' if tool == 'qr' else 'Analysis Situs') + '</td>' +
                        ''.join('<td>' + number(counts.get(k)) + '</td>' for k in metrics) +
                        f'<td>{counts.get("claimed_faces", "—")}</td></tr>')
    timing = []
    for tool, stages in data.get('cpu', {}).items():
        for stage, values in stages.items():
            timing.append(f'<tr><td>{e(tool)}</td><td>{e(stage)}</td><td>{values["n"]}</td>'
                          f'<td>{values["mean_ms"]}</td><td>{values["median_ms"]}</td>'
                          f'<td>{values["p99_ms"]}</td><td>{values["max_s"]}</td></tr>')
    class_rows = []
    for name, values in data.get('per_class_claimed_face_pct', {}).items():
        class_rows.append('<tr><td>' + e(name) + '</td><td>' +
                          str(data['per_class_faces'][name]) + '</td>' +
                          ''.join('<td>' + number(values.get(t)) + '</td>' for t in ('qr', 'as')) + '</tr>')
    provenance = e(json.dumps(data.get('provenance', {}), indent=2))
    return f'''<section><h2>{e(title)}</h2><p>{data['models']:,} models.
    Input: <code>{e(Path(path).name)}</code>. Health: <code>{e(json.dumps(data['health']))}</code>.</p>
    <details><summary>Provenance and limitations</summary><pre>{provenance}</pre></details>
    <div class="scroll"><table><tr><th>Comparison</th><th>Tool</th><th>Recognition %</th>
    <th>Claimed faces %</th><th>False positives %</th><th>No-face instances %</th><th>Claimed faces</th></tr>
    {''.join(rows)}</table></div>
    <h3>Any feature: claimed faces by class</h3><div class="scroll"><table>
    <tr><th>Class</th><th>Labelled faces</th><th>quiddity %</th><th>Analysis Situs %</th></tr>
    {''.join(class_rows)}</table></div>
    <h3>CPU timing</h3><div class="scroll"><table><tr><th>Tool</th><th>Stage</th><th>Models</th>
    <th>Mean ms</th><th>Median ms</th><th>p99 ms</th><th>Max s</th></tr>{''.join(timing)}</table></div></section>'''

content = section(historical, 'Historical: quiddity 0.4.8 (82bb3ba) vs Analysis Situs (246e2ab)')
content += section(full, 'Historical: quiddity 0.4.8, full corpus; earlier scorer')
if current != '-':
    data = json.loads(Path(current).read_text())
    content += section(current, 'Current quiddity rerun: ' + data['provenance']['quiddity_revision'])

Path(out).write_text(f'''<!doctype html><html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>MFInstSeg: quiddity and Analysis Situs measurements</title>
<style>body{{font:16px/1.5 system-ui;margin:2rem auto;padding:0 1rem;max-width:75rem}}
table{{border-collapse:collapse;font-size:0.85rem}}td,th{{padding:.4rem;border:1px solid #aaa;text-align:right}}
td:first-child,th:first-child{{text-align:left}}.scroll{{overflow:auto}}pre{{white-space:pre-wrap}}h2{{margin-top:2rem}}</style>
</head><body><h1>MFInstSeg: quiddity and Analysis Situs measurements</h1>
<p>The committed 0.4.8 results predate PRs #216 (flat floors and through-hole selector) and
#217 (superseded groove records). They are historical, not measurements of current main.
Analysis Situs was not rerun locally. Only the quiddity test split was rerun; the full-corpus results remain historical.</p>
<p>Recognition here means a labelled instance has at least half its faces in a tool's union
of claimed faces, regardless of predicted instance identity. No-face instances have no face claimed.
Claimed faces measures target-face recall; false positives are claims outside the target classes
as a share of all claims. Dimensions and feature typing are not evaluated by this union metric.</p>
<p><strong>These numbers are not comparable with <code>mfinstseg_baseline.json</code>.</strong>
The repo scorer matches instances one to one at IoU ≥ 0.5; its <code>any_feature</code> excludes
only <code>step_levels</code> and <code>risers</code>. This comparison's Any feature excludes twelve
families: flats, freeform_surfaces, plates, risers, step_levels, thin_wall_bodies, sheet_metal_bodies,
polygonal_stock, circular_face_patterns, repeating_radial_profiles, cylindrical_profiles, turned_steps.</p>
<p>Current scoring skips <code>superseded_by</code> records, retains <code>source</code> records,
and selects class-1 holes with <code>bottom == through</code>; class 12 selects flat, drill_point
or unknown bottoms. Pooled Holes takes their union. Analysis Situs's hole output is untyped and
is used unchanged in both class rows. No O-ring faces are neutralized.</p>
<p>The historical quiddity Shafts row was invalid: it scored the union of all feature claims
against an empty target. That row is removed because the raw sweep is unavailable. The current
Shafts row uses only <code>turned_steps</code> as a proxy. MFInstSeg has no shaft target class,
so this is a count of claims outside the target taxonomy, not evidence of geometric errors.</p>
<p>Timing scopes differ: quiddity measures calling-thread CPU; Analysis Situs measures process
CPU, including any worker threads used by the STEP reader. Analysis Situs recognition includes
AAG construction. Current quiddity recognition excludes JSON serialization; the historical
example included serialization in its recognition interval. These are different workloads and
clock scopes, not a controlled speed comparison.</p>
{content}
<h2>Reproduction limits</h2><p>This HTML is generated from committed aggregate JSON files.
Historical raw sweeps are not committed, so the old scores cannot be reproduced from this branch.
The full-corpus historical JSON used an earlier scorer. The October alpha.1 JSON is retained
as an archival result with a different schema; no cross-run output-identity claim is made.</p>
<p>Fresh scores require the external MFInstSeg STEP and label files and raw tool sweeps.
The README documents commands and the current run's provenance. The current test-split sweep is
committed as <code>tools/as_compare/runs/main-1b7dee67-test-qr.jsonl.gz</code> (about 1 MB),
with sweep, scorer and external-label digests in its aggregate JSON. Its records include 2,637
superseded holes and 2,637 superseded bosses; current scoring skips these records. Small synthetic
scoring inputs and semantic tests are also committed.</p>
</body></html>''')
