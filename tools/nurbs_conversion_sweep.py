"""Deterministic native-to-NURBS conversion evidence for issue #276.

The converter's own ``ModifiedShape`` history establishes the face bijection.  The sweep then
validates boundary structure, adjacency and orientation through that bijection before it compares
Raised Pad occurrences by exact defining-face evidence.  Timing is deliberately a separate mode:
wall-clock samples cannot be part of a byte-identical structural report.
"""

from __future__ import annotations

import argparse
import json
import statistics
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path
from time import perf_counter
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))

from build123d import Part  # noqa: E402
from OCP.BRep import BRep_Tool  # noqa: E402
from OCP.BRepBuilderAPI import BRepBuilderAPI_NurbsConvert  # noqa: E402

from quiddity import recognise_rectangular_pads  # noqa: E402
from quiddity._adjacency import FaceGraph  # noqa: E402
from quiddity._analytic_surfaces import SurfaceKind  # noqa: E402
from quiddity._candidates import FamilyId  # noqa: E402
from quiddity._effective_surfaces import (  # noqa: E402
    AnalyticSurfaceFact,
    EffectiveSurfaceIndex,
    RefusedSurfaceFact,
)
from quiddity.result import _take_inventory  # noqa: E402
from tests.golden._common import load_fixture  # noqa: E402

GOLDEN_ROOT = ROOT / "tests" / "golden"
JSON_REPORT = ROOT / "docs" / "benchmarks" / "nurbs-conversion-sweep.json"
MARKDOWN_REPORT = ROOT / "docs" / "benchmarks" / "nurbs-conversion-sweep.md"
BASELINE_COMMIT = "4b21f79d8a1a96f9970cbf160e3277be0e2289ca"
PERFORMANCE_BUDGET_SECONDS = 3.0
# This historical E3 Pad sweep requires whole-solid NURBS conversion to preserve every raw edge
# signature before it measures effective faces. OCCT splits a converted trimmed quarter-cylinder
# seam in the Circular Blind Step golden, so that fixture is outside this topology-preserving
# workload. The family's supported selected-cylinder recovery is pinned independently in its own
# contract test; silently weakening this sweep's topology oracle would invalidate its evidence.
EXCLUDED_FIXTURES = {
    "circular_blind_step": "whole-solid conversion changes the quarter-cylinder edge signature",
    "toroidal_blend_compound": "effective-surface recovery deliberately excludes native tori",
    "toroidal_blend_internal": "effective-surface recovery deliberately excludes native tori",
    "toroidal_blends_turned": "effective-surface recovery deliberately excludes native tori",
}
REVIEWED_DELTA_BOUNDS = {
    "face_centre_model_units": 0.1,
    "absolute_face_area_square_units": 25.0,
    "relative_face_area": 0.004,
    "effective_primitive_parameter": 1e-8,
}
PERFORMANCE_MEASUREMENT = {
    "environment": "Python 3.14.7, macOS/darwin, local development host",
    "measured_at": "2026-08-28",
    "median_seconds": 2.133,
    "repeat": 3,
    "workload": (
        "20 native plus 20 converted fixtures: all effective facts, standalone Pads and "
        "aggregate inventory; conversion excluded"
    ),
}


@dataclass(frozen=True, slots=True)
class Occurrence:
    record: dict[str, Any]
    defining: frozenset[int]
    provenance: tuple[dict[str, Any], ...]


def _convert(part) -> tuple[Part, BRepBuilderAPI_NurbsConvert]:
    converter = BRepBuilderAPI_NurbsConvert(part.wrapped, True)
    return Part(converter.Shape()), converter


def _face_correspondence(
    native, converted, converter: BRepBuilderAPI_NurbsConvert
) -> tuple[int, ...]:
    """Return native-index -> converted-index from OCCT's exact conversion history."""

    before, after = tuple(native.faces()), tuple(converted.faces())
    if len(before) != len(after):
        raise RuntimeError("NURBS conversion changed the face count")
    correspondence = []
    for index, face in enumerate(before):
        modified = converter.ModifiedShape(face.wrapped)
        matches = tuple(
            candidate
            for candidate, converted_face in enumerate(after)
            if converted_face.wrapped.IsSame(modified)
        )
        if len(matches) != 1:
            raise RuntimeError(
                f"native face {index} has {len(matches)} converted ModifiedShape matches"
            )
        correspondence.append(matches[0])
    if len(set(correspondence)) != len(after):
        raise RuntimeError("converter history is not a one-to-one face correspondence")
    return tuple(correspondence)


