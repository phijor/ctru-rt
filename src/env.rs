extern "C" {
    static __apt_appid: u32;
    static __heap_size: u32;
    static __service_ptr: *const u8;
    static __system_arglist: *const u8;
    static __system_runflags: u32;
}

pub fn is_homebrew() -> bool {
    unsafe { !__service_ptr.is_null() }
}

pub fn app_id() -> u32 {
    unsafe { __apt_appid }
}

pub fn system_runflags() -> u32 {
    unsafe { __system_runflags }
}

pub struct SystemArgList {
    length: usize,
    arguments: *const u8,
}

impl core::iter::Iterator for SystemArgList {
    type Item = &'static [u8];

    fn next(&mut self) -> Option<Self::Item> {
        match self.length {
            0 => None,
            _ => {
                let slice = unsafe {
                    let mut i = 0;
                    while self.arguments.offset(i).read() != b'\0' {
                        i += 1
                    }

                    self.arguments = self.arguments.offset(i + 1);
                    self.length -= 1;

                    core::slice::from_raw_parts(self.arguments, i as usize)
                };

                Some(slice)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.length, Some(self.length))
    }
}

impl core::iter::ExactSizeIterator for SystemArgList {}

pub fn system_arglist() -> SystemArgList {
    let length_ptr = unsafe { __system_arglist } as *const u32;
    let (length, arguments) = unsafe { (*length_ptr as usize, length_ptr.offset(1) as *const u8) };

    SystemArgList { length, arguments }
}

pub fn heap_size() -> usize {
    unsafe { __heap_size as usize }
}
