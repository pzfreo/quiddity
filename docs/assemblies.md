# Reading assemblies by product

A manifest lets quiddity recognise a large STEP assembly one distinct product at a
time. The scan indexes the source; each recognition reads only the header, common
records and selected product ranges. Repeated instances share one result in the
product's own coordinates. Their placements are separate transforms.

```sh
quiddity manifest big.step -o big.manifest.json
quiddity recognise big.step --manifest big.manifest.json --out-dir results/
quiddity recognise big.step --manifest big.manifest.json --out-dir worker/ --parts p0,p7
```

The existing `quiddity big.step` command is unchanged. `recognise` requires a
manifest and output directory; it does not replace that command.

## Scan and manifest format

`manifest` writes only `-o`. By default its entity index lives in memory. With
`--index-dir DIR`, the extent/union index uses a temporary mapped file in that
existing directory, deleted on success or error. Scanning makes two forward
passes over a seekable source, retaining structure rather than all geometry
text. Product and common hashes are included by default.

Gzip sources can be scanned. `source.compressed: true` records that offsets and
hashes refer to the **uncompressed** bytes. Product recognition refuses compressed
sources, including gzip detected by its magic bytes. Explicitly decompress and
scan the resulting file before using range reads; nothing is unpacked implicitly.

The JSON format is `quiddity-manifest/1`. Ranges are half-open `[start, end)` byte
offsets into the uncompressed source. Product ranges are sorted, non-overlapping
and coalesced across whitespace and comments.

| Field | Meaning |
| --- | --- |
| `format` | Manifest format identifier; incompatible formats are refused |
| `source` | Whole uncompressed `size` and `sha256`, `header` range, DATA section `data` ranges, and optional `compressed` flag (absent means false) |
| `reader.haecceity` | Reader version; another version is refused and requires a rescan |
| `common` | Shared contexts, units and protocol records: `ranges`, `entities`, `sha256`; the hash also covers the header bytes |
| `products` | Distinct geometric products in reader part order, with ids `p0`, `p1`, … |
| `products[].product_definition`, `shape` | STEP entity numbers for the product definition and product definition shape |
| `products[].name`, `product_id` | Product name and STEP PRODUCT identifier |
| `products[].ranges`, `entities`, `bytes` | Records needed to read this product, their count and summed range lengths; includes the product definition chain |
| `products[].sha256` | SHA-256 of product range bytes concatenated in ascending offset order |
| `products[].faces` | Part face index → ADVANCED_FACE entity number |
| `products[].edge_count` | Distinct EDGE_CURVE count; stage 1 does not publish an ordered edge map |
| `assemblies` | Assembly `product_definition`, `name`, and `components` containing occurrence and child definition entity numbers |
| `instances` | `product`, occurrence-chain `path`, and 3×4 `placement` matrix `[R \| t]`, translation in millimetres; one entry per placed solid/surface-model instance |
| `unowned.presentation`, `unowned.other` | Unread presentation/PMI and other records, as ranges and entity counts |
| `refusals` | Shared geometry: affected `products`, shared `entity` number and `reason` |

Product ids are local to this source and reader. They are worker selection keys,
not persistent identities across revisions. The manifest is metadata, not a STEP
copy, and needs its bound source file.

## Recognition outputs and staleness

`recognise --manifest` writes beneath `--out-dir`:

- `products/<id>.json`: the existing recognition JSON envelope for each successfully
  read product, in its own coordinates, with part face indices. Its source digest
  binds the original whole source. No placed copies are generated.
- `assembly.json`: the manifest's `source` binding and `instances` for the selected
  products, retaining `path` and `placement`.
- `refusals.json`: an array of `{product, reason, recovered}`. Empty on complete
  success without fallback. Every selected product that cannot be read is named.

`--parts p0,p7` selects manifest ids; all unknown ids are reported before writing.
Results follow manifest order. Omit `--parts` for every product. Use separate
output directories for independent workers or runs with different selections.
A rerun removes an existing result for a selected product that is now refused.
Other files from previous selections are not removed.

