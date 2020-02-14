use crate::svc::output_debug_string;

use core::fmt;

#[derive(Default)]
pub struct SvcDebugLog;

impl fmt::Write for SvcDebugLog {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        output_debug_string(s);
        Ok(())
    }
}
