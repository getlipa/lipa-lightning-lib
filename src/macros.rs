#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err($err);
        }
    };
}

#[macro_export]
macro_rules! invalid_input {
    ($err:expr) => {
        return Err(invalid_input($err))
    };
}

#[macro_export]
macro_rules! runtime_error {
    ($code:expr, $err:expr) => {
        return Err(runtime_error($code, $err))
    };
}

#[macro_export]
macro_rules! permanent_failure {
    ($err:expr) => {
        return Err(permanent_failure($err))
    };
}
