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
    fn new(value: Option<Py<PyAny>>, ready: bool) -> Self {
        Self { value, ready }
    }

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
    Ready(PyCacheEntry),
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
                PyEntryState::Ready(entry) => {
                    let return_value = entry
                        .value
                        .as_ref()
                        .expect("Ready value was None.")
                        .clone_ref(py);
                    return return_value;
                }
                PyEntryState::Pending(lock_var) => {
                    let (lock, cvar) = &**lock_var;

                    loop {
                        let entry = lock.lock().unwrap();

                        if entry.ready {
                            let return_value = entry
                                .value
                                .as_ref()
                                .expect("None after ready!")
                                .clone_ref(py);
                            return return_value;
                        }

                        let (guard, _) = cvar
                            .wait_timeout(entry, Duration::from_millis(self.timeout))
                            .unwrap();

                        if guard.ready {
                            let return_value = guard
                                .value
                                .as_ref()
                                .expect("Guard is none after ready!")
                                .clone_ref(py);
                            return return_value;
                        }
                        break;
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

        //Return
        cache = self
            .cache
            .lock()
            .expect("Unable to get lock for final update");
        cache.insert(
            key.clone(),
            PyEntryState::Ready(PyCacheEntry::new(Some(result.clone_ref(py)), true)),
        );
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
                PyEntryState::Ready(val) => {
                    assert_eq!(val.ready, true);
                    expected = val.value.as_ref().unwrap().extract::<i32>(py).unwrap();
                    assert!(expected > 10);
                }
                PyEntryState::Pending(_) => {
                    panic!("State was Pending after execution!")
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
