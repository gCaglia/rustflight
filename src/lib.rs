use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard};

struct RustFlight {
    num_calls: Arc<RwLock<u8>>,
}

impl RustFlight {
    fn new() -> Self {
        let instance = Self {
            num_calls: Arc::new(RwLock::new(0)),
        };
        return instance;
    }

    fn increment_calls(&self) {
        if let Ok(mut count) = self.num_calls.write() {
            *count += 1;
        } else {
            eprintln!("Failed to aquire write lock!")
        }
    }

    fn get_calls(&self) -> u8 {
        let calls: Result<RwLockReadGuard<u8>, PoisonError<_>> = self.num_calls.read();

        match calls {
            Ok(res) => *res,
            Err(err) => {
                eprintln!("Error getting calls: {:?}", err);
                0
            }
        }
    }
}

trait RustMethods {
    type WrappedFn: Fn(u8) -> u8;

    fn wrap<F>(&self, func: F) -> Self::WrappedFn
    where
        F: Fn(u8) -> u8 + 'static;
}

impl RustMethods for RustFlight {
    type WrappedFn = Box<dyn Fn(u8) -> u8>;

    fn wrap<F>(&self, func: F) -> Self::WrappedFn
    where
        F: Fn(u8) -> u8 + 'static,
    {
        let counter = Arc::clone(&self.num_calls);
        let closure = move |x| {
            println!("Calling with argument: {}", x);
            let result = func(x);
            if let Ok(mut count) = counter.write() {
                *count += 1;
            } else {
                eprintln!("Failed to aquire write lock!")
            }
            println!("Result: {}", result);
            return result;
        };
        Box::new(closure)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn test_instance() -> RustFlight {
        RustFlight::new()
    }

    #[test]
    fn test_increment_calls() {
        let instance = test_instance();
        instance.increment_calls();

        assert_eq!(instance.get_calls(), 1)
    }

    #[test]
    fn test_wrapper() {
        fn double_it(number: u8) -> u8 {
            number * 2
        }

        let test_instance = test_instance();
        assert_eq!(test_instance.get_calls(), 0);
        let wrapped = test_instance.wrap(double_it);
        let actual = wrapped(5);
        let actual_count = test_instance.get_calls();

        assert_eq!(actual, 10);
        assert_eq!(actual_count, 1);
    }
}
