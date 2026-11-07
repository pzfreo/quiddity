"""Build the quiddity vs Analysis Situs HTML report.
python3 -I build_report.py SCORE.json FULL_CORPUS_SCORE.json ALPHA1_SCORE.json TEST_IDS QR.jsonl AS.jsonl OUT.html
SCORE is score.py on the test split, FULL_CORPUS_SCORE on every model; ALPHA1_SCORE is October's run.
"""
import json, statistics as st, html, sys
score, full_score, alpha1, test_ids, qr_path, as_path, out_path = sys.argv[1:8]
d=json.load(open(score)); full=json.load(open(full_score))
old=json.load(open(alpha1))['comparisons']
ids=set(open(test_ids).read().split())
q={r['id']:r for r in map(json.loads,open(qr_path)) if r['id'] in ids}
a={r['id']:r for r in map(json.loads,open(as_path))}
assert set(a)==set(q)==ids
A5=list(a.values())
def tm(dd,f):
    v=sorted(f(r) for r in dd.values()); return (1e3*st.mean(v),1e3*st.median(v),1e3*v[int(.99*(len(v)-1))],v[-1])
T={t:{'read':tm(dd,lambda r:r['read_cpu_s']),'rec':tm(dd,lambda r:r['recognise_cpu_s']),'tot':tm(dd,lambda r:r['read_cpu_s']+r['recognise_cpu_s'])} for t,dd in (('qr',q),('as',a))}
T['as']['rec3']=tm(a,lambda r:r['recognise3_cpu_s']); T['qr']['rec3']=T['qr']['rec']
T['as']['tot3']=tm(a,lambda r:r['read_cpu_s']+r['recognise3_cpu_s']); T['qr']['tot3']=T['qr']['tot']
stage={k:1e3*st.mean(r[k] for r in A5) for k in ('aag_s','holes_s','blends_s','cavities_s','hull_s','shafts_s')}
e=html.escape
METRICS=[('recognition_pct','Recognition','features with ≥ half their faces claimed',True),
         ('claimed_face_pct','Claimed faces','labelled feature faces claimed',True),
         ('false_positive_pct','False positives','claimed faces that are not this feature',False),
         ('false_negative_pct','False negatives','features with no face claimed at all',False)]
def bar(v,cls,label):
    return f'<div class="b"><span class="bt {cls}" style="width:{v:.2f}%" title="{e(label)}: {v:.1f}%"></span><span class="bv">{v:.1f}%</span></div>'
def block(name,blurb,note='',title=None,metrics=None,instances=True):
    c=d['comparisons'][name]; rows=''
    for k,lab,desc,hi in (metrics or METRICS):
        qv,av=c['qr'][k],c['as'][k]; best='qr' if (qv>av)==hi else 'as'
        extra=''
        if k=='false_positive_pct' and name=='Holes':
            extra=f'<div class="sub2">O-ring faces neutral: quiddity {c["qr"]["false_positive_pct_oring_neutral"]:.1f}% · Analysis Situs {c["as"]["false_positive_pct_oring_neutral"]:.1f}%</div>'
        rows+=f'''<div class="m"><div class="ml"><b>{lab}</b><small>{desc} · {"higher" if hi else "lower"} is better</small></div>
<div class="mv"><div class="row{' win' if best=='qr' else ''}"><span class="who">quiddity</span>{bar(qv,'q',f'quiddity {lab}')}</div>
<div class="row{' win' if best=='as' else ''}"><span class="who">Analysis Situs</span>{bar(av,'a',f'Analysis Situs {lab}')}</div>{extra}</div></div>'''
    n=c['qr']
    cnt=f'{n["instances"]:,} labelled features · {n["labelled_faces"]:,} faces' if instances else f'{n["labelled_faces"]:,} labelled faces'
    return f'<section><h2>{title or name}</h2><p class="lead">{blurb}</p><p class="cnt">{cnt}</p>{rows}{note}</section>'
pc=d['per_class_claimed_face_pct']; pf=d['per_class_faces']
pcrows=''.join(f'<tr><td>{e(k)}</td><td class="n">{pf[k]:,}</td><td>{bar(v["qr"],"q","quiddity "+k)}</td><td>{bar(v["as"],"a","Analysis Situs "+k)}</td></tr>' for k,v in pc.items())
def trow(lab,key):
    qm,am=T['qr'][key],T['as'][key]
    return f'<tr><td>{lab}</td><td class="n">{qm[0]:.1f} ms</td><td class="n">{am[0]:.1f} ms</td><td class="n">{qm[1]:.1f} / {am[1]:.1f}</td><td class="n">{qm[2]:.0f} / {am[2]:.0f}</td></tr>'