def _boundary_signature(face) -> tuple[int, int, int]:
    edges = tuple(face.edges())
    return (
        len(edges),
        sum(BRep_Tool.IsClosed_s(edge.wrapped, face.wrapped) for edge in edges),
        sum(BRep_Tool.Degenerated_s(edge.wrapped) for edge in edges),
    )


def _validate_topology(native, converted, correspondence: tuple[int, ...]) -> dict[str, bool]:
    before, after = tuple(native.faces()), tuple(converted.faces())
    before_graph, after_graph = FaceGraph(native), FaceGraph(converted)
    inverse = {converted_at: native_at for native_at, converted_at in enumerate(correspondence)}
    maximum_centre_delta = 0.0
    maximum_area_delta = 0.0
    maximum_relative_area_delta = 0.0
    for native_at, converted_at in enumerate(correspondence):
        left, right = before[native_at], after[converted_at]
        if _boundary_signature(left) != _boundary_signature(right):
            raise RuntimeError(f"face {native_at} changed boundary structure")
        if int(left.wrapped.Orientation()) != int(right.wrapped.Orientation()):
            raise RuntimeError(f"face {native_at} changed orientation")
        left_neighbours = {
            node.index for node in before_graph.neighbours(before_graph.nodes[native_at])
        }
        right_neighbours = {
            inverse[node.index] for node in after_graph.neighbours(after_graph.nodes[converted_at])
        }
        if left_neighbours != right_neighbours:
            raise RuntimeError(f"face {native_at} changed adjacency")
        centre_delta = float((left.center() - right.center()).length)
        area_delta = abs(float(left.area) - float(right.area))
        maximum_centre_delta = max(maximum_centre_delta, centre_delta)
        maximum_area_delta = max(maximum_area_delta, area_delta)
        maximum_relative_area_delta = max(
            maximum_relative_area_delta,
            area_delta / float(left.area) if left.area else 0.0,
        )
    observed = {
        "face_centre_model_units": maximum_centre_delta,
        "absolute_face_area_square_units": maximum_area_delta,
        "relative_face_area": maximum_relative_area_delta,
    }
    for name, value in observed.items():
        if value > REVIEWED_DELTA_BOUNDS[name]:
            raise RuntimeError(
                f"{name} delta {value!r} exceeds reviewed bound "
                f"{REVIEWED_DELTA_BOUNDS[name]!r}"
            )
    # Do not serialize raw OCCT integration results: their insignificant final
    # digits vary by platform. The checked-in evidence records only the stable
    # structural result and the deliberately coarse reviewed bounds.
    return {
        "boundary_structure_preserved": True,
        "adjacency_preserved": True,
        "orientation_preserved": True,
        "evaluated_geometry_within_reviewed_bounds": True,
    }


def _parameter_delta(
    kind: SurfaceKind, left: tuple[float, ...], right: tuple[float, ...]
) -> float:
    """Return a primitive-gauge-invariant maximum parameter delta.

    Plane and axis directions describe the same geometry after simultaneous sign
    reversal. Near an equal-component dominant-axis tie, native and converted OCCT
    surfaces can choose opposite signs on different platforms; comparing their raw
    tuples would turn an equivalent plane offset into a delta twice its magnitude.
    """

    if kind is SurfaceKind.PLANE:
        gauge = 1.0 if sum(a * b for a, b in zip(left[:3], right[:3], strict=True)) >= 0 else -1.0
        deltas = [abs(a - gauge * b) for a, b in zip(left[:3], right[:3], strict=True)]
        deltas.append(abs(left[3] - gauge * right[3]))
    elif kind is SurfaceKind.CYLINDER:
        gauge = 1.0 if sum(a * b for a, b in zip(left[3:6], right[3:6], strict=True)) >= 0 else -1.0
        deltas = [abs(a - b) for a, b in zip(left[:3], right[:3], strict=True)]
        deltas.extend(abs(a - gauge * b) for a, b in zip(left[3:6], right[3:6], strict=True))
        deltas.append(abs(left[6] - right[6]))
    elif kind is SurfaceKind.CONE:
        gauge = 1.0 if sum(a * b for a, b in zip(left[3:6], right[3:6], strict=True)) >= 0 else -1.0
        deltas = [abs(a - b) for a, b in zip(left[:3], right[:3], strict=True)]
        deltas.extend(abs(a - gauge * b) for a, b in zip(left[3:6], right[3:6], strict=True))
        deltas.append(abs(left[6] - gauge * right[6]))
    else:
        deltas = [abs(a - b) for a, b in zip(left, right, strict=True)]
    return max(deltas, default=0.0)


