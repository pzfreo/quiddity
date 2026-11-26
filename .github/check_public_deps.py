"""Fail if any unallowlisted external crate appears in haecceity's public API."""
import json
import os
from pathlib import Path
import subprocess
import sys

ALWAYS_ALLOWED = frozenset({"std", "core", "alloc"})
ALLOWLIST = Path(__file__).with_name("public_api_crates.txt")
RUSTDOC_CMD = [
    "cargo", "rustdoc", "-p", "haecceity", "--lib", "--all-features",
    "--", "-Z", "unstable-options", "--output-format", "json", "--document-hidden-items",
]


def load_allowlist(path=ALLOWLIST):
    allowed = set(ALWAYS_ALLOWED)
    for lineno, raw in enumerate(Path(path).read_text().splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        crate, sep, reason = line.partition("#")
        if not crate.strip() or not sep or not reason.strip():
            raise ValueError(f"{path}:{lineno}: expected '<crate> # <reason>'")
        allowed.add(crate.strip())
    return allowed


def _tag(obj, what):
    if isinstance(obj, str):
        return obj, None
    if isinstance(obj, dict) and len(obj) == 1:
        return next(iter(obj.items()))
    raise ValueError(f"unexpected {what}: {obj!r}")


def _ids(node, kind):
    if node is None:
        return
    if kind == "path":
        yield node["id"]
        yield from _ids(node["args"], "args")
    elif kind == "bounds":
        for bound in node:
            match _tag(bound, "generic_bound"):
                case ("trait_bound", val): yield from _ids(val["trait"], "path")
                case ("outlives" | "use", _): pass
                case (k, _): raise ValueError(f"unsupported generic_bound: {k}")
    elif kind == "args":
        match _tag(node, "generic_args"):
            case ("parenthesized", val):
                for t in (*val["inputs"], val["output"]): yield from _ids(t, "type")
            case ("angle_bracketed", val):
                for arg in val["args"]:
                    match _tag(arg, "generic_arg"):
                        case ("type", ty): yield from _ids(ty, "type")
                        case ("lifetime", _): pass
                        case (k, _): raise ValueError(f"unsupported generic_arg: {k}")
                for con in val["constraints"]:
                    yield from _ids(con["args"], "args")
                    match _tag(con["binding"], "constraint"):
                        case ("equality", {"type": ty}): yield from _ids(ty, "type")
                        case (k, _): raise ValueError(f"unsupported constraint: {k}")
            case (k, _): raise ValueError(f"unsupported generic_args: {k}")
    elif kind == "type":
        match _tag(node, "type"):
            case ("resolved_path", val): yield from _ids(val, "path")
            case ("dyn_trait", val):
                for p in val["traits"]: yield from _ids(p["trait"], "path")
            case ("impl_trait", val): yield from _ids(val, "bounds")
            case ("qualified_path", val):
                yield from _ids(val["self_type"], "type")
                yield from _ids(val["trait"], "path")
                yield from _ids(val["args"], "args")
            case ("borrowed_ref" | "array", val): yield from _ids(val["type"], "type")
            case ("slice", val): yield from _ids(val, "type")
            case ("tuple", val):
                for t in val: yield from _ids(t, "type")
            case ("generic" | "primitive", _): pass
            case (k, _): raise ValueError(f"unsupported type: {k}")
    elif kind == "generics":
        for param in node["params"]:
            match _tag(param["kind"], "generic_param"):
                case ("type", pv):
                    yield from _ids(pv["bounds"], "bounds")
                    yield from _ids(pv["default"], "type")
                case ("lifetime", _): pass
                case (k, _): raise ValueError(f"unsupported generic_param: {k}")
        for pred in node["where_predicates"]:
            match _tag(pred, "where_predicate"):
                case ("bound_predicate", wv):
                    yield from _ids(wv["type"], "type")
                    yield from _ids(wv["bounds"], "bounds")
                case (k, _): raise ValueError(f"unsupported where_predicate: {k}")


def public_external_refs(doc):
    """Return sorted unique (item, crate, path) tuples for all public external refs."""
    if doc.get("includes_private"):
        raise ValueError("rustdoc JSON must not include private items")
    index, paths = doc["index"], doc["paths"]
    external = {int(k): v["name"] for k, v in doc["external_crates"].items()}
    seen, hits = set(), set()

    def external_of(item_id):
        sid = str(item_id)
        if sid in paths:
            cid, path = paths[sid]["crate_id"], "::".join(paths[sid]["path"])
        elif sid in index:
            cid, path = index[sid]["crate_id"], index[sid].get("name") or sid
        else:
            raise ValueError(f"unknown rustdoc item id: {item_id}")
        return None if cid == 0 else (external[cid], path)

    def record(label, *pairs):
        for node, kind in pairs:
            for rid in _ids(node, kind):
                if ext := external_of(rid):
                    hits.add((label, *ext))

    def walk(item_id, prefix):
        sid = str(item_id)
        if sid in seen:
            return
        seen.add(sid)
        item = index.get(sid)
        if item is None or item["crate_id"] != 0:
            if ext := external_of(item_id): hits.add((prefix, *ext))
            return
        match _tag(item["visibility"], "visibility"):
            case ("public" | "default", None): pass
            case ("crate", None) | ("restricted", _): return
            case (vis, _): raise ValueError(f"unsupported visibility: {vis}")
        name = item.get("name")
        here = f"{prefix}::{name}" if prefix and name else (name or prefix)
        match _tag(item["inner"], "item"):
            case ("module", val):
                for child in val["items"]: walk(child, here)
            case ("use", val):
                if val["id"] is None: raise ValueError(f"unresolved use at {prefix}: {val!r}")
                if ext := external_of(val["id"]):
                    hits.add((f"{prefix}::*" if val["is_glob"] else f"{prefix}::{val['name']}", *ext))
                elif val["is_glob"]:
                    match _tag(index[str(val["id"])]["inner"], "glob target"):
                        case ("module", tv):
                            for child in tv["items"]: walk(child, prefix)
                        case (tk, _): raise ValueError(f"unsupported glob target: {tk}")
                else: walk(val["id"], prefix)
            case ("function", val):
                tys = [(ty, "type") for _, ty in val["sig"]["inputs"]]
                record(here, (val["generics"], "generics"), *tys, (val["sig"]["output"], "type"))
            case ("struct" | "variant" as kind, val):
                if kind == "struct": record(here, (val["generics"], "generics"))
                match _tag(val["kind"], f"{kind} kind"):
                    case ("unit", _) | ("plain", None): fields = []
                    case ("tuple", sv): fields = [f for f in sv if f is not None]
                    case ("plain" | "struct", {"fields": fields}): pass
                    case (sk, _): raise ValueError(f"unsupported {kind} kind: {sk}")
                for child in (*fields, *val.get("impls", ())): walk(child, here)
            case ("enum", val):
                record(here, (val["generics"], "generics"))
                for child in (*val["variants"], *val["impls"]): walk(child, here)
            case ("impl", val):
                if val["is_synthetic"] or val["blanket_impl"] is not None: return
                label = f"impl {val['trait']['path']} for {prefix}" if val["trait"] else prefix
                record(label, (val["generics"], "generics"), (val["for"], "type"), (val["trait"], "path"))
                for mid in val["items"]: walk(mid, label)
            case ("type_alias" | "assoc_type" as kind, val):
                extra = ((val["bounds"], "bounds"),) if kind == "assoc_type" else ()
                record(here, (val["generics"], "generics"), (val["type"], "type"), *extra)
            case ("struct_field", val): record(here, (val, "type"))
            case ("constant" | "static" | "assoc_const", val): record(here, (val["type"], "type"))
            case (k, _): raise ValueError(f"unsupported item kind: {k}")

    walk(doc["root"], "")
    return sorted(hits)


def check_doc(doc, allowed):
    return [r for r in public_external_refs(doc) if r[1] not in allowed]


def main():
    subprocess.run(RUSTDOC_CMD, check=True, env={**os.environ, "RUSTC_BOOTSTRAP": "1"})
    doc = json.loads((Path(os.environ.get("CARGO_TARGET_DIR", "target")) / "doc" / "haecceity.json").read_text())
    refs = public_external_refs(doc)
    if violations := [r for r in refs if r[1] not in load_allowlist()]:
        print("haecceity public API leaks disallowed external crates:", file=sys.stderr)
        for item, crate, path in violations:
            print(f"  {item}: crate `{crate}` ({path})", file=sys.stderr)
        sys.exit(1)
    found = sorted({crate for _, crate, _ in refs if crate not in ALWAYS_ALLOWED})
    print(f"haecceity: public API external crates OK ({', '.join(found) or 'none'})", flush=True)


if __name__ == "__main__":
    main()
