//! Logging: `goat_core::info!("...")`. Messages go to a sink set by the platform (stderr on
//! desktop, the 3DS console / debug output on the 3DS); the default sink prints to stderr.

use std::fmt::Arguments;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Trace => "trace",
            Level::Debug => "debug",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

pub type Sink = fn(Level, &Arguments<'_>);

static SINK: RwLock<Option<Sink>> = RwLock::new(None);

/// Replaces where log messages go (called by platforms at startup).
pub fn set_sink(sink: Sink) {
    if let Ok(mut s) = SINK.write() {
        *s = Some(sink);
    }
}

#[doc(hidden)]
pub fn write(level: Level, args: Arguments<'_>) {
    let sink = SINK.read().ok().and_then(|s| *s);
    match sink {
        Some(f) => f(level, &args),
        None => eprintln!("[{}] {}", level.label(), args),
    }
}

#[macro_export]
macro_rules! info {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Info, format_args!($($t)*)) };
}
#[macro_export]
macro_rules! warn {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Warn, format_args!($($t)*)) };
}
#[macro_export]
macro_rules! error {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Error, format_args!($($t)*)) };
}
#[macro_export]
macro_rules! debug {
    ($($t:tt)*) => { $crate::log::write($crate::log::Level::Debug, format_args!($($t)*)) };
}
