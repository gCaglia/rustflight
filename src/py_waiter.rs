use pyo3::prelude::*;
use pyo3::types::{PyDict, PyTuple};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

struct PyCacheEntry {
    value: Option<Py<PyAny>>,
    ready: bool,
}

impl PyCacheEntry {
    fn pending() -> Self {
        Self {
            value: None,
            ready: false,
        }
    }

    fn ready(&mut self, new_value: Py<PyAny>) {
        self.value = Some(new_value);
        self.ready = true
    }
}

enum PyEntryState {
    Pending(Arc<(Mutex<PyCacheEntry>, Condvar)>),
}

#[pyclass]
pub struct PyCache {
    cache: Arc<Mutex<HashMap<String, PyEntryState>>>,
    timeout: u64,
}

#[pymethods]
impl PyCache {
    #[new]
    fn new(timeout: u64) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            timeout,
        }
    }

    fn py_call(
        &self,
        py: Python<'_>,
        py_func: Py<PyAny>,
        args: Py<PyAny>,
        kwargs: Py<PyAny>,
        key: String,
    ) -> Py<PyAny> {
        let mut cache = self.cache.lock().unwrap();

        let cached_value = cache.get(&key);

        if let Some(value_state) = cached_value {
            match value_state {
                PyEntryState::Pending(lock_var) => {
                    let (lock, cvar) = &**lock_var;
                    let entry = lock.lock().unwrap();
                    if entry.ready {
                        return entry
                            .value
                            .as_ref()
                            .expect("None after read!")
                            .clone_ref(py);
                    }
                    drop(entry);

                    let _ = py.allow_threads(move || {
                        let wait_guard = lock.lock().unwrap();
                        let _ = cvar
                            .wait_timeout(wait_guard, Duration::from_millis(self.timeout))
                            .unwrap();
                    });

                    let entry = lock.lock().unwrap();
                    if entry.ready {
                        return entry
                            .value
                            .as_ref()
                            .expect("None after read!")
                            .clone_ref(py);
                    }
                }
            }
        }
        // Insert waiting state and drop call
        let placeholder = PyCacheEntry::pending();
        let notification = Condvar::new();
        let pending_entry = Arc::new((Mutex::new(placeholder), notification));
        cache.insert(key.clone(), PyEntryState::Pending(pending_entry.clone()));
        drop(cache);

        // Do calculation
        let args_tuple: &Bound<'_, PyTuple> =
            args.downcast_bound(py).expect("Unable to cast to PyTuple!");
        let kwargs_dict: &Bound<'_, PyDict>;
        kwargs_dict = kwargs
            .downcast_bound(py)
            .expect("Unable to cast to PyDict!");
        let result = py_func
            .call(py, args_tuple, Some(kwargs_dict))
            .expect("PyCall failed");

        // Notify waiting values and update state
        let (lock, cvar) = &*pending_entry;
        let mut entry = lock.lock().expect("Unable to get cache entry for update");
        entry.ready(result.clone_ref(py));
        cvar.notify_all();
        result
    }

    fn drop(&self, key: String) {
        let mut cache = self.cache.lock().expect("Unable to lock cache!");
        cache.remove(&key);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pyo3::{
        ffi::c_str,
        types::{IntoPyDict, PyTuple},
    };

    #[test]
    fn test_pycall() {
        let pycache = PyCache::new(10000);
        let args: [i8; 2] = [1, 10];
        let kwargs: [(&'static str, i16); 1] = [("multiplier", 100)];
        let test_key: String = "test".to_string();

        Python::with_gil(|py| {
            let pyfunc: Py<PyAny> = PyModule::from_code(
                py,
                c_str!(
                    "from random import randint

def f(lower, upper, multiplier):
                        return randint(lower, upper)*multiplier"
                ),
                c_str!(""),
                c_str!(""),
            )
            .unwrap()
            .getattr("f")
            .unwrap()
            .into();
            let py_args: Bound<'_, PyTuple> = PyTuple::new(py, &args).unwrap();
            let py_kwargs: Bound<'_, PyDict> = kwargs.into_py_dict(py).unwrap();

            let _ = pycache.py_call(
                py,
                pyfunc.clone_ref(py),
                py_args.clone().into(),
                py_kwargs.into(),
                test_key.clone(),
            );

            // Assert state of cache
            let cache = pycache.cache.lock().unwrap();
            let cached_entry = cache.get(&test_key).unwrap();
            let expected: i32;
            match cached_entry {
                PyEntryState::Pending(val) => {
                    let (lock, _) = &**val;
                    let entry = lock.lock().unwrap();
                    assert_eq!(entry.ready, true);
                    expected = entry.value.as_ref().unwrap().extract::<i32>(py).unwrap();
                }
            }
            drop(cache);
            let actual = pycache
                .py_call(
                    py,
                    pyfunc.clone_ref(py),
                    py_args.clone().into(),
                    PyDict::new(py).into(),
                    test_key,
                )
                .extract::<i32>(py)
                .unwrap();

            assert_eq!(actual, expected);
        })
    }
}
