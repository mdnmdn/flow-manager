pub mod auth;
pub mod cli;
pub mod commands;
pub mod core;
pub mod providers;

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::cli::is_debug() {
            eprintln!("[debug] {}", format!($($arg)*));
        }
    };
}