The default read checks file size, common/header hash, selected product hashes,
face entity order and edge count. A changed byte in a selected product's ranges
refuses that product. Changes outside the header/common/selected ranges can pass.
`--verify-source` also reads and hashes the whole source, detecting changes to
assembly, presentation, comments and unselected products. It fails before writing
if the whole-source binding differs. Rescan after editing the source.

`--no-product-hashes` disables both product and common/header hash comparisons;
staleness checking then uses size only, unless `--verify-source` is also supplied.
Structural validation still applies. This flag does not avoid the selected range
reads needed for geometry. The lower-level reader also accepts manifests scanned
without hashes; missing hashes have the same size-only limitation.

A source/manifest/IO error exits 1 with `quiddity: …` on stderr. Per-product
refusals are written to `refusals.json`, also reported on stderr, and exit 1 after
other selected products have been attempted. Success exits 0 with no stdout;
clap usage errors exit 2. Outputs may be partial after an IO failure.

## Shared geometry and fallback

When two products reach the same geometry entity, range reads refuse both and
name the shared `#N`. The default CLI lists these refusals, with a retry instruction.
For a complete result set, explicitly permit a whole-file parse:

```sh
quiddity recognise big.step --manifest big.manifest.json --out-dir results/ \
  --fallback-whole-file
```

The flag parses the whole file once for the selected shared-geometry products,
then reads each part in its own coordinates. It always verifies the full source
binding for that parse, even with `--no-product-hashes`. Memory now grows with
the entire source, so fallback is opt-in. Successfully recovered range refusals
remain in `refusals.json` with `recovered: true`; they do not cause a failing exit.
Hash mismatches and other read errors never trigger fallback.

## Mapping results back to STEP

Face index `i` in `products/p0.json` identifies
`ADVANCED_FACE #manifest.products[0].faces[i]`. For each instance of `p0`, place
that face using `instances[k].placement`. Positions transform as `R*x + t`;
directions use `R` only. The path identifies the occurrence chain from the root.
This mapping is exact within the bound file. Across revisions, use
[`correspond`](correspondence.md) on product recognition results.

## Python

```python
from pathlib import Path
import quiddity

m = quiddity.manifest(Path("big.step"))  # dict; no output file
quiddity.manifest("big.step", out="big.manifest.json")
p0 = quiddity.recognise_product("big.step", m, "p0")  # dict
for product_id, result in quiddity.recognise_products(
    "big.step", "big.manifest.json", parts=["p0", "p7"]
):
    print(product_id, result["document"]["face_count"])
```

All paths accept `str` or `os.PathLike`. A `manifest` argument accepts a dict or a
JSON file path, not inline JSON text. Recognition returns the same envelope as
`json.loads(quiddity.recognise(...))` for that part; existing `recognise` continues
to return JSON text. `recognise_products` is a lazy iterator of `(id, result_dict)`
in manifest order: it validates the selection before reading the first product,
then reads and recognises one at each iteration. Unknown ids and reader refusals
raise `ValueError`; a refusal stops iteration at that product. Use individual
calls to collect successes around refusals. These Python functions do not perform
whole-file fallback. Scan, read and recognition run with the GIL released.

## Limits and scope

Recognition memory grows with the largest selected product and its parsed
geometry, plus manifest metadata. The scan's extent/union index uses 16 bytes per
entity, with additional ownership, references, retained structure and range
metadata; `--index-dir` maps that index, not all scan allocations. Many short
ranges increase seek and metadata costs. The manifest scan defaults to one million
visited definitions/emitted instances; the existing geometry reader's depth,
instance and face limits still apply to each product read.

Stage 1 records presentation/PMI ranges but does not read PMI or presentation per
product. It does not split STEP, export placed copies, or provide cross-revision
product identity. Existing whole-file PMI and B-rep commands remain available.
