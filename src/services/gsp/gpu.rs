use crate::ipc::{IpcRequest, Reply, TranslateParameterSet};
use crate::os::mem::MemoryPermission;
use crate::os::{
    sharedmem::{MappedBlock, SharedMemoryMapper},
    BorrowHandle, Handle, WeakHandle,
};
use crate::result::{ErrorCode, Result};
use crate::services::srv::Srv;
use crate::svc::Timeout;
use crate::sync::{Event, ResetType};

use log::{debug, trace, warn};

use core::sync::atomic::{AtomicU32, Ordering};

use ctru_rt_macros::EnumCast;

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCast)]
#[enum_cast(value_type = "usize")]
pub enum Screen {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ScreenDimensions {
    pub width: u16,
    pub height: u16,
}

impl Screen {
    pub(crate) const fn dimensions(&self) -> ScreenDimensions {
        match self {
            Self::Top => ScreenDimensions {
                width: 240,
                height: 400,
            },
            Self::Bottom => ScreenDimensions {
                width: 240,
                height: 320,
            },
        }
    }

    pub(crate) const fn dimensions_register(&self) -> u32 {
        let dim = self.dimensions();
        (dim.height as u32) << 16 | (dim.width as u32)
    }
}

#[repr(packed)]
struct InterruptHeader {
    current_index: u8,
    events_total: u8,
    error: u8,
    _unused: u8,
}

impl From<u32> for InterruptHeader {
    fn from(header: u32) -> Self {
        let bytes = header.to_le_bytes();
        Self {
            current_index: bytes[0],
            events_total: bytes[1],
            error: bytes[2],
            _unused: bytes[3],
        }
    }
}

impl Into<u32> for InterruptHeader {
    fn into(self) -> u32 {
        u32::from_le_bytes([
            self.current_index,
            self.events_total,
            self.error,
            self._unused,
        ])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumCast)]
#[enum_cast(value_type = "u8")]
pub enum InterruptEvent {
    PSC0,
    PSC1,
    VBlank0,
    VBlank1,
    PPF,
    P3D,
    DMA,
}

#[derive(Debug)]
struct Sharedmem {
    gpu_events: Event,
    gsp_module_thread_index: u8,
    shared_memory: MappedBlock,
}

pub struct InterruptEventSet(u32);

impl InterruptEventSet {
    const fn empty() -> Self {
        Self(0)
    }

    fn add(&mut self, event: InterruptEvent) {
        self.0 |= 1 << event.to_value();
    }

    pub fn contains(&self, event: InterruptEvent) -> bool {
        self.0 & (1 << event.to_value()) != 0
    }
}

impl core::fmt::Debug for InterruptEventSet {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        use InterruptEvent::*;
        let mut set = f.debug_set();
        for event in &[PSC0, PSC1, VBlank0, VBlank1, PPF, P3D, DMA] {
            if self.contains(*event) {
                set.entry(event);
            }
        }

        set.finish()
    }
}

#[derive(Debug, EnumCast, Clone, Copy, PartialEq, Eq)]
#[enum_cast(value_type = "u8")]
pub enum FramebufferIndex {
    First,
    Second,
}

impl FramebufferIndex {
    fn swap(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::First,
        }
    }
}

impl core::ops::Not for FramebufferIndex {
    type Output = Self;
    fn not(self) -> Self::Output {
        self.swap()
    }
}

struct FramebufferInfo<'a> {
    header: &'a AtomicU32,
    fb_info: &'a mut [u32; 2 * 0x7],
}

impl<'a> FramebufferInfo<'a> {
    fn index(&mut self) -> FramebufferIndex {
        let header = self.header.load(Ordering::Relaxed).to_le_bytes();
        FramebufferIndex::from_value(header[0]).expect("Invalid framebuffer index from GSP")
    }

    fn next_fb_info(&mut self) -> &mut [u32; 0x7] {
        let next_index = self.index().swap();
        let fb_info_ptr = self.fb_info.as_mut_ptr();

        unsafe {
            core::mem::transmute(fb_info_ptr.offset(isize::from(next_index.to_value()) * 0x7))
        }
    }

    #[inline]
    fn update(
        &mut self,
        active_fb: FramebufferIndex,
        fb0: *const u8,
        fb1: *const u8,
        stride: u32,
        mode: u32,
    ) {
        let fb_info = self.next_fb_info();
        let active_fb = active_fb.to_value();
        fb_info[0] = u32::from(active_fb);
        fb_info[1] = fb0 as u32;
        fb_info[2] = fb1 as u32;
        fb_info[3] = stride;
        fb_info[4] = mode;
        fb_info[5] = u32::from(active_fb);
        fb_info[6] = 0;
        core::sync::atomic::fence(Ordering::Release);

        const FB_INDEX: usize = 0;
        const FB_UPDATE: usize = 1;

        let mut header = self.header.load(Ordering::Acquire);
        loop {
            let mut status = header.to_le_bytes();
            status[FB_INDEX] = active_fb;
            status[FB_UPDATE] = 1;

            match self.header.compare_exchange_weak(
                header,
                u32::from_le_bytes(status),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(new) => header = new,
            }
        }
    }
}

