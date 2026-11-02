"""Live OCCT oracle for the opt-in Rust CTC-01 Add regression; writes no captures."""
from collections import Counter
import json
from pathlib import Path
import sys

sys.path.insert(0, sys.argv[1])
from capture_pmi_occt import capture_file


def canonical(value):
    if isinstance(value, dict):
        return {k: canonical(v) for k, v in value.items()
                if k not in ("label", "tolerances", "presentation")}
    if isinstance(value, list):
        return [canonical(v) for v in value]
    return value


def items(capture, key):
    return Counter(json.dumps(canonical(item), sort_keys=True)
                   for part in [*capture["parts"], capture["unattached"]]
                   for item in part[key])


before = capture_file(Path(sys.argv[2]))
for path, expected in zip(sys.argv[3::2], sys.argv[4::2]):
    after = capture_file(Path(path))
    assert items(before, "dimensions") == items(after, "dimensions"), path
    old = items(before, "geometric_tolerances")
    new = items(after, "geometric_tolerances")
    assert not old - new, (path, old - new)
    assert sum(new.values()) == sum(old.values()) + 1, path
    datums = [d for p in after["parts"] for d in p["datums"] if d["name"] == "D"]
    assert len(datums) == 1, (path, datums, after["unattached"]["datums"])
    assert sorted(f["advanced_face"] for f in datums[0]["faces"]) == sorted(json.loads(expected)), (path, datums)
print("OCCT: four CTC-01 Add cases attached; every original dimension and tolerance unchanged")