def _surface_report(native, converted, correspondence: tuple[int, ...]) -> dict[str, Any]:
    native_graph, converted_graph = FaceGraph(native), FaceGraph(converted)
    native_surfaces = EffectiveSurfaceIndex(native_graph)
    converted_surfaces = EffectiveSurfaceIndex(converted_graph)
    recovered = Counter()
    refused = Counter()
    for native_at, converted_at in enumerate(correspondence):
        left = native_surfaces.fact(native_graph.nodes[native_at])
        right = converted_surfaces.fact(converted_graph.nodes[converted_at])
        if isinstance(right, RefusedSurfaceFact):
            refused[right.reason.value] += 1
        else:
            recovered[right.kind.value] += 1
        if not isinstance(left, AnalyticSurfaceFact) or not isinstance(right, AnalyticSurfaceFact):
            raise RuntimeError(f"face {native_at} did not retain an analytic surface fact")
        if left.kind is not right.kind or len(left.parameters) != len(right.parameters):
            raise RuntimeError(f"face {native_at} changed effective primitive kind")
        delta = _parameter_delta(left.kind, left.parameters, right.parameters)
        if delta > REVIEWED_DELTA_BOUNDS["effective_primitive_parameter"]:
            raise RuntimeError(
                f"face {native_at} effective parameter delta {delta!r} exceeds reviewed bound "
                f"{REVIEWED_DELTA_BOUNDS['effective_primitive_parameter']!r}"
            )
    return {
        "recovered_by_primitive": dict(sorted(recovered.items())),
        "refused_by_reason": dict(sorted(refused.items())),
        "ambiguous": refused.get("ambiguous-primitive", 0),
        "kind_correspondence_preserved": True,
        "parameters_within_reviewed_bounds": True,
    }


def _reported_distance(value: float) -> float:
    """Nine significant display digits; never used by a recognition or acceptance predicate.

    Recovery and probe distances inherit B-spline area/perimeter quadrature noise. Pinning
    their final binary digits makes equivalent geometry fail this reporting-only comparison.
    """
    return float(f"{value:.9g}")


def _certificate(use) -> dict[str, Any]:
    surface = use.surface
    recovery = surface.certificate
    material = use.material_side
    return {
        "face": use.node.index,
        "kind": surface.kind.value,
        "surface_provenance": surface.provenance.value,
        "orientation_capability": surface.orientation.value,
        "recovery": None
        if recovery is None
        else {
            "authority": recovery.authority,
            "maximum_distance_bound": _reported_distance(recovery.maximum_distance_bound),
            "occt_version": recovery.occt_version,
        },
        "material_side": None
        if material is None
        else {
            "authority": material.authority,
            "classifier_tolerance": material.classifier_tolerance,
            "outward": list(material.outward),
            "probe_distance": _reported_distance(material.probe_distance),
            "samples": len(material.sample_points),
        },
    }


def _occurrences(part) -> tuple[Occurrence, ...]:
    product = _take_inventory(part)
    result = []
    for candidate in product.accepted.candidate_set(FamilyId.PADS).candidates:
        result.append(
            Occurrence(
                candidate.record.to_dict(),
                frozenset(node.index for node in product.evidence.defining_of(candidate)),
                tuple(_certificate(use) for use in candidate.evidence.surfaces),
            )
        )
    return tuple(result)