struct InterruptInfo {
    event_buf: *const u32,
}

impl InterruptInfo {
    fn header(&self) -> &AtomicU32 {
        unsafe { &*(self.event_buf.offset(0) as *const AtomicU32) }
    }

    fn load_header(&self, order: Ordering) -> InterruptHeader {
        let raw_header = self.header().load(order);

        InterruptHeader::from(raw_header)
    }

    fn store_header(
        &self,
        current: InterruptHeader,
        new: InterruptHeader,
        success: Ordering,
        failure: Ordering,
    ) -> core::result::Result<(), InterruptHeader> {
        match self
            .header()
            .compare_exchange(current.into(), new.into(), success, failure)
        {
            Ok(_) => Ok(()),
            Err(updated) => Err(InterruptHeader::from(updated)),
        }
    }

    unsafe fn read_event(&self, header: &InterruptHeader) -> Option<InterruptEvent> {
        let index = header.current_index;
        let block_idx = usize::from(index / 4);
        let part_idx = usize::from(index & 0b11);
        let current_event_ptr = self.event_buf.offset(0x3 + block_idx as isize) as *const AtomicU32;
        let event_packed = (&*current_event_ptr).load(Ordering::SeqCst);

        match InterruptEvent::from_value(event_packed.to_le_bytes()[3 - part_idx]) {
            Err(e) => {
                warn!("GSP wrote an invalid interrupt event: 0x{:02x}", e);
                None
            }
            Ok(event) => Some(event),
        }
    }
}

impl Sharedmem {
    fn wait_event(&mut self) -> Result<InterruptEventSet> {
        self.gpu_events.wait(Timeout::forever())?;

        self.gpu_events.clear()?;

        let mut events = InterruptEventSet::empty();
        while let Some(event) = self.pop_interrupt() {
            events.add(event)
        }

        Ok(events)
    }

    fn interrupt_info(&self) -> InterruptInfo {
        let event_buf = unsafe {
            self.shared_memory
                .as_ptr()
                .offset(self.gsp_module_thread_index as isize * 0x10)
        };
        InterruptInfo { event_buf }
    }

    fn pop_interrupt(&self) -> Option<InterruptEvent> {
        let info = self.interrupt_info();

        let mut header = info.load_header(Ordering::Acquire);
        loop {
            if header.events_total == 0 {
                return None;
            }

            let event = unsafe { info.read_event(&header) }?;

            let acknowledged = InterruptHeader {
                current_index: if header.current_index >= 0x34 {
                    0
                } else {
                    header.current_index + 1
                },
                events_total: header.events_total - 1,
                error: 0,
                _unused: header._unused,
            };

            match info.store_header(header, acknowledged, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => return Some(event),
                Err(updated) => {
                    header = updated;
                }
            }
        }
    }

    pub fn present_buffer(
        &mut self,
        screen: Screen,
        active_fb: FramebufferIndex,
        fb0: *const u8,
        fb1: *const u8,
        stride: u32,
        mode: u32,
    ) {
        let sharedmem = unsafe {
            &mut self.shared_memory.as_mut_slice_raw()[0x80
                + (self.gsp_module_thread_index as usize * 0x20)
                + (screen.to_value() as usize * 0x10)..][..0xd]
        };
        let (header, fb_info) = sharedmem.split_first_mut().unwrap();
        let mut fb_info = FramebufferInfo {
            header: AtomicU32::from_mut(header),
            fb_info: unsafe { core::mem::transmute(fb_info.as_mut_ptr()) },
        };

        fb_info.update(active_fb, fb0, fb1, stride, mode)
    }
}

#[derive(Debug)]
pub struct Gpu {
    access: AccessRightsToken,
    sharedmem: Sharedmem,
}

impl Gpu {
    pub fn init(srv: &Srv) -> Result<Self> {
        let service_handle = srv.get_service_handle("gsp::Gpu")?;

        const ACCESS_FLAGS: u8 = 0x00;
        let mut access =
            Self::aquire_access(service_handle, WeakHandle::active_process(), ACCESS_FLAGS)?;

        const QUEUE_FLAGS: u8 = 0x01;
        let gsp_relay_queue = Self::register_interrupt_relay_queue(&mut access, QUEUE_FLAGS)?;

        Ok(Self {
            access,
            sharedmem: gsp_relay_queue,
        })
    }

