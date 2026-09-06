"""Authority and arithmetic guards for the label-blind blend-chain audit."""

from __future__ import annotations

import ast
import hashlib
import inspect
import json
from pathlib import Path

from tools import audit_mfcadpp_blend_chains as audit

ROOT = Path(__file__).parents[1]


def _report(limit: int) -> dict:
    return json.loads(
        (
            ROOT / "docs" / "benchmarks" / f"mfcadpp-blend-chain-audit-{limit}-ee5f4ae.json"
        ).read_text(encoding="utf-8")
    )


def test_neutral_discovery_precedes_label_loading() -> None:
    tree = ast.parse(inspect.getsource(audit.main))
    calls = [node for node in ast.walk(tree) if isinstance(node, ast.Call)]
    discovery = [
        node.lineno
        for node in calls
        if isinstance(node.func, ast.Name) and node.func.id == "BlendCollapseIndex"
    ]
    labels = [
        node.lineno
        for node in calls
        if isinstance(node.func, ast.Name) and node.func.id == "load_mfcadpp_truth"
    ]
    assert len(discovery) == len(labels) == 1
    assert discovery[0] < labels[0]


def test_canonical_reports_cover_every_selected_model_and_reconcile() -> None:
    for limit in (500, 2500):
        report = _report(limit)
        rows = report["models"]
        summary = report["summary"]
        ids = [row["model_id"] for row in rows]

        assert report["labels_used_in_discovery"] is False
        assert report["selection"]["limit"] == limit
        assert len(rows) == summary["models"] == limit
        assert ids == sorted(ids)
        assert (
            report["selection"]["selected_ids_sha256"]
            == hashlib.sha256(("\n".join(ids) + "\n").encode()).hexdigest()
        )
        for field in summary:
            if field == "models":
                continue
            assert summary[field] == sum(row[field] for row in rows)
        assert sum(report["chain_sides"].values()) == summary["chains"]
        assert sum(report["chain_label_profiles"].values()) == summary["chains"]
        assert sum(report["chain_side_label_profiles"].values()) == summary["chains"]
