"""OCP 7.x / 8.x compatibility.

OCP 8 (build123d 0.13+) made ``TopoDS`` a namespace, so the static downcasts lost
their ``_s`` suffix (``TopoDS.Face_s`` -> ``TopoDS.Face``).  The OCP 7 name is tried
first so behaviour on older stacks is unchanged.
"""

from collections.abc import Callable
from typing import Any

from OCP.BRepCheck import BRepCheck_Analyzer, BRepCheck_Status
from OCP.BRepPrimAPI import BRepPrimAPI_MakeBox
from OCP.TopoDS import TopoDS

try:
    from OCP.TColgp import TColgp_Array2OfPnt as Array2OfPnt
    from OCP.TColStd import TColStd_Array1OfInteger as Array1OfInteger
    from OCP.TColStd import TColStd_Array1OfReal as Array1OfReal
    from OCP.TColStd import TColStd_Array2OfReal as Array2OfReal
except ImportError:  # OCP 8 moved the NCollection arrays to OCP.collections
    from OCP.collections import Array1_double as Array1OfReal
    from OCP.collections import Array1_int as Array1OfInteger
    from OCP.collections import Array2_double as Array2OfReal
    from OCP.collections import Array2_gp_Pnt as Array2OfPnt

__all__ = [
    "Array1OfInteger",
    "Array1OfReal",
    "Array2OfPnt",
    "Array2OfReal",
    "brepcheck_status_ok",
    "brepcheck_statuses_readable",
    "to_edge",
    "to_face",
    "to_shell",
]


def _downcast(name: str) -> Callable[..., Any]:
    downcast: Callable[..., Any] = getattr(TopoDS, f"{name}_s", None) or getattr(TopoDS, name)
    return downcast


to_face = _downcast("Face")
to_edge = _downcast("Edge")
to_shell = _downcast("Shell")


def brepcheck_status_ok(analyzer: Any, shape: Any) -> bool | None:
    """Whether ``analyzer``'s own status for ``shape`` is exactly ``NoError``.

    ``None`` means the status cannot be read: OCP 8.0.1.1 cannot convert the
    ``BRepCheck_ListOfStatus`` that ``BRepCheck_Result.Status()`` returns. Callers
    must treat that as "no proof", never as valid.
    """

    try:
        statuses = tuple(analyzer.Result(shape).Status())
    except TypeError:
        return None
    return statuses == (BRepCheck_Status.BRepCheck_NoError,)


def brepcheck_statuses_readable() -> bool:
    """Whether this OCP can read ``BRepCheck_Result.Status()`` at all (False on OCP 8.0.1.1)."""

    box = BRepPrimAPI_MakeBox(1.0, 1.0, 1.0).Solid()
    return brepcheck_status_ok(BRepCheck_Analyzer(box), box) is not None