    fn aquire_access(
        service_handle: Handle,
        owner_process: WeakHandle,
        flags: u8,
    ) -> Result<AccessRightsToken> {
        let _reply = IpcRequest::command(0x16)
            .with_params(&[flags.into()])
            .with_translate_params(&[TranslateParameterSet::HandleRef(&[owner_process])])
            .dispatch(service_handle.handle())?;

        Ok(AccessRightsToken { service_handle })
    }

    fn register_interrupt_relay_queue(
        access: &mut AccessRightsToken,
        flags: u8,
    ) -> Result<Sharedmem> {
        use crate::result::{Level, Module, Summary};

        let gpu_events = Event::new(ResetType::OneShot)?;

        let (reply, result_code) = IpcRequest::command(0x13)
            .with_params(&[flags as u32])
            .with_translate_params(&[TranslateParameterSet::HandleRef(&[
                gpu_events.borrow_handle()
            ])])
            .dispatch_no_parse(access.borrow_handle())
            .map(Reply::parse_nofail)?;

        let gsp_module_thread_index = (reply.values[0] & 0xff) as u8;
        let queue_handle = unsafe { Handle::own(reply.translate_values[0]) };

        const RESULT_NEED_HW_INIT: ErrorCode =
            ErrorCode::new(Level::Success, Summary::Success, Module::Gsp, 519);

        match result_code.into_result() {
            Ok(()) => {}
            Err(RESULT_NEED_HW_INIT) => Self::init_hardware(access)?,
            Err(e) => return Err(e),
        };

        let shared_memory = Self::map_shared_memory(queue_handle)?;

        Ok(Sharedmem {
            gpu_events,
            gsp_module_thread_index,
            shared_memory,
        })
    }

    fn map_shared_memory(shared_memory_handle: Handle) -> Result<MappedBlock> {
        SharedMemoryMapper::global().map(
            shared_memory_handle,
            0x1000,
            MemoryPermission::Rw,
            MemoryPermission::DontCare,
        )
    }

    fn init_hardware(access: &mut AccessRightsToken) -> Result<()> {
        const GPU_REG_BASE: u32 = 0x40_0000;
        const INIT_VALUES: &'static [(u32, u32, Option<u32>)] = &[
            (0x1000, 0, None),
            (0x1080, 0x12345678, None),
            (0x10C0, 0xFFFFFFF0, None),
            (0x10D0, 1, None),
            // (0x1914, 1, None), // homebrew addition: make sure GPUREG_START_DRAW_FUNC0 starts off in configuration mode
            // Top screen LCD configuration, see https://www.3dbrew.org/wiki/GPU/External_Registers#LCD_Source_Framebuffer_Setup

            // Top screen sync registers:
            (0x0400, 0x1C2, None),
            (0x0404, 0xD1, None),
            (0x0408, 0x1C1, None),
            (0x040C, 0x1C1, None),
            (0x0410, 0, None),
            (0x0414, 0xCF, None),
            (0x0418, 0xD1, None),
            (0x041C, (0x1C5 << 16) | 0x1C1, None),
            (0x0420, 0x10000, None),
            (0x0424, 0x19D, None),
            (0x0428, 2, None),
            (0x042C, 0x192, None),
            (0x0430, 0x192, None),
            (0x0434, 0x192, None),
            (0x0438, 1, None),
            (0x043C, 2, None),
            (0x0440, (0x196 << 16) | 0x192, None),
            (0x0444, 0, None),
            (0x0448, 0, None),
            // Top screen fb geometry
            (0x045C, (400 << 16) | 240, None), // dimensions
            (0x0460, (0x1C1 << 16) | 0xD1, None),
            (0x0464, (0x192 << 16) | 2, None),
            // Top screen framebuffer format (initial)
            (0x0470, 0x80340, None),
            // Top screen unknown reg @ 0x9C
            (0x049C, 0, None),
            // Bottom screen LCD configuration

            // Bottom screen sync registers:
            (0x0500, 0x1C2, None),
            (0x0504, 0xD1, None),
            (0x0508, 0x1C1, None),
            (0x050C, 0x1C1, None),
            (0x0510, 0xCD, None),
            (0x0514, 0xCF, None),
            (0x0518, 0xD1, None),
            (0x051C, (0x1C5 << 16) | 0x1C1, None),
            (0x0520, 0x10000, None),
            (0x0524, 0x19D, None),
            (0x0528, 0x52, None),
            (0x052C, 0x192, None),
            (0x0530, 0x192, None),
            (0x0534, 0x4F, None),
            (0x0538, 0x50, None),
            (0x053C, 0x52, None),
            (0x0540, (0x198 << 16) | 0x194, None),
            (0x0544, 0, None),
            (0x0548, 0x11, None),
            // Bottom screen fb geometry
            (0x055C, Screen::Bottom.dimensions_register(), None), // dimensions
            (0x0560, (0x1C1 << 16) | 0xD1, None),
            (0x0564, (0x192 << 16) | 0x52, None),
            // Bottom screen framebuffer format (initial)
            (0x0570, 0x80300, None),
            // Bottom screen unknown reg @ 0x9C
            (0x059C, 0, None),
            // Initial, blank framebuffer (top left A/B, bottom A/B, top right A/B)
            (0x0468, 0x18300000, None),
            (0x046C, 0x18300000, None),
            (0x0568, 0x18300000, None),
            (0x056C, 0x18300000, None),
            (0x0494, 0x18300000, None),
            (0x0498, 0x18300000, None),
            // Framebuffer select: A
            (0x0478, 1, None),
            (0x0578, 1, None),
            // Clear DMA transfer (PPF) "transfer finished" bit
            (0x0C18, 0, Some(0xFF00)),
            // GX_GPU_CLK |= 0x70000 (value is 0x100 when gsp starts, enough to at least display framebuffers & have memory fill work)
            // This enables the clock to some GPU components
            (0x0004, 0x70100, None),
            // Clear Memory Fill (PSC0 and PSC1) "busy" and "finished" bits
            (0x001C, 0, Some(0xFF)),
            (0x002C, 0, Some(0xFF)),
            // More init registers
            (0x0050, 0x22221200, None),
            (0x0054, 0xFF2, Some(0xFFFF)),
            // Enable some LCD clocks (?) (unsure)
            (0x0474, 0x10501, None),
            (0x0574, 0x10501, None),
        ];