def _compare_occurrences(
    native: tuple[Occurrence, ...],
    converted: tuple[Occurrence, ...],
    correspondence: tuple[int, ...],
) -> dict[str, Any]:
    available = set(range(len(converted)))
    same: list[tuple[int, int]] = []
    changed: list[tuple[int, int]] = []
    absent = []
    for native_at, occurrence in enumerate(native):
        mapped = frozenset(correspondence[index] for index in occurrence.defining)
        matches = tuple(
            converted_at
            for converted_at in sorted(available)
            if converted[converted_at].defining == mapped
        )
        if len(matches) != 1:
            absent.append(native_at)
            continue
        converted_at = matches[0]
        available.remove(converted_at)
        if occurrence.record == converted[converted_at].record:
            same.append((native_at, converted_at))
        else:
            changed.append((native_at, converted_at))
    return {
        "native_occurrences": len(native),
        "converted_occurrences": len(converted),
        "same_family": len(same),
        "reclassified": 0,
        "changed_record": len(changed),
        "absent": len(absent),
        "introduced": len(available),
        "native_records": [occurrence.record for occurrence in native],
        "converted_records": [occurrence.record for occurrence in converted],
        "converted_provenance": [list(converted[index].provenance) for _native, index in same],
    }


def sweep() -> dict[str, Any]:
    fixtures: dict[str, Any] = {}
    totals: Counter[str] = Counter()
    recovered: Counter[str] = Counter()
    refused: Counter[str] = Counter()
    for fixture_path in sorted(GOLDEN_ROOT.glob("*/fixture.py")):
        if fixture_path.parent.name in EXCLUDED_FIXTURES:
            continue
        native = load_fixture(fixture_path).build_fixture()
        converted, converter = _convert(native)
        correspondence = _face_correspondence(native, converted, converter)
        topology = _validate_topology(native, converted, correspondence)
        surfaces = _surface_report(native, converted, correspondence)
        occurrences = _compare_occurrences(
            _occurrences(native), _occurrences(converted), correspondence
        )
        for field in (
            "native_occurrences",
            "converted_occurrences",
            "same_family",
            "reclassified",
            "changed_record",
            "absent",
            "introduced",
        ):
            totals[field] += occurrences[field]
        recovered.update(surfaces["recovered_by_primitive"])
        refused.update(surfaces["refused_by_reason"])
        fixtures[fixture_path.parent.name] = {
            "faces": len(correspondence),
            "topology": topology,
            "surfaces": surfaces,
            "raised_pads": occurrences,
        }
    return {
        "schema": 2,
        "baseline_commit": BASELINE_COMMIT,
        "face_correspondence": "OCCT BRepBuilderAPI_NurbsConvert.ModifiedShape one-to-one history",
        "reviewed_delta_bounds": REVIEWED_DELTA_BOUNDS,
        "performance_budget_seconds": PERFORMANCE_BUDGET_SECONDS,
        "performance_measurement": PERFORMANCE_MEASUREMENT,
        "excluded_fixtures": EXCLUDED_FIXTURES,
        "totals": {
            **dict(totals),
            "fixtures": len(fixtures),
            "faces": sum(fixture["faces"] for fixture in fixtures.values()),
            "recovered_by_primitive": dict(sorted(recovered.items())),
            "refused_by_reason": dict(sorted(refused.items())),
        },
        "fixtures": fixtures,
    }


