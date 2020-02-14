use crate::result::ResultCode;

pub fn output_debug_string(message: &str) {
    extern "C" {
        fn svcOutputDebugString(message: *const u8, length: u32);
    }

    unsafe { svcOutputDebugString(message.as_ptr(), message.len() as u32) }
}

pub fn exit_process() -> ! {
    extern "C" {
        fn svcExitProcess();
    }

    unsafe { svcExitProcess() }

    loop {}
}

pub mod mem {
    use crate::os::MemoryRegion;

    #[repr(u32)]
    pub enum MemoryOperationTarget {
        Heap = 0x0_0000,
        Linear = 0x1_0000,
    }

    #[repr(u32)]
    pub enum MemoryOperationRegion {
        App = (MemoryRegion::Application as u32) << 16,
        System = (MemoryRegion::System as u32) << 16,
        Base = (MemoryRegion::Base as u32) << 16,
    }

    #[repr(u32)]
    pub enum MemoryOperationAction {
        Free = 1,
        Reserve = 2,
        Allocate = 3,
        Map = 4,
        Unmap = 5,
        ChangeProtection = 6,
    }

    pub struct MemoryOperation(pub(crate) u32);

    impl MemoryOperation {
        #[inline]
        pub const fn new(
            action: MemoryOperationAction,
            region: MemoryOperationRegion,
            target: MemoryOperationTarget,
        ) -> Self {
            Self((action as u32) | (region as u32) | (target as u32))
        }

        #[inline]
        pub const fn allocate() -> Self {
            Self(MemoryOperationAction::Allocate as u32)
        }
    }

    #[repr(u32)]
    pub enum MemoryPermission {
        None = 0,
        R = 1,
        W = 2,
        Rw = 3,
        X = 4,
        Rx = 5,
        Wx = 6,
        Rwx = 7,
        DontCare = 0x10000000,
    }
}

pub unsafe fn control_memory(
    addr0: usize,
    addr1: usize,
    size: usize,
    op: mem::MemoryOperation,
    permission: mem::MemoryPermission,
) -> Result<usize, ResultCode> {
    extern "C" {
        fn svcControlMemory(
            dest: *mut usize,
            addr0: usize,
            addr1: usize,
            size: usize,
            op: u32,
            permission: u32,
        ) -> ResultCode;
    }

    let mut dest: usize = 0;
    svcControlMemory(
        &mut dest as *mut usize,
        addr0,
        addr1,
        size,
        op.0,
        permission as u32,
    )?;

    Ok(dest)
}
