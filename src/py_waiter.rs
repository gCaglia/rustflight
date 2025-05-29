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

struct PyCache {
    cache: Arc<Mutex<HashMap<String, PyEntryState>>>,
    timeout: u64,
}

impl PyCache {
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
                    let mut entry = lock.lock().unwrap();

                    while !entry.ready {
                        let (guard, _) = cvar
                            .wait_timeout(entry, Duration::from_secs(self.timeout))
                            .unwrap();
                        entry = guard;

                        if !entry.ready {
                            panic!("Timeout while waiting for result")
                        }
                    }

                    let return_value = entry
                        .value
                        .as_ref()
                        .expect("Ready value was None after Pending state.")
                        .clone_ref(py);
                    return return_value;
                }
            }
        } else {
            // Insert waiting state and drop call
            let placeholder = PyCacheEntry::pending();
            let notification = Condvar::new();
            let pending_entry = Arc::new((Mutex::new(placeholder), notification));
            cache.insert(key.clone(), PyEntryState::Pending(pending_entry.clone()));
            drop(cache);

            // Do calculation
            let args_tuple: &Bound<'_, PyTuple> =
                args.downcast_bound(py).expect("Unable to cast to PyTuple!");
            let kwargs_dict: &Bound<'_, PyDict> = kwargs
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
    }
}
