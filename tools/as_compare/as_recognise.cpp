// Run Analysis Situs' open-source recognisers (holes, blends, cavities) on one STEP
// file and print one JSON line of 0-based face indices (TopExp::MapShapes order).
#include <asiAlgo_AAG.h>
#include <asiAlgo_RecognizeBlends.h>
#include <asiAlgo_RecognizeCavities.h>
#include <asiAlgo_RecognizeConvexHull.h>
#include <asiAlgo_RecognizeShafts.h>
#include <asiAlgo_RecognizeDrillHoles.h>

#include <IFSelect_ReturnStatus.hxx>
#include <Precision.hxx>
#include <STEPControl_Reader.hxx>
#include <TColStd_PackedMapOfInteger.hxx>
#include <TopExp.hxx>
#include <TopTools_IndexedMapOfShape.hxx>

#include <algorithm>
#include <ctime>
#include <chrono>
#include <vector>
#include <cstdio>
#include <iostream>
#include <sstream>

static std::string ids(const TColStd_PackedMapOfInteger& m)
{
  std::vector<int> v;
  for (TColStd_PackedMapOfInteger::Iterator it(m); it.More(); it.Next())
    v.push_back(it.Key() - 1);
  std::sort(v.begin(), v.end());
  std::ostringstream o;
  o << "[";
  for (size_t i = 0; i < v.size(); ++i) o << (i ? "," : "") << v[i];
  o << "]";
  return o.str();
}

// CPU seconds of this process (single-threaded recognition; OCCT reader may use more threads,
// which this counts too).
static double cpu()
{
  timespec ts;
  clock_gettime(CLOCK_PROCESS_CPUTIME_ID, &ts);
  return ts.tv_sec + ts.tv_nsec * 1e-9;
}

static double since(std::chrono::steady_clock::time_point t)
{
  return std::chrono::duration<double>(std::chrono::steady_clock::now() - t).count();
}

int main(int argc, char** argv)
{
  if (argc != 3) { std::cerr << "usage: as_recognise ID FILE.step\n"; return 2; }
  std::ostringstream out;
  out << "{\"id\":\"" << argv[1] << "\"";
  const double c0 = cpu();
  STEPControl_Reader reader;
  if (reader.ReadFile(argv[2]) != IFSelect_RetDone || reader.TransferRoots() == 0) {
    std::cout << out.str() << ",\"status\":\"read_failed\"}" << std::endl;
    return 0;
  }
  TopoDS_Shape shape = reader.OneShape();
  TopTools_IndexedMapOfShape faces;
  TopExp::MapShapes(shape, TopAbs_FACE, faces);
  out << ",\"faces\":" << faces.Extent();
  const double c1 = cpu();
  out << ",\"read_cpu_s\":" << (c1 - c0);

  auto t = std::chrono::steady_clock::now();
  Handle(asiAlgo_AAG) aag = new asiAlgo_AAG(shape);
  out << ",\"aag_s\":" << since(t);

  try {
    t = std::chrono::steady_clock::now();
    asiAlgo_RecognizeDrillHoles holes(aag, true);
    bool ok = holes.Perform(Precision::Infinite());
    out << ",\"holes\":" << (ok ? ids(holes.GetResultIndices()) : "null") << ",\"holes_s\":" << since(t);
  } catch (...) { out << ",\"holes\":\"exception\""; }

  try {
    t = std::chrono::steady_clock::now();
    asiAlgo_RecognizeBlends blends(shape, aag);
    bool ok = blends.Perform(1e100);
    out << ",\"blends\":" << (ok ? ids(blends.GetResultIndices()) : "null") << ",\"blends_s\":" << since(t);
  } catch (...) { out << ",\"blends\":\"exception\""; }

  try {
    t = std::chrono::steady_clock::now();
    asiAlgo_RecognizeCavities cavities(aag);
    cavities.SetMaxSize(Precision::Infinite());
    cavities.SetMaxVolume(Precision::Infinite());
    bool ok = cavities.Perform();
    out << ",\"cavities\":" << (ok ? ids(cavities.GetResultIndices()) : "null") << ",\"cavities_s\":" << since(t);
  } catch (...) { out << ",\"cavities\":\"exception\""; }

  out << ",\"recognise3_cpu_s\":" << (cpu() - c1);

  // Defaults of the recognize-convex-hull Tcl command: tolerance 0.1, 20 probe points.
  try {
    t = std::chrono::steady_clock::now();
    asiAlgo_RecognizeConvexHull hull(aag);
    hull.SetGridResolution(20);
    hull.SetTolerance(0.1);
    bool ok = hull.Perform();
    out << ",\"hull\":" << (ok ? ids(hull.GetResultIndices()) : "null") << ",\"hull_s\":" << since(t);
  } catch (...) { out << ",\"hull\":\"exception\""; }

  try {
    t = std::chrono::steady_clock::now();
    asiAlgo_RecognizeShafts shafts(aag);
    bool ok = shafts.Perform(Precision::Infinite());
    out << ",\"shafts\":" << (ok ? ids(shafts.GetResultIndices()) : "null") << ",\"shafts_s\":" << since(t);
  } catch (...) { out << ",\"shafts\":\"exception\""; }

  out << ",\"recognise_cpu_s\":" << (cpu() - c1);
  out << ",\"status\":\"ok\"}";
  std::cout << out.str() << std::endl;
  return 0;
}
