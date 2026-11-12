//! CPython abi3 boundary. Geometry work runs with the GIL released.
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};
use quiddity::{Part, file_api, kernel::step, serve};
use std::panic::{AssertUnwindSafe, catch_unwind};

mod pmi;

pyo3::create_exception!(quiddity, QuiddityError, PyRuntimeError);

/// Catch before PyO3 can turn an unwind into a BaseException. All off-GIL
/// work, including manifest and PMI operations, goes through this boundary.
fn detached<T: Send>(py: Python<'_>, work: impl FnOnce() -> T + Send) -> PyResult<T> {
    catch_unwind(AssertUnwindSafe(|| py.detach(work))).map_err(|payload| {
        let message = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied())
            .unwrap_or("non-string panic payload");
        QuiddityError::new_err(format!("internal: {message}"))
    })
}

#[derive(Clone)]
enum Input {
    Path(String),
    Bytes(Vec<u8>),
}

impl Input {
    fn extract(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        if let Ok(value) = value.cast::<PyString>() {
            return Ok(Self::Path(value.to_str()?.to_owned()));
        }
        if let Ok(value) = value.cast::<PyBytes>() {
            return Ok(Self::Bytes(value.as_bytes().to_vec()));
        }
        Err(PyTypeError::new_err(
            "step must be a str path or STEP bytes",
        ))
    }

    fn load(&self) -> Result<Part, String> {
        match self {
            Self::Path(path) => {
                step::read_step_file(std::path::Path::new(path)).map_err(|e| e.to_string())
            }
            Self::Bytes(bytes) => step::read_step(bytes).map_err(|e| e.to_string()),
        }
    }

    /// The STEP text: a path's bytes, gunzipped when the name ends in `.gz`.
    fn bytes(self) -> Result<Vec<u8>, String> {
        match self {
            Self::Path(path) => {
                let raw = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
                step::decode_step_file_bytes(std::path::Path::new(&path), raw)
                    .map_err(|e| format!("{path}: {e}"))
            }
            Self::Bytes(bytes) => Ok(bytes),
        }
    }

    fn recognise(self) -> Result<String, String> {
        match self {
            Self::Path(path) => file_api::recognise_file(&path),
            Self::Bytes(bytes) => file_api::recognise_bytes(bytes),
        }
    }

    fn fingerprints(self) -> Result<quiddity::Fingerprints, String> {
        let output = match self {
            Self::Path(path) => serve::recognise_file(&path),
            Self::Bytes(bytes) => serve::recognise_bytes("<bytes>", bytes),
        }
        .map_err(|e| e.to_string())?;
        Ok(output.0.recognition.fingerprints)
    }
}

#[pyfunction]
fn recognise(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || input.recognise())?.map_err(PyValueError::new_err)
}

/// Return complete recognition and evidence as JSON from shared analysis.
///
/// Evidence uses part coordinates whenever a frame is inferred, and file coordinates
/// otherwise. This choice is automatic; there is no independent ``framed`` argument.
/// Use ``evidence(step, framed=False)`` for fixed file coordinates, or the existing
/// ``evidence`` API with its explicit ``framed`` option for caller-selected framing.
/// Recognition document parameters remain in file coordinates. An evidence refusal
/// preserves recognition and is reported in ``evidence_error`` with ``evidence: null``.
#[pyfunction]
fn recognise_with_evidence(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || match input {
        Input::Path(path) => file_api::recognise_with_evidence_file(&path),
        Input::Bytes(bytes) => file_api::recognise_with_evidence_bytes(bytes),
    })?
    .map_err(PyValueError::new_err)
}

#[pyfunction]
#[pyo3(signature = (step, framed=false))]
fn evidence(py: Python<'_>, step: &Bound<'_, PyAny>, framed: bool) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || file_api::evidence(&input.load()?, framed))?.map_err(PyValueError::new_err)
}

#[pyfunction]
fn anchor(py: Python<'_>, step: &Bound<'_, PyAny>, face: usize) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || file_api::anchor(&input.load()?, face))?.map_err(PyValueError::new_err)
}

#[pyfunction]
fn fingerprints(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || file_api::json(&input.fingerprints()?))?.map_err(PyValueError::new_err)
}

#[pyfunction]
fn correspond(
    py: Python<'_>,
    old_step: &Bound<'_, PyAny>,
    new_step: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let old = Input::extract(old_step)?;
    let new = Input::extract(new_step)?;
    detached(py, || {
        let result = serve::correspond(&old.fingerprints()?, &new.fingerprints()?)
            .map_err(|e| e.to_string())?;
        file_api::json(&result)
    })?
    .map_err(PyValueError::new_err)
}

