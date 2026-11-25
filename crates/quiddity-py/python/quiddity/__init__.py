"""Rust STEP recognition. Recognition returns JSON text; PMI interchange is available in quiddity.pmi."""
from ._native import QuiddityError, PmiError, __version__, anchor, anchors, capabilities, correspond, evidence, fingerprints, parts, recognise, recognise_with_evidence, step_entities

__all__ = ["QuiddityError", "PmiError", "__version__", "anchor", "anchors", "capabilities", "correspond", "evidence", "fingerprints", "parts", "recognise", "recognise_with_evidence", "step_entities"]


def manifest(path, out=None):
    """Scan a STEP path into a dict; write only ``out`` when supplied (paths accept PathLike)."""
    import json
    import os
    from ._native import _manifest
    return json.loads(_manifest(os.fspath(path), None if out is None else os.fspath(out)))


def _manifest_handle(manifest, parts, k_factor):
    import json
    import os
    from ._native import ManifestHandle
    if isinstance(manifest, dict):
        return ManifestHandle(json.dumps(manifest), k_factor=k_factor)
    return ManifestHandle.from_path(os.fspath(manifest), parts, k_factor=k_factor)


def recognise_product(path, manifest, product_id, *, k_factor=None):
    """Recognise one product into a dict. ``manifest`` is a dict or a JSON file path.

    The sheet-metal K-factor defaults to 0.5. Shared geometry raises ValueError.
    Use the CLI's explicit whole-file fallback if needed.
    """
    import json
    import os
    handle = _manifest_handle(manifest, [product_id], k_factor)
    return json.loads(handle.recognise(os.fspath(path), product_id))


def recognise_products(path, manifest, parts=None, *, k_factor=None):
    """Yield (id, result_dict) in manifest order, reading one product per iteration.

    ``parts`` is an optional iterable of manifest ids; unknown ids raise ValueError before
    any product is read. A refused product raises ValueError at that iteration.
    The sheet-metal K-factor defaults to 0.5.
    """
    import json
    import os
    selected = None if parts is None else list(parts)
    handle = _manifest_handle(manifest, selected, k_factor)
    ids = handle.product_ids(selected)
    source = os.fspath(path)
    for product_id in ids:
        yield product_id, json.loads(handle.recognise(source, product_id))


__all__ += ["manifest", "recognise_product", "recognise_products"]
