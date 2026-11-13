import json, sys, statistics, collections
rows=[json.loads(l) for l in open(sys.argv[1])]
st=collections.Counter(r['status'] for r in rows)
def q(v,p): v=sorted(v); return v[min(len(v)-1,int(round(p*(len(v)-1))))]
out={'models':len(rows),'status':dict(st)}
for k in ('read_cpu_s','recognise_cpu_s','seconds'):
    v=[r[k] for r in rows if r.get(k) is not None]
    if v: out[k]={'n':len(v),'median':statistics.median(v),'p95':q(v,.95),'max':max(v)}
v=[r['read_cpu_s']+r['recognise_cpu_s'] for r in rows if r.get('read_cpu_s') is not None and r.get('recognise_cpu_s') is not None]
if v: out['read+recognise']={'median':statistics.median(v),'p95':q(v,.95),'sum_s':sum(v)}
print(json.dumps(out,indent=1))
