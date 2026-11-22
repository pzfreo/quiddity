# Fillet chain line crease

Original synthetic Apache-2.0 fixture for quiddity #341. No external geometry is used.
Run `tools/make_fillet_chain_crease.py` with build123d 0.13.0 to regenerate it.

Two overlapping boxes have opposed R2 convex corner rounds and are fused along their overlap.
Each round keeps both tangent walls outside the overlap. The cylindrical faces (2 and 7)
intersect transversely along two straight edges (12 and 14). They belong to separate chains
and the same radius group.