        for (register_offset, value, mask) in INIT_VALUES {
            let register = GPU_REG_BASE + register_offset;
            trace!(
                "Writing GSP register: {:#08x} = {:#08x} (mask: {:08x?})",
                register,
                value,
                mask
            );
            match mask {
                None => {
                    write_graphics_register(access.service_handle.borrow_handle(), register, value)?
                }
                Some(mask) => write_graphics_register_masked(
                    access.service_handle.borrow_handle(),
                    register,
                    value,
                    mask,
                )?,
            }
        }

        Ok(())
    }

    pub fn next_event(&mut self) -> Result<InterruptEventSet> {
        self.sharedmem.wait_event()
    }

    pub fn present_buffer(
        &mut self,
        screen: Screen,
        active_fb: FramebufferIndex,
        fb0: *const u8,
        fb1: *const u8,
        stride: u32,
        mode: u32,
    ) {
        self.sharedmem
            .present_buffer(screen, active_fb, fb0, fb1, stride, mode)
    }

    pub fn set_lcd_force_blank(&mut self, flags: u8) -> Result<()> {
        let _ = IpcRequest::command(0x0b)
            .with_params(&[flags as u32])
            .dispatch(self.access.borrow_handle())?;
        Ok(())
    }
}

#[derive(Debug)]
#[must_use = "GPU access rights must be released properly"]
struct AccessRightsToken {
    service_handle: Handle,
}

impl AccessRightsToken {
    pub fn borrow_handle(&self) -> WeakHandle {
        self.service_handle.handle()
    }

    fn release(&mut self) -> Result<Handle> {
        debug!("Releasing GPU access rights");
        let _ = IpcRequest::command(0x17).dispatch(self.service_handle.borrow_handle())?;
        Ok(self.service_handle.take())
    }
}

impl Drop for AccessRightsToken {
    fn drop(&mut self) {
        let _ = self.release();
    }
}

fn write_graphics_register(
    service_handle: WeakHandle,
    register_offset: u32,
    value: &u32,
) -> Result<()> {
    let data = core::slice::from_ref(value);
    let _ = IpcRequest::command(0x01)
        .with_params(&[register_offset, 4])
        .with_translate_params(&[TranslateParameterSet::StaticBuffer(data, 0)])
        .dispatch(service_handle)?;

    Ok(())
}

fn write_graphics_register_masked(
    service_handle: WeakHandle,
    register_offset: u32,
    value: &u32,
    mask: &u32,
) -> Result<()> {
    let value = core::slice::from_ref(value);
    let mask = core::slice::from_ref(mask);
    let _ = IpcRequest::command(0x02)
        .with_params(&[register_offset, 4])
        .with_translate_params(&[
            TranslateParameterSet::StaticBuffer(value, 0),
            TranslateParameterSet::StaticBuffer(mask, 1),
        ])
        .dispatch(service_handle)?;

    Ok(())
}
