"""In-process haecceity PMI JSON ↔ AP242 STEP, with verified byte-buffer writes."""
import json
import os
from importlib.resources import files

from ._native import PmiError, pmi_read, pmi_validate, pmi_write

SCHEMA = json.loads(files(__package__).joinpath("schema/haecceity.pmi-1.json").read_text(encoding="utf-8"))
PRESENTATION_SCHEMA = json.loads(files(__package__).joinpath("schema/haecceity.presentation-1.json").read_text(encoding="utf-8"))


def _document(document: str | dict) -> str:
    if isinstance(document, str):
        return document
    try:
        if not isinstance(document, dict):
            raise ValueError("document must be a JSON string or dict")
        return json.dumps(document, allow_nan=False)
    except (TypeError, ValueError) as error:
        failure = PmiError(str(error))
        failure.findings = []
        raise failure from error


def _step(step: str | os.PathLike | bytes) -> str | bytes:
    return step if isinstance(step, bytes) else os.fsdecode(os.fspath(step))


def _part(part: int | None) -> int | None:
    if part is not None and (type(part) is not int or not 0 <= part < 2**64):
        failure = PmiError("part must be a non-negative part index")
        failure.findings = []
        raise failure
    return part


def write(geometry: str | os.PathLike | bytes, document: str | dict, *, part: int | None = None, report: bool = False, presentation: str | dict | None = None) -> bytes | tuple[bytes, dict]:
    """Return verified AP242 bytes, or (bytes, import-report dict) with report=True; no files written."""
    output, details = pmi_write(_step(geometry), _document(document), _part(part), report, None if presentation is None else _document(presentation))
    return (output, json.loads(details)) if report else output


def read(step: str | os.PathLike | bytes, *, part: int | None = None, anchors: str = "index") -> dict:
    """Read haecceity.pmi/1. Name mode refuses missing or ambiguous referenced face names."""
    if anchors not in ("index", "name"):
        failure = PmiError("anchors must be 'index' or 'name'")
        failure.findings = []
        raise failure
    return json.loads(pmi_read(_step(step), _part(part), names=anchors == "name"))


def presentation(step: str | os.PathLike | bytes, *, part: int | None = None) -> dict:
    """Read the file's haecceity.presentation/1 document, including generated callouts."""
    return json.loads(pmi_read(_step(step), _part(part), presentation=True))


def validate(document: str | dict) -> None:
    """Validate schema, references and geometry-independent authoring refusals."""
    pmi_validate(_document(document))