fc=full['comparisons']
fullrows=''.join(f'<tr><td>{c}</td>'+''.join(f'<td class="n">{fc[c]["qr"][k]:.1f}%</td><td class="n dim">{d["comparisons"][c]["qr"][k]:.1f}%</td>' for k,_,_,_ in METRICS)+'</tr>' for c in fc)
oc=[('Holes','holes'),('Rounds','rounds'),('Any feature','any-feature (cavities)')]
octrows=''.join(f'<tr><td>{n}</td><td class="n">{100*old[k]["qr"]["face_recall"]:.1f}% → {d["comparisons"][n]["qr"]["claimed_face_pct"]:.1f}%</td><td class="n">{100-100*old[k]["qr"]["face_precision"]:.1f}% → {d["comparisons"][n]["qr"]["false_positive_pct"]:.1f}%</td></tr>' for n,k in oc)
op=json.load(open(alpha1))['comparisons']['any-feature (cavities)']['qr']['per_class_face_recall']
movers=sorted(((pc[k]['qr']-100*op[k],k) for k in pc),reverse=True)[:4]
movtxt=', '.join(f'{k.lower()} {100*op[k]:.0f}% → {pc[k]["qr"]:.0f}%' for _,k in movers)
SR=[('Drill holes', 'Holes', 'holes'), ('Blends', 'Rounds', 'blends + fillets'), ('Cavities', 'Any feature', 'all feature families'), ('All three combined', 'Overall: holes+blends+cavities', 'all feature families'), ('All three + convex hull', 'Overall: + off-hull faces', 'all feature families')]

def srow(asname,key,qname):
    c=d['comparisons'][key]; out=f'<tr><td><b>{asname}</b><br><small>vs {qname}</small></td>'
    for k,_,_,hi in METRICS:
        qv,av=c['qr'][k],c['as'][k]; w='qr' if (qv>av)==hi else 'as'
        fq=f'<b>{qv:.1f}</b>' if w=='qr' else f'{qv:.1f}'; fa=f'<b>{av:.1f}</b>' if w=='as' else f'{av:.1f}'
        out+=f'<td class="n">{fq}<br>{fa}</td>'
    return out+'</tr>'
