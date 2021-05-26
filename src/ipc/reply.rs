use super::{IpcHeader, TranslationDescriptor, COMMAND_BUFFER_LENGTH};
use crate::result::{CommonDescription, Level, Module, Summary};
use crate::{
    os::WeakHandle,
    result::{Result, ResultCode},
};

use log::{debug, trace};

use core::{
    marker::PhantomData,
    ops::{Index, Range},
};

pub(crate) struct ReplyBuffer<'a>(*const u32, *const u32, PhantomData<&'a u32>);

impl<'a> ReplyBuffer<'a> {
    pub(crate) const unsafe fn new(buf: *const u32) -> Self {
        Self(buf, buf, PhantomData)
    }

    pub(crate) const fn start(&self) -> *const u32 {
        self.0
    }

    pub(crate) fn end(&self) -> *const u32 {
        unsafe { self.0.offset(COMMAND_BUFFER_LENGTH as isize) }
    }

    pub(crate) fn range(&self) -> Range<*const u32> {
        Range {
            start: self.start(),
            end: self.end(),
        }
    }

    pub(crate) const fn read_ptr(&self) -> *const u32 {
        self.1
    }

    unsafe fn advance_read_ptr(&mut self, offset: usize) {
        self.1 = self.1.offset(offset as isize)
    }

    #[inline]
    pub(crate) fn read(&mut self) -> u32 {
        if self.range().contains(&self.read_ptr()) {
            unsafe {
                let value = self.read_ptr().read();
                self.advance_read_ptr(1);
                value
            }
        } else {
            panic!(
                "Detected attempt to read past the end of the result buffer: {:?} is past the end of {:?}", 
                self.read_ptr(), self.range()
            )
        }
    }

    #[inline]
    pub(crate) fn read_range(&mut self, len: usize) -> &'a [u32] {
        let slice_range = Range {
            start: self.read_ptr(),
            end: unsafe { self.read_ptr().offset(len as isize).offset(-1) },
        };

        if self.range().contains(&slice_range.start) && self.range().contains(&slice_range.end) {
            unsafe {
                let slice = core::slice::from_raw_parts(slice_range.start, len);
                self.advance_read_ptr(len);
                slice
            }
        } else {
            panic!(
                "Detected attempt to read past the end of the result buffer: {:?} is past the end of {:?}", 
                slice_range, self.range()
            )
        }
    }
}

#[derive(Debug)]
pub struct Reply<'a> {
    pub command_id: u16,
    pub values: ReplyValues<'a>,
    pub translate_values: ReplyTranslateValues<'a>,
}

impl<'a> Reply<'a> {
    #[inline(always)]
    pub(crate) fn parse_nofail(mut reply_buffer: ReplyBuffer<'a>) -> (Self, ResultCode) {
        let header = IpcHeader::from(reply_buffer.read());

        trace!("Parsed header: {:x?}", header);

        let (result_code, values) = match header.normal_param_words() {
            0 => {
                debug!("IPC reply contains no result code, assuming failure");
                (
                    ResultCode::new(
                        Level::Info,
                        Summary::InvalidResultValue,
                        Module::Application,
                        CommonDescription::NoData.to_value(),
                    ),
                    None,
                )
            }
            count => {
                let result_code = ResultCode::from(reply_buffer.read());

                trace!("Reply contains {} normal parameters", count);
                (
                    result_code,
                    Some(reply_buffer.read_range(count.wrapping_sub(1))),
                )
            }
        };

        let translate_values = match header.translate_param_words() {
            0 => None,
            1 => None,
            word_size => {
                trace!("Reply contains {} words of translate parameters", word_size);
                let (header, body) = reply_buffer.read_range(word_size).split_first().unwrap();
                let descriptor = TranslationDescriptor::from(*header);
                trace!("translate descriptor: {:08x?}", descriptor);
                let nhandles = descriptor.len() + 1;
                let handles = &body[0..nhandles];

                trace!("Handles: {:08x?}", handles);

                Some(unsafe {
                    core::slice::from_raw_parts(
                        handles.as_ptr() as *const WeakHandle,
                        handles.len(),
                    )
                })
            }
        };

        (
            Self {
                command_id: header.command_id(),
                values: ReplyValues { values },
                translate_values: ReplyTranslateValues {
                    values: translate_values,
                },
            },
            result_code,
        )
    }

    #[inline(always)]
    pub(crate) fn parse(reply_buffer: ReplyBuffer<'a>) -> Result<Self> {
        let (reply, result_code) = Self::parse_nofail(reply_buffer);

        result_code.and(reply)
    }
}

#[derive(Debug)]
pub struct ReplyValues<'a> {
    values: Option<&'a [u32]>,
}

impl<'a> Index<usize> for ReplyValues<'a> {
    type Output = u32;
    fn index(&self, index: usize) -> &Self::Output {
        match self.values {
            None => panic!("Cannot index into empty set of reply values"),
            Some(values) => &values[index],
        }
    }
}

#[derive(Debug)]
pub struct ReplyTranslateValues<'a> {
    values: Option<&'a [WeakHandle<'a>]>,
}

impl<'a> Index<usize> for ReplyTranslateValues<'a> {
    type Output = WeakHandle<'a>;
    fn index(&self, index: usize) -> &Self::Output {
        match self.values {
            None => panic!("Cannot index into empty set of reply translate values"),
            Some(values) => &values[index],
        }
    }
}
