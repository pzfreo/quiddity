import json,sys
d=json.load(open(sys.argv[1]))
names={0:'chamfer',3:'rectangular_passage',5:'triangular_through_slot',6:'rectangular_through_slot',7:'circular_through_slot',10:'slanted_through_step',12:'blind_hole',14:'rectangular_pocket',16:'circular_end_pocket',17:'rectangular_blind_slot',22:'rectangular_blind_step'}
def tab(t,top=8):
    items=sorted(t.items(),key=lambda x:-x[1]); s=', '.join(f"{k} {v}" for k,v in items[:top])
    if len(items)>top: s+=f", other {sum(v for _,v in items[top:])}"
    return s
print(f"Models analysed: {d['models']} (those the one-to-one scorer scores).\n")
print("## (a) False-positive predictions: majority labelled class of their faces\n")
print("A false positive is a prediction (one distinct defining-face set for the class) matched to no labelled instance of the class by the scorer's one-to-one IoU >= 0.5 matching.\n")
print("| class | FP predictions | majority label of their faces (count) | source family (count) | also a prediction for another class | other classes sharing the record |")
print("|---|---:|---|---|---:|---|")
for c in (6,16,22,14,0,12):
    k=str(c)
    print(f"| {names[c]} ({c}) | {d['fp_total'].get(k,0)} | {tab(d['fp_majority_label'].get(k,{}))} | {tab(d['fp_by_source_family'].get(k,{}),5)} | {d['fp_shared'].get(k,0)} | {tab(d['fp_shared_with'].get(k,{}),5)} |")
print("\n## (b) Missed labelled instances: what quiddity did with their faces\n")
print("A missed instance is a labelled instance of the class matched by no prediction. Its faces are looked up in every family's defining faces (unmarked records, as the scorer). 'Top family' is the family defining most of its faces; '(nothing)' means no family defines any of its faces.\n")
print("| class | missed instances | top family that claims its faces (count) | none of its faces claimed | fewer than half its faces claimed (incl. none) |")
print("|---|---:|---|---:|---:|")
for c in (5,10,0,17,7,3):
    k=str(c); un=d['miss_unclaimed'].get(k,0); uh=d['miss_under_half'].get(k,0)
    print(f"| {names[c]} ({c}) | {d['miss_total'].get(k,0)} | {tab(d['miss_top_family'].get(k,{}))} | {un} | {uh+un} |")
