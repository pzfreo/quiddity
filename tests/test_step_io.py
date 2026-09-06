# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle

from __future__ import annotations

import ast
from pathlib import Path

import pytest
from build123d import Box, Compound, Pos, export_step, import_step
from OCP.IFSelect import IFSelect_ReturnStatus
from OCP.STEPControl import STEPControl_AsIs, STEPControl_Reader, STEPControl_Writer

import quiddity.step_io as step_io
from quiddity import import_step_geometry

ROOT = Path(__file__).parents[1]
MFCADPP_FIXTURE = ROOT / "tests/corpus/mfcadpp/1000.step"


def _face_signature(part) -> tuple[tuple[object, ...], ...]:
    """Ordered geometric signature used only to compare two independent imports."""

    return tuple(
        (
            face.geom_type,
            round(float(face.area), 8),
            *(round(float(value), 8) for value in face.center()),
        )
        for face in part.faces()
    )


def test_geometry_loader_preserves_face_order_against_build123d_importer() -> None:
    metadata_import = import_step(MFCADPP_FIXTURE)
    geometry_import = import_step_geometry(MFCADPP_FIXTURE)

    assert type(geometry_import) is type(metadata_import)
    assert len(geometry_import.solids()) == len(metadata_import.solids()) == 1
    assert _face_signature(geometry_import) == _face_signature(metadata_import)


def test_geometry_loader_flattens_multiple_roots_without_losing_solids(tmp_path) -> None:
    source = Compound(children=[Box(2, 3, 4), Pos(10, 0, 0) * Box(5, 6, 7)])
    target = tmp_path / "two-solids.step"
    assert export_step(source, target)

    metadata_import = import_step(target)
    geometry_import = import_step_geometry(target)

    assert len(geometry_import.solids()) == len(metadata_import.solids()) == 2
    assert _face_signature(geometry_import) == _face_signature(metadata_import)


def test_geometry_loader_does_not_call_the_xcaf_importer(monkeypatch) -> None:
    monkeypatch.setattr(
        "build123d.importers.import_step",
        lambda *_: pytest.fail("geometry-only loading must not traverse XCAF metadata"),
    )

    assert import_step_geometry(MFCADPP_FIXTURE).faces()


def test_geometry_loader_preserves_independent_transfer_roots(tmp_path) -> None:
    target = tmp_path / "independent-roots.step"
    writer = STEPControl_Writer()
    for solid in (Box(2, 3, 4), Pos(10, 0, 0) * Box(5, 6, 7)):
        status = writer.Transfer(solid.wrapped, STEPControl_AsIs)
        assert status == IFSelect_ReturnStatus.IFSelect_RetDone
    assert writer.Write(str(target)) == IFSelect_ReturnStatus.IFSelect_RetDone
    reader = STEPControl_Reader()
    assert reader.ReadFile(str(target)) == IFSelect_ReturnStatus.IFSelect_RetDone
    assert reader.NbRootsForTransfer() == 2
    result = import_step_geometry(target)
    assert sorted(round(solid.volume, 8) for solid in result.solids()) == [24.0, 210.0]


class _Reader:
    def __init__(
        self,
        *,
        status=IFSelect_ReturnStatus.IFSelect_RetDone,
        roots: int = 1,
        expected_roots: int | None = None,
        shape=None,
    ) -> None:
        self.status = status
        self.roots = roots
        self.expected_roots = roots if expected_roots is None else expected_roots
        self.shape = shape

    def ReadFile(self, _path):
        return self.status

    def TransferRoots(self):
        return self.roots

    def NbRootsForTransfer(self):
        return self.expected_roots

    def OneShape(self):
        return self.shape


def test_geometry_loader_reports_read_and_empty_transfer_failures(monkeypatch) -> None:
    monkeypatch.setattr(
        step_io,
        "STEPControl_Reader",
        lambda: _Reader(status=IFSelect_ReturnStatus.IFSelect_RetError),
    )
    with pytest.raises(ValueError, match="cannot read STEP geometry"):
        import_step_geometry("missing.step")

    monkeypatch.setattr(step_io, "STEPControl_Reader", lambda: _Reader(roots=0))
    with pytest.raises(ValueError, match="contains no transferable roots"):
        import_step_geometry("empty.step")


def test_geometry_loader_rejects_partial_transfer_before_accessing_shape(monkeypatch) -> None:
    class PartialReader(_Reader):
        def OneShape(self):
            pytest.fail("partial geometry must not be published")

    monkeypatch.setattr(
        step_io, "STEPControl_Reader", lambda: PartialReader(roots=1, expected_roots=2)
    )
    with pytest.raises(ValueError, match="transferred 1 of 2 roots; refusing incomplete geometry"):
        import_step_geometry("partial.step")


class _Topology:
    def __init__(self, *, null: bool) -> None:
        self.null = null

    def IsNull(self) -> bool:
        return self.null


def test_geometry_loader_rejects_null_and_unknown_transfers(monkeypatch) -> None:
    monkeypatch.setattr(step_io, "STEPControl_Reader", lambda: _Reader(shape=object()))
    monkeypatch.setattr(step_io, "downcast", lambda _shape: _Topology(null=True))
    with pytest.raises(ValueError, match="transferred a null shape"):
        import_step_geometry("null.step")

    monkeypatch.setattr(step_io, "downcast", lambda _shape: _Topology(null=False))
    with pytest.raises(ValueError, match="transferred unsupported topology _Topology"):
        import_step_geometry("unknown.step")


def test_geometry_loader_is_the_root_public_object() -> None:
    assert import_step_geometry is step_io.import_step_geometry


def test_repository_corpus_tools_do_not_import_metadata_step_loader() -> None:
    offenders: list[str] = []
    for path in sorted((ROOT / "tools").glob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if isinstance(node, ast.ImportFrom) and node.module == "build123d" and any(
                alias.name == "import_step" for alias in node.names
            ):
                offenders.append(path.name)
            if (
                isinstance(node, ast.Attribute)
                and isinstance(node.value, ast.Name)
                and node.value.id == "build123d"
                and node.attr == "import_step"
            ):
                offenders.append(path.name)
    assert offenders == []
