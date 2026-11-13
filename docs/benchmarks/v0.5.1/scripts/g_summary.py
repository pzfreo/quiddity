import json, sys, statistics, collections
rows=[json.loads(l) for l in open(sys.argv[1])]
by=collections.defaultdict(dict)
for r in rows: by[r['path']][r['version']]=r
def q(v,p): v=sorted(v); return v[min(len(v)-1,int(round(p*(len(v)-1))))]
def summ(paths, label):
    both=[p for p in paths if p in by and 'old' in by[p] and 'new' in by[p] and by[p]['old'].get('ok') and by[p]['new'].get('ok')]
    o=[by[p]['old']['total_s'] for p in both]; n=[by[p]['new']['total_s'] for p in both]
    ratios=[a/b for a,b in zip(o,n)]
    print(f"\n{label}: parts both ok = {len(both)} of {len(paths)}; old failed={[p.split('/')[-1] for p in paths if p in by and not by[p].get('old',{}).get('ok')][:5]} new failed={[p.split('/')[-1] for p in paths if p in by and not by[p].get('new',{}).get('ok')][:5]}")
    for name,v in (('0.3.12',o),('0.5.1',n)):
        print(f"  {name}: median {statistics.median(v):.3f}s p95 {q(v,.95):.3f}s max {max(v):.2f}s total {sum(v):.1f}s")
    print(f"  speed-up: median-per-part ratio {statistics.median(ratios):.1f}x; ratio of medians {statistics.median(o)/statistics.median(n):.1f}x; ratio of totals {sum(o)/sum(n):.1f}x; per-part ratio min {min(ratios):.1f} max {max(ratios):.1f}")
    for name,v in (('0.3.12','old'),('0.5.1','new')):
        pk=[by[p][v]['peak_rss_kb']/1024 for p in both]; base=[by[p][v]['rss_after_import_kb']/1024 for p in both]
        print(f"  {name}: peak RSS median {statistics.median(pk):.0f} MB p95 {q(pk,.95):.0f} MB max {max(pk):.0f} MB; after import median {statistics.median(base):.0f} MB; process import median {statistics.median([by[p][v]['import_s'] for p in both]):.2f}s")
    # families
    co=collections.Counter(); cn=collections.Counter()
    for p in both:
        for k,v in (by[p]['old'].get('counts') or {}).items(): co[k]+=v
        for k,v in (by[p]['new'].get('counts') or {}).items(): cn[k]+=v
    print("  records 0.3.12:", sum(co.values()), dict(sorted(co.items())))
    print("  records 0.5.1:", sum(cn.values()), dict(sorted(cn.items())))
paths=[p for p in by]
nist=[p for p in paths if 'dw-examples' in p]; mfi=[p for p in paths if 'data/steps' in p]
summ(nist,'NIST CTC/FTC (10)'); summ(mfi,'MFInstSeg 200-model sample'); summ(paths,'All')
