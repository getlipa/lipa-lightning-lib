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
    ($($arg:tt)*) => {{
        let res = std::fmt::format(format_args!($($arg)*));
        return Err(invalid_input(res))
    }}
}

#[macro_export]
macro_rules! runtime_error {
    ($code:expr, $($arg:tt)*) => {{
        let res = std::fmt::format(format_args!($($arg)*));
        return Err(runtime_error($code, res))
    }}
}

#[macro_export]
macro_rules! permanent_failure {
    ($($arg:tt)*) => {{
        let res = std::fmt::format(format_args!($($arg)*));
        return Err(permanent_failure(res))
    }}
}
