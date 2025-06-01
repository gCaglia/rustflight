mod py_waiter;

use py_waiter::PyCache;
use pyo3::prelude::*;

#[pymodule]
fn rustflight(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyCache>()?;
    Ok(())
}
