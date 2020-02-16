use crate::svc::output_debug_string;

#[derive(Default)]
pub struct SvcDebugLog;

impl core::fmt::Write for SvcDebugLog {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        Ok(output_debug_string(s))
    }

    #[cfg(feature = "heap")]
    fn write_fmt(&mut self, args: core::fmt::Arguments) -> core::fmt::Result {
        if crate::heap::initialized() {
            Ok(output_debug_string(&alloc::fmt::format(args)))
        } else {
            core::fmt::write(self, args)
        }
    }
}
