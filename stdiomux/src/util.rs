macro_rules! panic_or_warn  {
    ($($arg:tt)*) => {
        if cfg!(debug_assertions) {
            panic!($($arg)*);
        } else {
            tracing::warn!($($arg)*);
        }
    }
}

pub(crate) use panic_or_warn;
