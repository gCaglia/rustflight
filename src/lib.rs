use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::any::Any;
use std::hash::Hash;
use std::sync::Condvar;
use std::sync::Mutex;
use std::{
    collections::HashMap,
    error::Error,
    sync::{Arc, RwLock},
};

type Cache = Arc<RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>>;

#[pyclass]
struct RustFlight {
    call_cache: Cache,
}

#[pymethods]
impl RustFlight {
    #[new]
    fn new() -> Self {
        let call_cache: Cache = Arc::new(RwLock::new(HashMap::new()));
        RustFlight { call_cache }
    }

    fn py_call(
        &self,
        py: Python<'_>,
        func: Py<PyAny>,
        args: Py<PyAny>,
        kwargs: Py<PyAny>,
        key: String,
    ) -> PyResult<PyObject> {
        let call_cache = Arc::clone(&self.call_cache);

        if let Ok(reader) = call_cache.read() {
            if let Some(cached) = reader.get(&key) {
                if let Some(py_obj) = cached.downcast_ref::<Py<PyAny>>() {
                    return Ok(py_obj.clone_ref(py));
                } else {
                    eprintln!("Could not downcast cached result to PyAny!")
                }
            }
        } else {
            eprintln!("Failed to read cache!")
        }
        let args_tuple_result = args.downcast_bound(py)?;
        let kwargs_pydict: &Bound<'_, PyDict> = kwargs.downcast_bound(py)?;
        let result = func.call(py, args_tuple_result, Some(kwargs_pydict))?;
        let writer_lock = call_cache.write();

        if let Ok(mut writer) = writer_lock {
            writer.insert(key, Box::new(result.clone_ref(py)));
        } else {
            eprintln!("Failed to acquire write lock!");
        }

        return Ok(result);
    }
}

trait RustMethods {
    fn call<F, Args, T>(&self, func: F, args: Args, key: String) -> Result<T, Box<dyn Error>>
    where
        F: FnOnce(Args) -> T,
        T: Any + Send + Sync + Clone;
}

impl RustMethods for RustFlight {
    fn call<F, Args, T>(&self, func: F, args: Args, key: String) -> Result<T, Box<dyn Error>>
    where
        F: FnOnce(Args) -> T,
        T: Any + Send + Sync + Clone,
    {
        // Check if the key is in args and if yes retunr the assoicated value
        let call_cache: Cache = Arc::clone(&self.call_cache);

        if let Ok(reader) = call_cache.read() {
            if let Some(result) = reader.get(&key) {
                if let Some(out) = result.downcast_ref::<T>() {
                    return Ok(out.clone());
                } else {
                    return Err("Cached value has unexpected type!".into());
                }
            }
        } else {
            return Err("Failed to get read lock!".into());
        }

        // Otherwise, call the function and return the result
        let result = func(args);
        let call_cache_writer = call_cache.write();
        if let Ok(mut writer) = call_cache_writer {
            writer.insert(key, Box::new(result.clone()));
        } else {
            eprintln!("Failed to aquire write lock!")
        }
        Ok(result)
    }
}

// For Training Purposes
struct CacheEntry<V> {
    value: Option<V>,
    ready: bool,
}

impl<V> CacheEntry<V> {
    fn pending() -> Self {
        Self {
            value: None,
            ready: false,
        }
    }
}

enum EntryState<V> {
    Ready(V),
    Pending(Arc<(Mutex<CacheEntry<V>>, Condvar)>),
}

struct SimpleWaiter<K, V> {
    cache: Arc<Mutex<HashMap<K, EntryState<V>>>>,
}