def markdown(report: dict[str, Any]) -> str:
    total = report["totals"]
    lines = [
        "# Golden-corpus NURBS-conversion sweep",
        "",
        f"Native baseline: `{report['baseline_commit']}`. Generated by",
        "`tools/nurbs_conversion_sweep.py`.",
        "",
        "Face correspondence is established first from OCCT's one-to-one `ModifiedShape`",
        "conversion history, then checked for boundary structure, adjacency and orientation.",
        "Occurrence results compare exact mapped defining-face sets and complete Raised Pad",
        "records.",
        "",
        "| fixtures | faces | native | converted | same | changed | absent | introduced |",
        "| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
        f"| {total['fixtures']} | {total['faces']} | {total['native_occurrences']} | "
        f"{total['converted_occurrences']} | {total['same_family']} | "
        f"{total['changed_record']} | {total['absent']} | {total['introduced']} |",
        "",
        "Recovered primitives: "
        + ", ".join(f"{kind} {count}" for kind, count in total["recovered_by_primitive"].items())
        + ".",
        f"Refused facts: {sum(total['refused_by_reason'].values())}.",
        "",
        "Reviewed representation-delta bounds enforced across every face:",
        "",
        f"- evaluated face centre: `{report['reviewed_delta_bounds']['face_centre_model_units']}` "
        "model units",
        f"- absolute face area: "
        f"`{report['reviewed_delta_bounds']['absolute_face_area_square_units']}` square units",
        f"- relative face area: `{report['reviewed_delta_bounds']['relative_face_area']}`",
        f"- effective primitive parameter: "
        f"`{report['reviewed_delta_bounds']['effective_primitive_parameter']}`",
        "",
        "Recovery maximum-distance bounds and material-side probe distances are displayed",
        "to nine significant digits in the JSON provenance. This suppresses insignificant",
        "quadrature drift; full-precision certificates and recognition checks are unchanged.",
        "Counts, identities, decisions and the reviewed delta thresholds remain exact.",
        "",
        "## Performance gate",
        "",
        f"Recorded on {report['performance_measurement']['measured_at']} under "
        f"{report['performance_measurement']['environment']}: median "
        f"`{report['performance_measurement']['median_seconds']}` seconds over "
        f"{report['performance_measurement']['repeat']} repeats.",
        "",
        f"`--benchmark` measures the complete native/converted Pad workload and fails above "
        f"`{report['performance_budget_seconds']}` seconds (median of the requested repeats).",
        "Conversion itself is excluded: it belongs to the importing pipeline, not recognition.",
        "Timing is kept out of this report because a byte-identical artifact cannot contain live",
        "wall-clock measurements.",
        "",
        "## Scope",
        "",
        "This proves exact OCCT-converted analytic geometry for Raised Pads only. Other families,",
        "torus recovery, approximate reverse-engineered NURBS and third-party exporter behaviour",
        "remain outside the claim.",
        "",
    ]
    return "\n".join(lines)


def benchmark(*, repeat: int) -> dict[str, Any]:
    native = [
        load_fixture(path).build_fixture()
        for path in sorted(GOLDEN_ROOT.glob("*/fixture.py"))
        if path.parent.name not in EXCLUDED_FIXTURES
    ]
    converted = [_convert(part)[0] for part in native]

    def workload() -> None:
        for parts in (native, converted):
            for part in parts:
                graph = FaceGraph(part)
                surfaces = EffectiveSurfaceIndex(graph)
                for node in graph.nodes:
                    surfaces.fact(node)
                recognise_rectangular_pads(part)
                _ = _take_inventory(part).result.pads

    samples = []
    for _ in range(repeat):
        started = perf_counter()
        workload()
        samples.append(perf_counter() - started)
    median = statistics.median(samples)
    return {
        "environment": f"Python {sys.version.split()[0]}, {sys.platform}",
        "fixtures": len(native),
        "repeat": repeat,
        "median_seconds": median,
        "budget_seconds": PERFORMANCE_BUDGET_SECONDS,
        "passed": median <= PERFORMANCE_BUDGET_SECONDS,
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--write", action="store_true", help="update checked-in JSON/Markdown")
    parser.add_argument("--benchmark", action="store_true", help="run the reviewed time budget")
    parser.add_argument("--repeat", type=int, default=3)
    args = parser.parse_args()
    if args.repeat < 1:
        parser.error("--repeat must be positive")
    if args.benchmark:
        measured = benchmark(repeat=args.repeat)
        print(json.dumps(measured, indent=2, sort_keys=True))
        if not measured["passed"]:
            raise SystemExit(1)
        return
    report = sweep()
    rendered = json.dumps(report, indent=2, sort_keys=True) + "\n"
    if args.write:
        JSON_REPORT.write_text(rendered, encoding="utf-8")
        MARKDOWN_REPORT.write_text(markdown(report), encoding="utf-8")
    else:
        print(rendered, end="")


if __name__ == "__main__":
    main()
