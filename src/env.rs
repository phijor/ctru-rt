pub fn app_id() -> u32 {
    extern "C" {
        static __apt_appid: u32;
    }

    unsafe { __apt_appid }
}

pub fn system_runflags() -> u32 {
    extern "C" {
        static __system_runflags: u32;
    }

    unsafe { __system_runflags }
}

pub struct SystemArglist {
    length: usize,
    arguments: *const u8,
}

pub struct SystemArglistIter {
    remaining: usize,
    arguments: *const u8,
}

impl core::iter::IntoIterator for SystemArglist {
    type IntoIter = SystemArglistIter;
    type Item = <SystemArglistIter as core::iter::Iterator>::Item;

    fn into_iter(self) -> Self::IntoIter {
        SystemArglistIter {
            remaining: self.length,
            arguments: self.arguments,
        }
    }
}

impl core::iter::Iterator for SystemArglistIter {
    type Item = &'static [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self.remaining {
            0 => None,
            _ => {
                let arg_len: usize = unsafe {
                    let mut i = 0;
                    while self.arguments.offset(i).read() != b'\0' {
                        i += 1
                    }

                    self.remaining -= 1;
                    self.arguments = self.arguments.offset(i);

                    i as usize
                };

                let slice = unsafe { core::slice::from_raw_parts(self.arguments, arg_len) };

                Some(slice)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl core::iter::ExactSizeIterator for SystemArglistIter {}

pub fn system_arglist() -> SystemArglist {
    extern "C" {
        static __system_arglist: *const u8;
    }

    let length_ptr = unsafe { __system_arglist } as *const u32;
    let (length, arguments) = unsafe { (*length_ptr as usize, length_ptr.offset(1) as *const u8) };

    SystemArglist { length, arguments }
}

pub fn heap_size() -> usize {
    extern "C" {
        static __heap_size: u32;
    }

    unsafe { __heap_size as usize }
}
