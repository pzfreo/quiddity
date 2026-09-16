# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Compatibility facade for the historical cylinder-feature implementation module."""

from quiddity._cylinder_substrate import (
    _FULL_CYL_MIN_EXTENT as _FULL_CYL_MIN_EXTENT,
)
from quiddity._cylinder_substrate import (
    analyse_cylinders,
    full_cylinders,
)
from quiddity._pattern_geometry import (
    _linear_array_candidates as _linear_array_candidates,
)
from quiddity._pattern_geometry import (
    _plane_uv as _plane_uv,
)
from quiddity._pattern_geometry import (
    _rect_grid as _rect_grid,
)
from quiddity._typing import Vector3 as Vector3
from quiddity.bosses import BossRecord, recognise_bosses
from quiddity.countersinks import CounterSink as CounterSink
from quiddity.diameters import feature_diameters
from quiddity.holes import (
    BoltCircle,
    CounterBore,
    HoleRecord,
    HoleSpec,
    LinearArray,
    RectGrid,
    recognise_hole_patterns,
    recognise_holes,
)
from quiddity.holes import (
    _bolt_circle_candidates as _bolt_circle_candidates,
)

__all__ = [
    "BoltCircle",
    "BossRecord",
    "CounterBore",
    "HoleRecord",
    "HoleSpec",
    "LinearArray",
    "RectGrid",
    "analyse_cylinders",
    "feature_diameters",
    "full_cylinders",
    "recognise_bosses",
    "recognise_hole_patterns",
    "recognise_holes",
]

for _exported_name in __all__:
    globals()[_exported_name].__module__ = __name__
