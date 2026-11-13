"""One process, all files once. usage: batch.py {old|new} LIST"""
import sys, time, json, warnings
warnings.simplefilter("ignore")
ver, lst = sys.argv[1], sys.argv[2]
paths=[l.strip() for l in open(lst) if l.strip()]
import quiddity
if ver=="old":
    from quiddity import import_step_geometry, build_framed_recognition_result
t=time.perf_counter(); n=0
for p in paths:
    if ver=="old": build_framed_recognition_result(import_step_geometry(p))
    else: quiddity.recognise(p)
    n+=1
print(json.dumps({"version":ver,"files":n,"wall_s":time.perf_counter()-t}))
