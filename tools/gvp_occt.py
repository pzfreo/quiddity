"""Independent OCCT GProps measurements. No tessellation, density=1, millimetres.

Only cadquery-ocp-novtk and the Python standard library are needed. XCAF visits unique
part definitions in assembly traversal order, as StepFile.parts does. Faces follow
TopExp.MapShapes, the numbering contract of StepFile.part. Mismatched counts fail.
"""
import gzip
import re
import tempfile
import OCP
from OCP.STEPCAFControl import STEPCAFControl_Reader
from OCP.TDocStd import TDocStd_Document
from OCP.TCollection import TCollection_ExtendedString, TCollection_AsciiString
from OCP.XCAFDoc import XCAFDoc_DocumentTool, XCAFDoc_ShapeTool
from OCP.TDF import TDF_Label, TDF_Tool
try:
    from OCP.TDF import TDF_LabelSequence
    from OCP.TopTools import TopTools_IndexedMapOfShape
except ImportError:  # OCP 8 renamed the collection wrappers.
    from OCP.collections import Sequence_TDF_Label as TDF_LabelSequence
    from OCP.collections import IndexedMap_TopoDS_Shape_TopTools_ShapeMapHasher as TopTools_IndexedMapOfShape
from OCP.TopExp import TopExp
from OCP.TopAbs import TopAbs_FACE, TopAbs_SOLID
from OCP.TopoDS import TopoDS_Compound
from OCP.BRep import BRep_Builder
from OCP.BRepGProp import BRepGProp
from OCP.GProp import GProp_GProps

EPS = 1e-9


def indexed(shape, kind):
    result = TopTools_IndexedMapOfShape()
    TopExp.MapShapes_s(shape, kind, result)
    return [result.FindKey(i) for i in range(1, result.Extent()+1)]


def compound(shapes):
    builder = BRep_Builder()
    result = TopoDS_Compound()
    builder.MakeCompound(result)
    for shape in shapes:
        builder.Add(result, shape)
    return result


def measure(shape, material=True):
    area, volume = GProp_GProps(), GProp_GProps()
    BRepGProp.SurfaceProperties_s(shape, area, EPS, False)
    if material:
        BRepGProp.VolumeProperties_s(shape, volume, EPS, False, False)
    return dict(volume=volume.Mass() if material else None, area=area.Mass(),
                centroid=list(volume.CentreOfMass().Coord()) if material and volume.Mass() else None)


def load(path):
    reader = STEPCAFControl_Reader()
    raw = open(path, 'rb').read()
    if raw[:2] == b'\x1f\x8b':
        raw = gzip.decompress(raw)
    with tempfile.NamedTemporaryFile(suffix='.stp') as temporary:
        temporary.write(raw)
        temporary.flush()
        if int(reader.ReadFile(temporary.name)) != 1:
            raise ValueError('OCCT ReadFile failed')
    document = TDocStd_Document(TCollection_ExtendedString('BinXCAF'))
    if not reader.Transfer(document):
        raise ValueError('OCCT transfer failed')
    shapes = XCAFDoc_DocumentTool.ShapeTool_s(document.Main())
    roots = TDF_LabelSequence()
    shapes.GetFreeShapes(roots)
    parts, seen = [], set()

    def visit(label):
        if XCAFDoc_ShapeTool.IsReference_s(label):
            referred = TDF_Label()
            XCAFDoc_ShapeTool.GetReferredShape_s(label, referred)
            label = referred
        entry = TCollection_AsciiString()
        TDF_Tool.Entry_s(label, entry)
        key = entry.ToCString()
        if key in seen:
            return
        seen.add(key)
        if XCAFDoc_ShapeTool.IsAssembly_s(label):
            children = TDF_LabelSequence()
            XCAFDoc_ShapeTool.GetComponents_s(label, children)
            for i in range(1, children.Length()+1):
                visit(children.Value(i))
        else:
            shape = XCAFDoc_ShapeTool.GetShape_s(label)
            if not shape.IsNull() and indexed(shape, TopAbs_FACE):
                parts.append(shape)
    for i in range(1, roots.Length()+1):
        visit(roots.Value(i))
    # OCCT can split the independent sheets of one product into additional free
    # XCAF roots. Regroup only when the STEP itself declares exactly one product.
    declarations = re.finditer(rb"'(?:[^']|'')*'|/\*.*?\*/|#\s*\d+\s*=\s*(PRODUCT_DEFINITION(?:_WITH_ASSOCIATED_DOCUMENTS)?)\s*\(", raw, re.S | re.I)
    product_count=sum(m[1] is not None for m in declarations)
    return [compound(parts)] if product_count == 1 and len(parts)>1 else parts


def material_measure(shape):
    solids=indexed(shape, TopAbs_SOLID)
    result=measure(compound(solids)) if solids else dict(volume=0.0,centroid=None,no_solid=True)
    result['area']=measure(shape,False)['area']
    return result


def compare(row):
    parts = load(row['file'])
    row['occt_parts']=[dict(faces=len(indexed(shape,TopAbs_FACE)),reference=material_measure(shape)) for shape in parts]
    row['occt_version'] = OCP.__version__
    row['occt_tolerance'] = EPS
    if 'error' in row:
        return row
    if len(parts) != len(row['parts']):
        raise ValueError(f'part count: OCCT {len(parts)}, ours {len(row["parts"])}')
    for shape, part in zip(parts, row['parts'], strict=True):
        faces = indexed(shape, TopAbs_FACE)
        if len(faces) != len(part['faces']):
            raise ValueError(f'{part["name"]}: face count OCCT {len(faces)}, ours {len(part["faces"])}')
        part['occt'] = row['occt_parts'][part['part']]['reference']
        cache = {}
        for prop in part['gvp']:
            if prop['faces']:
                material=prop['reference'].get('volume') is not None or prop['reference'].get('centroid') is not None
                key = (prop['scope'],prop.get('area_kind'),material,tuple(prop['faces']),prop['reference'].get('area'))
                if key not in cache:
                    if prop['scope']=='product':
                        cache[key] = dict(part['occt'])
                        solids = indexed(shape, TopAbs_SOLID)
                        sheets = solids and len(indexed(compound(solids), TopAbs_FACE)) < len(faces)
                        if sheets:
                            # Same scope rule as ours, chosen independently from OCCT's values.
                            solid_area = measure(compound(solids), False)['area']
                            reference = prop['reference'].get('area')
                            use_solids = prop.get('area_kind') == 'wetted' or reference is None or abs(solid_area-reference) <= abs(cache[key]['area']-reference)
                            if use_solids:
                                cache[key]['area'] = solid_area
                            cache[key]['area_scope'] = 'solids' if use_solids else 'solids and sheets'
                    else:
                        cache[key] = measure(compound([faces[i] for i in key[3]]),material)
                prop['occt'] = cache[key]
    row['occt_version'] = OCP.__version__
    row['occt_tolerance'] = EPS
    return row
