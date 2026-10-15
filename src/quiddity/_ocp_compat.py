"""OCP 7.x / 8.x downcast compatibility.

OCP 8 (build123d 0.12+) made ``TopoDS`` a namespace, so the static downcasts lost
their ``_s`` suffix (``TopoDS.Face_s`` -> ``TopoDS.Face``).  The OCP 7 name is tried
first so behaviour on older stacks is unchanged.
"""

from collections.abc import Callable
from typing import Any

from OCP.TopoDS import TopoDS


def _downcast(name: str) -> Callable[..., Any]:
    return getattr(TopoDS, f"{name}_s", None) or getattr(TopoDS, name)


to_face = _downcast("Face")
to_edge = _downcast("Edge")
to_shell = _downcast("Shell")