SUMM=''.join(srow(*r) for r in SR)
STOCKM=[('claimed_face_pct','Claimed faces','labelled stock faces claimed',True),('false_positive_pct','False positives','claimed “stock” faces that are really feature faces',False)]
sh=d['comparisons']['Shafts']['as']
H=f'''<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>quiddity vs Analysis Situs — MFInstSeg</title><style>
.viz-root{{--surface-1:#fcfcfb;--text-primary:#1d1d1b;--text-secondary:#5c5b55;--text-muted:#85847c;--rule:#e4e3dd;--track:#efeee9;--series-1:#2a78d6;--series-2:#eb6834}}
@media (prefers-color-scheme:dark){{.viz-root{{color-scheme:dark;--surface-1:#1a1a19;--text-primary:#f0efec;--text-secondary:#c3c2b7;--text-muted:#8f8e86;--rule:#33332f;--track:#262624;--series-1:#3987e5;--series-2:#d95926}}}}
*{{box-sizing:border-box}}body{{margin:0}}
.viz-root{{background:var(--surface-1);color:var(--text-primary);font:15px/1.55 -apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;padding:1.4rem 1rem 3rem;min-height:100vh}}
main{{max-width:46rem;margin:0 auto}}h1{{font-size:1.45rem;line-height:1.25;margin:0 0 .3rem}}
h2{{font-size:1.1rem;margin:2rem 0 .3rem;padding-top:1rem;border-top:1px solid var(--rule)}}
.sub,.lead,.cnt,small,.sub2,.dim,footer{{color:var(--text-secondary)}}.cnt{{font-size:.8rem;color:var(--text-muted);margin:.2rem 0 .8rem}}
.legend{{display:flex;gap:1.2rem;font-size:.85rem;margin:.8rem 0}}.legend i{{display:inline-block;width:.8rem;height:.8rem;border-radius:3px;vertical-align:-1px;margin-right:.35rem}}
.m{{display:grid;grid-template-columns:11rem 1fr;gap:.2rem 1rem;padding:.55rem 0;border-bottom:1px solid var(--rule)}}
@media (max-width:560px){{.m{{grid-template-columns:1fr}}}}
.ml b{{display:block}}.ml small{{font-size:.75rem;line-height:1.3;display:block}}
.row{{display:grid;grid-template-columns:7.2rem 1fr;align-items:center;gap:.5rem;font-size:.82rem;color:var(--text-secondary)}}.row.win .bv{{font-weight:700;color:var(--text-primary)}}
.b{{display:flex;align-items:center;gap:.45rem}}.bt{{display:block;height:.7rem;border-radius:0 4px 4px 0;min-width:2px}}.bt.q{{background:var(--series-1)}}.bt.a{{background:var(--series-2)}}
.row .b{{margin:2px 0}}.bv{{font:600 .8rem ui-monospace,Menlo,monospace;white-space:nowrap;color:var(--text-secondary)}}
.sub2{{font-size:.75rem;margin-top:.25rem}}
table{{border-collapse:collapse;width:100%;font-size:.83rem;margin:.6rem 0}}th,td{{text-align:left;padding:.35rem .4rem;border-bottom:1px solid var(--rule);vertical-align:middle}}
th{{font-size:.7rem;text-transform:uppercase;letter-spacing:.04em;color:var(--text-muted)}}td.n,th.n{{text-align:right;font-family:ui-monospace,Menlo,monospace;white-space:nowrap}}
.pc td:nth-child(3),.pc td:nth-child(4){{width:30%}}.pc .bt{{height:.55rem}}
ul{{padding-left:1.1rem}}li{{margin:.3rem 0}}code{{font-size:.85em}}footer{{margin-top:2rem;padding-top:.8rem;border-top:1px solid var(--rule);font-size:.75rem}}
.scroll{{overflow-x:auto}}

.sum th,.sum td{{padding:.35rem .3rem;line-height:1.35}}.sum th{{white-space:normal}}.sum td.w{{font-weight:700;color:var(--text-primary)}}.sum td.n{{color:var(--text-secondary)}}.sum small{{font-size:.7rem;color:var(--text-muted)}}
.note{{border-left:3px solid var(--rule);padding:.1rem 0 .1rem .8rem;color:var(--text-secondary);font-size:.9rem}}
</style></head><body><div class="viz-root"><main>
<h1>quiddity vs Analysis Situs on MFInstSeg</h1>
<p class="sub">Every open-source Analysis Situs recogniser against quiddity, on the AAGNet MFInstSeg test split: 9,375 labelled CAD models. Both tools read and recognised every model. 6 November 2026.</p>
<div class="legend"><span><i style="background:var(--series-1)"></i>quiddity 0.4.8 (Rust, <code>82bb3ba</code>)</span><span><i style="background:var(--series-2)"></i>Analysis Situs (<code>246e2ab</code>, OCCT 7.6.3)</span></div>
<h2>At a glance</h2>
<p class="lead">Percent. In each cell the top figure is quiddity, the bottom Analysis Situs; bold is better.</p>
<div class="scroll"><table class="sum"><tr><th>AS recogniser</th><th class="n">Recog&shy;nition ↑</th><th class="n">Claimed faces ↑</th><th class="n">False pos ↓</th><th class="n">False neg ↓</th></tr>{SUMM}</table></div>
<ul><li><b>Feature by feature, quiddity finds more and claims less wrongly</b>, with two exceptions: Analysis Situs finds more rounds (but 71% of its round claims are wrong), and its raw hole false positives are lower — but quiddity's are O-ring walls; with those set aside quiddity is 0.1% wrong against 3.7%.</li>
<li><b>Analysis Situs claims more of each feature's faces on holes and rounds</b>; quiddity on cavities.</li>
<li><b>Combined, quiddity finds 82% of features against 57%</b> for Analysis Situs's three feature recognisers together.</li>
<li><b>Adding convex hull flips it</b>: every face off the stock's hull is a machined face, which on MFInstSeg's block-shaped parts finds 99% of features. It says a face was machined, not what feature it belongs to.</li></ul>
<p class="lead" style="margin-top:1rem">Open-source Analysis Situs has five feature recognisers: drill holes, blends, cavities, convex hull and shafts. Its typed machining vocabulary (pockets, slots, steps by name) is in the commercial SDK and is not measured here.</p>
{block("Holes","Through and blind holes.",title="Drill holes vs quiddity holes")}
<ul><li><b>The 33% is O-rings, not holes.</b> quiddity reports an O-ring groove's outer wall as a hole by design; with those faces neutral it is 0.1% wrong.</li>
<li><b>The low claimed-face figure is blind-hole floors.</b> quiddity treats the floor as context, not part of the hole. It finds 98.5% of holes; it just doesn't claim the floor.</li></ul>
{block("Rounds","Fillet-shaped rounds.",title="Blends vs quiddity blends + fillets")}
<ul><li><b>Analysis Situs catches every round but 71% of what it calls a round isn't one</b> — it flags any fillet-shaped face, including the semicircular ends of pockets and slots.</li>
<li>quiddity is the reverse: wrong 0.2% of the time, but misses 8.5% of rounds.</li></ul>
{block("Any feature","Every machined feature against the stock. quiddity: the union of all feature families (body-level families excluded).",title="Cavities vs all quiddity features")}
<ul><li><b>Cavities misses nearly half of all features</b>: it only sees closed depressions (see the class table below).</li>
<li>On closed pockets and blind holes its claimed faces are near-complete (~98%); quiddity's are 70–78% on pockets and 50% on blind holes, where it leaves the floor as context.</li></ul>
{block("Stock (convex hull)","Faces on the stock's convex hull, scored against the labelled stock faces. quiddity has no stock recogniser; its proxy is every face no feature family claims.",title="Convex hull vs quiddity unclaimed faces",metrics=STOCKM,instances=False)}
<ul><li><b>Convex hull is almost exact on this dataset</b>: 99.97% of stock faces, only 2.5% wrong. Every MFInstSeg part is a block with features cut into it, so the stock is exactly the hull — on real parts with non-convex stock it would not be.</li>
<li>quiddity's proxy is 46% wrong only because it includes the feature faces quiddity missed. It isn't a stock recogniser.</li></ul>
<section><h2>Shafts</h2><p class="lead">Turned shafts. MFInstSeg has none, so every claim is a false positive by the labels.</p>
<p>Shaft recognition claimed <b>{sh["claimed_faces"]:,} faces in {sh["models_with_claims"]:,} models</b>, and every one is the inner core of an O-ring groove — the same face quiddity reports as a boss. A reasonable reading of the geometry rather than noise. quiddity has no shaft family for prismatic parts; its <code>turned_steps</code> claimed nothing here.</p></section>
{block("Overall: holes+blends+cavities","Analysis Situs's three feature recognisers combined, against every quiddity feature family.",title="Overall: holes + blends + cavities")}
{block("Overall: + off-hull faces","As above, plus every face off the convex hull.",title="Overall: + convex hull")}
<p class="note">This is the honest headline for Analysis Situs: on synthetic block parts, “not on the hull” finds 99% of features. But it doesn't separate one feature from another, name it, or dimension it — which is what quiddity spends its time on — and it depends on the stock being convex.</p>
<h2>Cavities, class by class</h2><p class="lead">Claimed face % for each of the 24 MFInstSeg classes.</p>
<div class="scroll"><table class="pc"><tr><th>Class</th><th class="n">Faces</th><th>quiddity</th><th>Analysis Situs</th></tr>{pcrows}</table></div>
<p>Cavities is effectively at zero on every class open to a side — through slots, through steps, blind slots and blind steps. quiddity covers every class; its weakest are slanted through steps (42%) and triangular through slots (45%).</p>
<h2>Time</h2><p class="lead">CPU time per model over all 9,375 test models, measured inside each process (unaffected by the other workers).</p>
<div class="scroll"><table><tr><th></th><th class="n">quiddity</th><th class="n">AS (3)</th><th class="n">AS (all 5)</th></tr>
<tr><td>Read STEP, mean</td><td class="n">{T["qr"]["read"][0]:.1f} ms</td><td class="n" colspan="2">{T["as"]["read"][0]:.1f} ms</td></tr>
<tr><td>Recognise, mean</td><td class="n">{T["qr"]["rec"][0]:.1f} ms</td><td class="n">{T["as"]["rec3"][0]:.1f} ms</td><td class="n">{T["as"]["rec"][0]:.1f} ms</td></tr>
<tr><td><b>Total, mean</b></td><td class="n"><b>{T["qr"]["tot"][0]:.0f} ms</b></td><td class="n"><b>{T["as"]["tot3"][0]:.0f} ms</b></td><td class="n"><b>{T["as"]["tot"][0]:.0f} ms</b></td></tr>
<tr><td>Total, median</td><td class="n">{T["qr"]["tot"][1]:.0f} ms</td><td class="n">{T["as"]["tot3"][1]:.0f} ms</td><td class="n">{T["as"]["tot"][1]:.0f} ms</td></tr>
<tr><td>Total, 99th percentile</td><td class="n">{T["qr"]["tot"][2]:.0f} ms</td><td class="n">{T["as"]["tot3"][2]:.0f} ms</td><td class="n">{T["as"]["tot"][2]:.0f} ms</td></tr>
<tr><td>Slowest model</td><td class="n">{T["qr"]["tot"][3]:.1f} s</td><td class="n">{T["as"]["tot3"][3]:.2f} s</td><td class="n">{T["as"]["tot"][3]:.2f} s</td></tr></table></div>
<ul><li><b>On the median model quiddity is about level with all five Analysis Situs recognisers</b> ({T["qr"]["tot"][1]:.0f} vs {T["as"]["tot"][1]:.0f} ms). On the mean Analysis Situs is faster, because quiddity has a long tail.</li>
<li>quiddity reads STEP about 4× faster ({T["qr"]["read"][0]:.0f} vs {T["as"]["read"][0]:.0f} ms). Analysis Situs's three feature recognisers take only {T["as"]["rec3"][0]:.1f} ms; convex hull is its one expensive step ({stage["hull_s"]:.0f} ms).</li>
<li><b>Not like for like:</b> quiddity runs about 45 families and names and dimensions each feature; Analysis Situs returns untyped face sets.</li>
<li>quiddity's tail: slowest model {T["qr"]["tot"][3]:.0f} s here and 122 s across the whole corpus, against {T["as"]["tot"][3]:.2f} s for Analysis Situs. The whole 62,495-model corpus took quiddity 33 minutes on 4 cores.</li></ul>
<h2>Whole corpus (quiddity only)</h2><p class="lead">quiddity on all 62,495 models (test split in grey). Within half a point everywhere, so the test split is representative.</p>
<div class="scroll"><table><tr><th></th>{"".join(f'<th class="n" colspan="2">{lab}</th>' for _,lab,_,_ in METRICS)}</tr>{fullrows}</table></div>
<h2>Since October</h2><p class="lead">quiddity's own movement on the same 9,375 models (alpha.1 in October → 0.4.8 now). Analysis Situs is unchanged and deterministic: its holes, blends and cavities output is identical to October's on all 9,375 models.</p>
<div class="scroll"><table><tr><th></th><th class="n">Claimed faces</th><th class="n">False positives</th></tr>{octrows}</table></div>
<p>Rounds false positives fell from 21% to 0.2%, and the biggest per-class gains were {movtxt}.</p>
<h2>How it was measured</h2>
<ul><li><b>Recognition</b>: labelled feature instances with at least half their faces claimed. <b>False negatives</b>: instances with no face claimed. The same rule for both tools, since Analysis Situs reports face sets, not feature instances.</li>
<li><b>Claimed faces</b>: labelled faces of the target classes that were claimed. <b>False positives</b>: claimed faces outside the target classes, as a share of all claimed faces.</li>
<li>quiddity is scored on each family's <i>defining</i> faces; consulted context (such as blind-hole floors) is not credited.</li>
<li>Body-level quiddity families (flats, plates, risers, step levels, stock, profiles, patterns, sheet metal, thin walls, turned steps) are excluded from feature claims.</li>
<li>Face-level only: dimensions and correct feature <i>typing</i> are not scored — where quiddity's advantage lies.</li>
<li>Analysis Situs calls: RecognizeDrillHoles (hard mode), RecognizeBlends (no radius cap), RecognizeCavities (no size or volume cap), RecognizeConvexHull (tolerance 0.1, 20-point grid — the Tcl command defaults), RecognizeShafts (no radius cap). Not included: probes, isolated features and canonical surfaces, which are not feature recognisers.</li></ul>
<footer>MFInstSeg (AAGNet, Wu et al. 2023) test partition. Scripts: <code>tools/as_compare</code>; scores: <code>docs/benchmarks/mfinstseg-vs-analysis-situs-82bb3ba*.json</code>.</footer>
</main></div></body></html>'''
open(out_path,'w').write(H); print(len(H))
