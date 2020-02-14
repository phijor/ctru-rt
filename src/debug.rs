use crate::svc::output_debug_string;

#[cfg(not(feature = "heap"))]
use core::fmt;

#[cfg(feature = "heap")]
use alloc::fmt;

#[derive(Default)]
pub struct SvcDebugLog;

impl fmt::Write for SvcDebugLog {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Ok(output_debug_string(s))
    }

    #[cfg(feature = "heap")]
    fn write_fmt(&mut self, args: fmt::Arguments) -> fmt::Result {
        Ok(output_debug_string(&fmt::format(args)))
    }
}
