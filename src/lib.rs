use std::{cell::RefCell, rc::Rc};

struct RustFlight {
    num_calls: Rc<RefCell<u8>>,
}

impl RustFlight {
    fn new() -> Self {
        let instance = Self {
            num_calls: Rc::new(RefCell::new(0)),
        };
        return instance;
    }

    fn increment_calls(&self) {
        let mut counter = self.num_calls.borrow_mut();
        *counter += 1;
    }

    fn get_calls(&self) -> u8 {
        *self.num_calls.borrow()
    }

    fn wrap<F, T>(&self, func: F) -> impl Fn(u8) -> T
    where
        F: Fn(u8) -> T,
        T: std::fmt::Display,
    {
        let counter = Rc::clone(&self.num_calls);
        let closure = move |x| {
            println!("Calling with argument: {}", x);
            let result = func(x);
            *counter.borrow_mut() += 1;
            println!("Result: {}", result);
            return result;
        };
        closure
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
