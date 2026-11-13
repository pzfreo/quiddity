import sys, os, re, statistics, collections, json
rows=[]
for l in open(sys.argv[1]):
    p=[x.strip() for x in l.rstrip('\n').split(' | ')]
    if len(p)==8 and p[2] in('A','B','A-B') and p[3] in('volume','area','independent_area','centroid'):
        f=os.path.basename(p[0]).replace('.gz','')
        try: err=float(p[6])
        except: continue
        rows.append(dict(file=f,full=p[0],part=p[1],ref=p[2],q=p[3],err=err,status=p[7].split()[0], note=p[7]))
# dedupe identical file copies: key by basename, part, ref, q
seen={};
for r in rows: seen.setdefault((r['file'],r['part'],r['ref'],r['q']),r)
u=list(seen.values())
def agg(ref):
    rs=[r for r in u if r['ref']==ref]
    c=collections.Counter(r['status'] for r in rs)
    files=len({r['file'] for r in rs})
    out={'files':files,'checks':len(rs),'status':dict(c)}
    for q in('volume','area','independent_area','centroid'):
        e=[r['err'] for r in rs if r['q']==q and r['status']=='PASS']
        if e: out[q]={'n_pass':len(e),'median_err':statistics.median(e),'p95_err':sorted(e)[int(.95*(len(e)-1))],'max_err':max(e)}
    return out
print(json.dumps({'A (file validation properties)':agg('A'),'B (OCCT)':agg('B'),'A-B':agg('A-B')},indent=1))
print('non-pass:')
for r in u:
    if r['status']!='PASS': print(r['ref'],r['file'],r['part'][:50],r['q'],r['err'],r['note'][:90])
