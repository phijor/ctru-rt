use crate::svc::output_debug_string;

use log::{Level, Log, Metadata, Record};

#[derive(Default)]
pub struct SvcDebugLog;

impl SvcDebugLog {}

impl core::fmt::Write for SvcDebugLog {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        Ok(output_debug_string(s))
    }

    #[cfg(feature = "heap")]
    fn write_fmt(mut self: &mut Self, args: core::fmt::Arguments<'_>) -> core::fmt::Result {
        if crate::heap::initialized() {
            Ok(output_debug_string(&alloc::fmt::format(args)))
        } else {
            core::fmt::write(self, args)
        }
    }
}

#[macro_export]
macro_rules! debug {
    ($fmt: expr, $($args: tt,)*) => {
        #[cfg(debug_assertions)]
        {
            let write = || {
                use core::fmt::Write;

                let _ = write!($crate::debug::SvcDebugLog, $fmt, $($args),*);
            };

            write()
        }
    };

    ($fmt: expr, $($args: tt),*) => {
        debug!($fmt, $($args,)*)
    }
}

static LOGGER: SvcDebugLog = SvcDebugLog;

impl Log for SvcDebugLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        #[cfg(feature = "heap")]
        if self.enabled(record.metadata()) {
            output_debug_string(&alloc::fmt::format(format_args!(
                "[{}] {}({}:{}) - {}",
                record.level(),
                record.module_path_static().unwrap_or(""),
                record.file().unwrap_or(""),
                record.line().unwrap_or(0),
                record.args()
            )))
        }
    }

    fn flush(&self) {}
}

pub fn init_log() -> Result<(), log::SetLoggerError> {
    log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Debug))
}
