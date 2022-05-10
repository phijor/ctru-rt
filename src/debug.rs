// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::svc::{output_debug_bytes, output_debug_string};

use log::{Level, Log, Metadata, Record};

use alloc::fmt;

#[derive(Default)]
pub struct SvcDebugLog;

impl SvcDebugLog {}

#[allow(clippy::unit_arg)]
impl fmt::Write for SvcDebugLog {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        Ok(output_debug_string(s))
    }

    fn write_fmt(&mut self, args: fmt::Arguments<'_>) -> fmt::Result {
        if crate::heap::initialized() {
            Ok(output_debug_string(&alloc::fmt::format(args)))
        } else {
            write_string_fallback(args)
        }
    }
}

/// Format a string to a fixed size buffer, then write it to the debug log.
///
/// This is `#[inline(never)]` so the stack allocation for the fixed size buffer is not necessary
/// in the presence of a heap.
#[inline(never)]
fn write_string_fallback(args: fmt::Arguments) -> fmt::Result {
    output_debug_string("[INTERNAL] Falling back to heap-less debug write");

    let mut buffer = FixedSizeBufferWriter::<512>::new();
    fmt::write(&mut buffer, args)?;

    output_debug_bytes(buffer.occupied());
    Ok(())
}

#[derive(Debug)]
#[doc(hidden)]
pub struct FixedSizeBufferWriter<const N: usize> {
    buffer: [u8; N],
    pos: usize,
}

impl<const N: usize> FixedSizeBufferWriter<N> {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            buffer: [0; N],
            pos: 0,
        }
    }

    fn remaining(&mut self) -> &mut [u8] {
        &mut self.buffer[self.pos..]
    }

    pub fn occupied(&self) -> &[u8] {
        &self.buffer[..self.pos]
    }
}

impl<const N: usize> fmt::Write for FixedSizeBufferWriter<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let remaining = self.remaining();

        let printed = &bytes[..bytes.len().min(remaining.len())];

        remaining[..printed.len()].copy_from_slice(printed);

        self.pos += printed.len();
        self.pos = self.pos.min(N);

        Ok(())
    }
}

#[doc(hidden)]
#[macro_export]
macro_rules! early_debug {
    ($fmt: expr, $($args: expr,)*) => {
        #[cfg(debug_assertions)]
        {
            let write = || {
                use alloc::fmt::Write;
                use $crate::{debug::FixedSizeBufferWriter, svc::output_debug_bytes};

                let mut buffer = FixedSizeBufferWriter::<256>::new();

                let _ = write!(&mut buffer, $fmt, $($args),*);

                output_debug_bytes(buffer.occupied())
            };

            write()
        }
    };

    ($fmt: expr, $($args: expr),*) => {
        early_debug!($fmt, $($args,)*)
    };

    ($fmt: expr) => {
        $crate::svc::output_debug_string($fmt);
    }
}

static LOGGER: SvcDebugLog = SvcDebugLog;

impl Log for SvcDebugLog {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let level = record.level();
            let color = match level {
                Level::Trace => "0",
                Level::Debug => "37",
                Level::Info => "32",
                Level::Warn => "33",
                Level::Error => "31",
            };
            output_debug_string(&alloc::fmt::format(format_args!(
                "\x1b[0m[\x1b[{};1m{:<5}\x1b[0m] {} - {}",
                color,
                level,
                record.module_path_static().unwrap_or(""),
                record.args()
            )))
        }
    }

    fn flush(&self) {}
}

pub fn init_log() -> Result<(), log::SetLoggerError> {
    use log::LevelFilter::{self, *};

    #[cfg(debug_assertions)]
    const FILTER: LevelFilter = Debug;
    #[cfg(not(debug_assertions))]
    const FILTER: LevelFilter = Info;

    log::set_logger(&LOGGER).map(|()| log::set_max_level(FILTER))
}
