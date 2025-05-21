use pyo3::prelude::*;
use std::any::Any;
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
        let result = func.call1(py, args_tuple_result)?;
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

#[cfg(test)]
mod test {
    use super::*;
    use pyo3::ffi::c_str;
    use pyo3::types::PyTuple;
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
                    "def f(*args):
                            return sum(args)"
                ),
                c_str!(""),
                c_str!(""),
            )
            .unwrap()
            .getattr("f")
            .unwrap()
            .into();
            let py_args: Bound<'_, PyTuple> = PyTuple::new(py, &args).unwrap();
            let actual: PyResult<Py<PyAny>> =
                instance.py_call(py, pyfunc, py_args.into(), "key".to_string());
            let rust_actual = actual.unwrap().extract::<i8>(py).unwrap();
            assert_eq!(rust_actual, 6);
        });
    }
}
