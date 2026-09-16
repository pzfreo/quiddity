# SPDX-License-Identifier: Apache-2.0
# Copyright 2024-2026 Paul Fremantle
"""Recognition of slots, pockets, channels, and their derived patterns.

This module is the supported compatibility facade; implementation modules remain private.
"""

from quiddity._candidates import CompletedInputs, DerivedId, FamilyId
from quiddity._definitions import (
    AcceptedInputs,
    Counted,
    DerivedDefinition,
    DiscoveryServices,
    FullyAttributed,
    ManifestEvidence,
    NotCounted,
    PhysicalDefinition,
    always,
)
from quiddity._recess_features import (
    _discover_channels,
    _discover_pockets,
    _discover_slots,
    recognise_channels,
    recognise_pockets,
    recognise_slots,
)
from quiddity._recess_patterns import (
    recognise_pocket_patterns,
    recognise_slot_patterns,
)
from quiddity._recess_records import (
    Channel,
    Pocket,
    PocketArray,
    PocketGrid,
    Slot,
    SlotArray,
    SlotGrid,
)
from quiddity._typing import Vector3 as Vector3

__all__ = [
    "Channel",
    "Pocket",
    "PocketArray",
    "PocketGrid",
    "Slot",
    "SlotArray",
    "SlotGrid",
    "recognise_channels",
    "recognise_pocket_patterns",
    "recognise_pockets",
    "recognise_slot_patterns",
    "recognise_slots",
]

for _exported_name in __all__:
    globals()[_exported_name].__module__ = __name__


# This module is the public face of three families, so each declaration is named after its family
# rather than `DEFINITION`, as `polygonal_bosses` and `levels` do. Their records stay in
# `_recess_records`, which the recess machinery depends on; see RECORDS_DEFINED_NEXT_DOOR.
def _discover_slot_family(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs  # no completed predecessors
    return list(
        _discover_slots(
            services.context.part,
            writer=services.writer,
            face_edges=services.context.face_edges,
        )
    )


def _discover_pocket_family(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs  # no completed predecessors
    return list(
        _discover_pockets(
            services.context.part,
            writer=services.writer,
            face_edges=services.context.face_edges,
        )
    )


def _discover_channel_family(services: DiscoveryServices, inputs: CompletedInputs) -> list[object]:
    del inputs  # no completed predecessors
    return list(
        _discover_channels(
            services.context.part,
            face_edges=services.context.face_edges,
            writer=services.writer,
        )
    )


def _derive_slot_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_slot_patterns(inputs.records(FamilyId.SLOTS, Slot)))


def _derive_pocket_patterns(inputs: AcceptedInputs) -> list[object]:
    return list(recognise_pocket_patterns(inputs.records(FamilyId.POCKETS, Pocket)))


SLOTS = PhysicalDefinition(
    family=FamilyId.SLOTS,
    record_types=(Slot,),
    result_field="slots",
    public_entrypoint=recognise_slots.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover_slot_family,
    census=Counted("slot"),
    attribution=FullyAttributed(
        "every returned Slot owns its complete selected wall and cap faces"
    ),
    evidence=ManifestEvidence(goldens=("straight_and_obround_slots",)),
)

# The package exports neither this entry point nor the two below it, so those three families
# have no capability manifest entry and declare no evidence.
POCKETS = PhysicalDefinition(
    family=FamilyId.POCKETS,
    record_types=(Pocket,),
    result_field="pockets",
    public_entrypoint=recognise_pockets.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover_pocket_family,
    census=NotCounted("Counted once through the unified section_recess projection"),
    attribution=FullyAttributed(
        "every returned Pocket owns its selected walls, corner floor, or caps"
    ),
)

CHANNELS = PhysicalDefinition(
    family=FamilyId.CHANNELS,
    record_types=(Channel,),
    result_field="channels",
    public_entrypoint=recognise_channels.__name__,
    dependencies=(),
    applicable=always,
    discover=_discover_channel_family,
    census=NotCounted("Counted once through the unified section_recess projection"),
    attribution=FullyAttributed(
        "every returned Channel owns its exact two opposed side-wall faces"
    ),
)

SLOT_PATTERNS = DerivedDefinition(
    identifier=DerivedId.SLOT_PATTERNS,
    record_types=(SlotArray, SlotGrid),
    result_field="slot_patterns",
    public_entrypoint=recognise_slot_patterns.__name__,
    sources=(FamilyId.SLOTS,),
    derive=_derive_slot_patterns,
    census=NotCounted("not a distinct census key"),
    evidence=ManifestEvidence(goldens=("straight_and_obround_slots",)),
)

POCKET_PATTERNS = DerivedDefinition(
    identifier=DerivedId.POCKET_PATTERNS,
    record_types=(PocketArray, PocketGrid),
    result_field="pocket_patterns",
    public_entrypoint=recognise_pocket_patterns.__name__,
    sources=(FamilyId.POCKETS,),
    derive=_derive_pocket_patterns,
    census=NotCounted("not a distinct census key"),
)