/// `quiddity parts`, with the CLI's trailing newline.
#[pyfunction]
fn parts(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || {
        let bytes = input.bytes()?;
        file_api::parts(&bytes, &bytes).map(|json| format!("{json}\n"))
    })?
    .map_err(PyValueError::new_err)
}

#[pyfunction]
fn step_entities(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    detached(py, || file_api::step_entities(input.bytes()?))?.map_err(PyValueError::new_err)
}

/// JSON conversion belongs to the small Python facade; scanning/reading stays off the GIL.
#[pyfunction]
#[pyo3(signature = (path, out=None))]
fn _manifest(py: Python<'_>, path: String, out: Option<String>) -> PyResult<String> {
    detached(py, || {
        let manifest = file_api::manifest(&path, &Default::default())?;
        let json = file_api::json(&manifest)?;
        if let Some(out) = out {
            std::fs::write(&out, &json).map_err(|e| format!("{out}: {e}"))?;
        }
        Ok::<_, String>(json)
    })?
    .map_err(PyValueError::new_err)
}

/// One immutable parsed manifest, reused across a lazy Python iteration.
#[pyclass(frozen)]
struct ManifestHandle {
    manifest: step::manifest::Manifest,
}

#[pymethods]
impl ManifestHandle {
    #[new]
    fn new(py: Python<'_>, manifest_json: String) -> PyResult<Self> {
        detached(py, || {
            serde_json::from_str(&manifest_json)
                .map(|manifest| Self { manifest })
                .map_err(|e| e.to_string())
        })?
        .map_err(PyValueError::new_err)
    }

    #[staticmethod]
    #[pyo3(signature = (path, parts=None))]
    fn from_path(py: Python<'_>, path: String, parts: Option<Vec<String>>) -> PyResult<Self> {
        detached(py, || -> Result<Self, String> {
            let input =
                std::io::BufReader::new(std::fs::File::open(&path).map_err(|e| e.to_string())?);
            let manifest = if let Some(parts) = parts {
                step::manifest::Manifest::read_selected(input, &parts, |_| Ok(()))
                    .map_err(|e| e.to_string())?
            } else {
                serde_json::from_reader(input).map_err(|e| e.to_string())?
            };
            Ok(Self { manifest })
        })?
        .map_err(PyValueError::new_err)
    }

    #[pyo3(signature = (parts=None))]
    fn product_ids(&self, py: Python<'_>, parts: Option<Vec<String>>) -> PyResult<Vec<String>> {
        detached(py, || {
            file_api::product_ids(&self.manifest, parts.as_deref())
        })?
        .map_err(PyValueError::new_err)
    }

    fn recognise(&self, py: Python<'_>, path: String, product_id: String) -> PyResult<String> {
        detached(py, || {
            file_api::recognise_product(&path, &self.manifest, &product_id)
        })?
        .map_err(PyValueError::new_err)
    }
}

#[pyfunction]
fn capabilities(py: Python<'_>) -> PyResult<String> {
    detached(py, || format!("{}\n", file_api::capabilities()))
}

#[pyfunction]
fn cli(py: Python<'_>, argv: Vec<String>) -> PyResult<u8> {
    detached(py, || quiddity::cli::main(&argv))
}

// Compiled only into dedicated test builds, never into release wheels.
#[cfg(feature = "test-panic")]
#[pyfunction]
fn _test_panic(py: Python<'_>, kind: u8) -> PyResult<()> {
    detached(py, || match kind {
        0 => panic!("forced internal panic"),
        1 => std::panic::panic_any(String::from("forced owned panic")),
        _ => std::panic::panic_any(42_u8),
    })
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("QuiddityError", m.py().get_type::<QuiddityError>())?;
    #[cfg(feature = "test-panic")]
    m.add_function(wrap_pyfunction!(_test_panic, m)?)?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(recognise, m)?)?;
    m.add_function(wrap_pyfunction!(_manifest, m)?)?;
    m.add_class::<ManifestHandle>()?;
    m.add_function(wrap_pyfunction!(recognise_with_evidence, m)?)?;
    m.add_function(wrap_pyfunction!(evidence, m)?)?;
    m.add_function(wrap_pyfunction!(anchor, m)?)?;
    m.add_function(wrap_pyfunction!(fingerprints, m)?)?;
    m.add_function(wrap_pyfunction!(correspond, m)?)?;
    m.add_function(wrap_pyfunction!(parts, m)?)?;
    m.add_function(wrap_pyfunction!(step_entities, m)?)?;
    m.add_function(wrap_pyfunction!(capabilities, m)?)?;
    m.add_function(wrap_pyfunction!(cli, m)?)?;
    pmi::register(m)?;
    Ok(())
}
