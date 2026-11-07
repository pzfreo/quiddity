//! CPython abi3 boundary. Geometry work runs with the GIL released.
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyString};
use quiddity::{Part, file_api, kernel::step, serve};

mod pmi;

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
    py.detach(|| input.recognise())
        .map_err(PyValueError::new_err)
}

#[pyfunction]
#[pyo3(signature = (step, framed=false))]
fn evidence(py: Python<'_>, step: &Bound<'_, PyAny>, framed: bool) -> PyResult<String> {
    let input = Input::extract(step)?;
    py.detach(|| file_api::evidence(&input.load()?, framed))
        .map_err(PyValueError::new_err)
}

#[pyfunction]
fn anchor(py: Python<'_>, step: &Bound<'_, PyAny>, face: usize) -> PyResult<String> {
    let input = Input::extract(step)?;
    py.detach(|| file_api::anchor(&input.load()?, face))
        .map_err(PyValueError::new_err)
}

#[pyfunction]
fn fingerprints(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    py.detach(|| file_api::json(&input.fingerprints()?))
        .map_err(PyValueError::new_err)
}

#[pyfunction]
fn correspond(
    py: Python<'_>,
    old_step: &Bound<'_, PyAny>,
    new_step: &Bound<'_, PyAny>,
) -> PyResult<String> {
    let old = Input::extract(old_step)?;
    let new = Input::extract(new_step)?;
    py.detach(|| {
        let result = serve::correspond(&old.fingerprints()?, &new.fingerprints()?)
            .map_err(|e| e.to_string())?;
        file_api::json(&result)
    })
    .map_err(PyValueError::new_err)
}

/// `quiddity parts`, with the CLI's trailing newline.
#[pyfunction]
fn parts(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    py.detach(|| {
        let bytes = input.bytes()?;
        file_api::parts(&bytes, &bytes).map(|json| format!("{json}\n"))
    })
    .map_err(PyValueError::new_err)
}

#[pyfunction]
fn step_entities(py: Python<'_>, step: &Bound<'_, PyAny>) -> PyResult<String> {
    let input = Input::extract(step)?;
    py.detach(|| file_api::step_entities(input.bytes()?))
        .map_err(PyValueError::new_err)
}

#[pyfunction]
fn capabilities() -> String {
    format!("{}\n", file_api::capabilities())
}

#[pyfunction]
fn cli(py: Python<'_>, argv: Vec<String>) -> PyResult<u8> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        py.detach(|| quiddity::cli::main(&argv))
    }))
    .map_err(|_| PyRuntimeError::new_err("Rust CLI panicked"))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add_function(wrap_pyfunction!(recognise, m)?)?;
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
