"""A benchmark that disables a family must still be able to find that family's entry point."""

import ast
import importlib
from pathlib import Path

import pytest

TOOLS = Path(__file__).parents[1] / "tools"


def _disable_handles(source: str) -> list[tuple[str, str]]:
    """Every ``original... = <alias>.<name>``, as the module `<alias>` names and the attribute.

    The tools spell the handle three ways: plain, annotated, and one suffixed name per handle
    where a tool disables two things. Match any target beginning `original`, and both nodes.
    """

    tree = ast.parse(source)
    aliases = {
        alias.asname or alias.name: alias.name
        for node in ast.walk(tree)
        if isinstance(node, ast.Import)
        for alias in node.names
    }
    handles = []
    for node in ast.walk(tree):
        if isinstance(node, ast.Assign):
            targets: list[ast.expr] = list(node.targets)
        elif isinstance(node, ast.AnnAssign):
            targets = [node.target]
        else:
            continue
        if not any(isinstance(t, ast.Name) and t.id.startswith("original") for t in targets):
            continue
        value = node.value
        if isinstance(value, ast.Attribute) and isinstance(value.value, ast.Name):
            module = aliases.get(value.value.id)
            if module is not None:
                handles.append((module, value.attr))
    return handles


@pytest.mark.parametrize("path", sorted(TOOLS.glob("benchmark_*.py")), ids=lambda path: path.name)
def test_benchmark_disable_handle_resolves(path: Path) -> None:
    """Declaring a family moves its entry point off `_registry`, and these follow by hand.

    Five of these tools are driven end to end by a test, which is how one such break was caught
    in review. This is for the rest, which have no driver and so break silently.
    """
    for module_name, attribute in _disable_handles(path.read_text(encoding="utf-8")):
        assert hasattr(importlib.import_module(module_name), attribute), (
            f"{path.name} disables {module_name}.{attribute}, which no longer exists"
        )
