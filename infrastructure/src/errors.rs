#[macro_export]
macro_rules! fail {
     ($($arg:tt)+) => {
         log::error!($($arg)+);
         panic!($($arg)+);
     };
}
