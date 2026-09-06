"""Closed F5b source-path and executable-case roster."""

from __future__ import annotations

import ast
from pathlib import Path

ROOT = Path(__file__).parents[1]

CONSTRUCTOR_COUNTS = {
    # The equal-span ring path and the ADR 0007/#460 bounded-mouth fallback are the two
    # reviewed issuers. Both stay local to prismatic_pockets and publish through its writer.
    "prismatic_pockets.py": ("PrismaticPocket", 2),
    # The sole direct constructor is the frozen writer-free legacy finder. Accepted rich
    # compatibility values are built through `_passage_compat.passage_from_view` instead.
    "passages.py": ("Passage", 1),
    "grooves.py": ("Groove", 1),
    "turned.py": ("TurnedStep", 1),
    "chamfers.py": ("Chamfer", 2),
    "angled_steps.py": ("AngledStep", 1),
}


def test_new_record_constructor_paths_require_f5b_roster_review() -> None:
    for filename, (constructor, expected) in CONSTRUCTOR_COUNTS.items():
        tree = ast.parse((ROOT / "src" / "quiddity" / filename).read_text(encoding="utf-8"))
        found = sum(
            isinstance(node, ast.Call)
            and isinstance(node.func, ast.Name)
            and node.func.id == constructor
            for node in ast.walk(tree)
        )
        assert found == expected, f"{filename} added or removed an unaudited {constructor} path"