impl<K, V> SimpleWaiter<K, V>
where
    K: Eq + Clone + Hash + Send + 'static,
    V: Clone + Send + 'static,
{
    fn new() -> Self {
        return Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        };
    }
    fn call_with_cache<F, Args>(&self, f: F, args: Args, key: K) -> V
    where
        F: FnOnce(Args) -> V,
    {
        let mut cache = self.cache.lock().unwrap();

        // If entry for key is found, return it
        match cache.get(&key) {
            Some(EntryState::Ready(val)) => val.clone(),
            Some(EntryState::Pending(pending)) => {
                let (lock, cvar) = &**pending;
                let mut done = lock.lock().unwrap();

                while !done.ready {
                    done = cvar.wait(done).unwrap();
                }

                let cache = self.cache.lock().unwrap();
                if let Some(EntryState::Ready(val)) = cache.get(&key) {
                    return val.clone();
                } else {
                    panic!("Value not found!")
                }
            }
            None => {
                let state = Mutex::new(CacheEntry::<V>::pending());
                let notification = Arc::new((state, Condvar::new()));
                cache.insert(key.clone(), EntryState::Pending(notification.clone()));
                drop(cache);
                let result: V = f(args);
                let mut cache = self.cache.lock().unwrap();
                cache.insert(key, EntryState::Ready(result.clone()));
                let (_, cvar) = &*notification;
                cvar.notify_all();
                result
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use pyo3::types::PyTuple;
    use pyo3::{ffi::c_str, types::IntoPyDict};
    use rand::{rng, Rng};

    fn test_instance() -> RustFlight {
        RustFlight::new()
    }

    struct TestObject {
        num_calls: usize,
    }

    impl TestObject {
        fn new() -> Self {
            TestObject { num_calls: 0 }
        }

        fn sum_up(&mut self, a: i8, b: i8) -> i8 {
            self.num_calls += 1;
            a + b
        }

        fn get_random_value(&self) -> u8 {
            return rng().random();
        }
    }

    #[test]
    fn test_call() {
        let instance = test_instance();
        let mut test_callable = TestObject::new();
        let result: i8 = instance
            .call(
                |(a, b)| test_callable.sum_up(a, b),
                (1, 2),
                "test".to_string(),
            )
            .unwrap();

        assert_eq!(result, 3);
        assert_eq!(test_callable.num_calls, 1);
    }

    #[test]
    fn test_call_keys_are_cached() {
        let instance = test_instance();
        let test_object = TestObject::new();

        let result1: u8 = instance
            .call(|()| test_object.get_random_value(), (), "key1".to_string())
            .unwrap();
        let result2: u8 = instance
            .call(|()| test_object.get_random_value(), (), "key1".to_string())
            .unwrap();
        let result3: u8 = instance
            .call(|()| test_object.get_random_value(), (), "key2".to_string())
            .unwrap();

        assert_eq!(result1, result2);
        assert_ne!(result2, result3);
    }

    #[test]
    fn test_py_call() {
        let instance = test_instance();
        let args = [1, 2, 3];

        Python::with_gil(|py| {
            let pyfunc: Py<PyAny> = PyModule::from_code(
                py,
                c_str!(
                    "def f(*args, **kwargs):
                        return sum(args) / kwargs.get('divide', 1)"
                ),
                c_str!(""),
                c_str!(""),
            )
            .unwrap()
            .getattr("f")
            .unwrap()
            .into();
            let py_args: Bound<'_, PyTuple> = PyTuple::new(py, &args).unwrap();
            let py_kwargs: Bound<'_, PyDict> = [("divide", 3)].into_py_dict(py).unwrap();
            let actual: PyResult<Py<PyAny>> = instance.py_call(
                py,
                pyfunc,
                py_args.into(),
                py_kwargs.into(),
                "key".to_string(),
            );
            let rust_actual = actual.unwrap().extract::<f32>(py).unwrap();
            assert_eq!(rust_actual, 2 as f32);
        });
    }

    #[test]
    fn test_simple_waiter() {
        let waiter = SimpleWaiter::<String, u8>::new();
        let rng = TestObject::new();
        let test_key = "abc".to_string();

        let expected:u8 = waiter.call_with_cache(|()| rng.get_random_value(), (), test_key.clone());
        let actual: u8 = waiter.call_with_cache(|()| rng.get_random_value(), (), test_key.clone());

        assert_eq!(actual, expected)
    }
}
