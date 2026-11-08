// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use core::ops::Deref;

use num_enum::{IntoPrimitive, TryFromPrimitive};

use crate::ipc::{self, CommandBufferWriter, IpcRequest, StaticBuffer};
use crate::os::{OwnedHandle, ValidRawHandle};
use crate::ports::srv::{LazyServiceSession, Srv};
use crate::result::{Level, Result, ResultCode, ResultValue};
use crate::svc::Timeout;
use crate::sync::{Event, Mutex, MutexGuard, OsMutex};
use crate::{env, svc};

const APT_SERVICE_NAMES: [&str; 3] = ["APT:S", "APT:A", "APT:U"];

#[derive(Clone, Copy)]
struct AppletAttributes(u8);

impl AppletAttributes {
    const fn new() -> Self {
        Self(0)
    }

    const fn position(self, position: AppPosition) -> Self {
        Self(self.0 | (position as u8))
    }

    const fn manual_gpu_rights(self) -> Self {
        Self(self.0 | (1 << 3))
    }

    const fn manual_dsp_rights(self) -> Self {
        Self(self.0 | (1 << 4))
    }
}

impl ipc::Parameter for AppletAttributes {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(self.0.into())
    }
}

pub struct Apt {
    handle: OwnedHandle,
}

impl Apt {
    fn new(handle: OwnedHandle) -> Self {
        Self { handle }
    }

    fn get_lock(&self, flags: u16) -> Result<OsMutex> {
        let mut reply = IpcRequest::command(0x01)
            .parameter(u32::from(flags))
            .dispatch(&self.handle)?;

        let _applet_attributes = reply.read_word();
        let _apt_state = reply.read_word();

        let mut reply = reply.finish_results();
        let lock_handle = unsafe { reply.read_handle() };

        Ok(unsafe { OsMutex::from_handle(lock_handle) })
    }

    /// Attach to APT under the given [`AppId`].
    ///
    /// Fails if an applet with this [`AppletAttributes::position`] is already attached.
    /// Returns event handles that signal pending APT events (parameters and notifications).
    ///
    /// This is [`APT:Initialize`](https://www.3dbrew.org/wiki/APT:Initialize).
    fn attach(&self, app_id: AppId, attributes: AppletAttributes) -> Result<Events> {
        let reply = IpcRequest::command(0x02)
            .parameter(app_id)
            .parameter(attributes)
            .dispatch(&self.handle)?;

        let mut reply = reply.finish_results();
        let [notification, parameter] = unsafe { reply.read_handles() };

        Ok(Events {
            notification: unsafe { Event::from_handle(notification) },
            parameter: unsafe { Event::from_handle(parameter) },
        })
    }

    /// Subscribe to parameter messages for the given attributes.
    ///
    /// This is [`APT:Enable`](https://www.3dbrew.org/wiki/APT:Enable).
    fn subscribe_parameters(&self, attributes: AppletAttributes) -> Result<()> {
        let _ = IpcRequest::command(0x03)
            .parameter(attributes)
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn finalize(&self, app_id: AppId) -> Result<()> {
        let _ = IpcRequest::command(0x04)
            .parameter(app_id)
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn is_registered(&self, app_id: AppId) -> Result<bool> {
        let mut reply = IpcRequest::command(0x09)
            .parameter(app_id)
            .dispatch(&self.handle)?;
        let is_registered = reply.read_word() != 0;

        Ok(is_registered)
    }

    fn receive_parameter(&self, app_id: AppId, buffer: &mut [u8]) -> Result<Parameter> {
        let mut reply = IpcRequest::command(0x0d)
            .parameter(app_id)
            .parameter(buffer.len())
            .dispatch_with_receive_buffers(&self.handle, &mut [buffer])?;

        let sender = reply.read_word();
        let command = Command::try_from(reply.read_word());
        let bytes_written = reply.read_word() as usize;

        let mut reply = reply.finish_results();
        let handle: Option<OwnedHandle> = unsafe { reply.read_translate_result() };

        Ok(Parameter {
            sender,
            command,
            bytes_written,
            handle,
        })
    }

    fn prepare_to_close_application(&self, return_to_system: bool) -> Result<()> {
        log::info!("Preparing to close app (returning to system: {return_to_system})");
        let _ = IpcRequest::command(0x22)
            .parameter(u32::from(return_to_system))
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn close_application_raw(&self, payload: &[u32], handle: Option<ValidRawHandle>) -> Result<()> {
        log::info!("Closing app!");
        let _ = IpcRequest::command(0x27)
            .parameter(size_of_val(payload))
            .translate_parameter(handle)
            .translate_parameter(StaticBuffer::from_u32_slice(payload, 0))
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn close_application(&self, payload: &[u32]) -> Result<()> {
        self.close_application_raw(payload, None)
    }

    fn prepare_to_do_application_jump(
        &self,
        target: JumpTarget,
        program_id: u64,
        media_type: u8,
    ) -> Result<()> {
        log::info!("Preparing application jump to {target:?}, {program_id:#x}");
        let _ = IpcRequest::command(0x31)
            .parameters(&[
                target as u32,
                program_id as u32,
                (program_id >> 32) as u32,
                media_type as u32,
            ])
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn do_application_jump(&self, parameter: &[u8], hmac: &[u8; 0x20]) -> Result<()> {
        log::info!("Attempting application jump");
        let _ = IpcRequest::command(0x32)
            .parameter(parameter.len())
            .parameter(hmac.len())
            .translate_parameter(StaticBuffer::new(parameter, 0))
            .translate_parameter(StaticBuffer::new(hmac, 2))
            .dispatch(&self.handle)?;
        Ok(())
    }

    /// [`APT:NotifyToWait`](https://www.3dbrew.org/wiki/APT:NotifyToWait), a no-op.
    fn notify_to_wait(&self, app_id: AppId) -> Result<()> {
        let _ = IpcRequest::command(0x43)
            .parameter(app_id)
            .dispatch(&self.handle)?;
        Ok(())
    }

    fn applet_utility(&self, utility: Utility, input: &[u8], output: &mut [u8]) -> Result<()> {
        let mut reply = IpcRequest::command(0x4b)
            .parameter(utility)
            .parameter(input.len())
            .parameter(output.len())
            .translate_parameter(StaticBuffer::new(input, 1))
            .dispatch_with_receive_buffers(&self.handle, &mut [output])?;

        // Result code of the utility
        let result: ResultCode = reply.read_result();
        if matches!(result.level(), Ok(Level::Success)) {
            Ok(())
        } else {
            result.into_result()
        }
    }

    fn unlock_transition(&self, transition: u32) -> Result<()> {
        self.applet_utility(
            Utility::UnlockTransition,
            &transition.to_le_bytes(),
            &mut [0u8; 1],
        )
        .inspect_err(|err| log::error!("unlock_transition failed with {err:?}"))
    }

    fn sleep_if_shell_closed(&self) -> Result<()> {
        self.applet_utility(Utility::SleepIfShellClosed, &[0; 1], &mut [0; 1])
            .inspect_err(|err| log::error!("sleep_if_shell_closed failed with {err:?}"))
    }
}

struct LazyApt {
    session: LazyServiceSession,
}

impl LazyApt {
    fn open_session(&mut self) -> Result<Apt> {
        self.session.open().map(Apt::new)
    }
}

#[repr(C)]
struct Events {
    notification: Event,
    parameter: Event,
}

pub struct AptLock {
    events: Events,
    app_id: AppId,
    apt: Mutex<LazyApt>,
}

impl AptLock {
    pub fn init(srv: Srv) -> Result<Self> {
        let mut session = LazyServiceSession::new(srv, &APT_SERVICE_NAMES);

        let mutex = {
            // Obtain the mutex for cross-process synchronization.
            const FLAGS: u16 = 0x0;
            let mutex = Apt::new(session.open()?).get_lock(FLAGS)?;
            Mutex::const_new(mutex, LazyApt { session })
        };

        let app_id = AppId::from_env().expect("environment should contain a valid App ID");

        // Perform the APT initialization sequence in a temporary session:
        let events = {
            let apt = AptGuard::lock(&mutex)?;

            // Tell APT which app we are.
            const ATTRS: AppletAttributes = AppletAttributes::new().position(AppPosition::App);
            let events = apt.attach(app_id, ATTRS)?;

            // Mark ourselves as ready to receive parameters.
            apt.subscribe_parameters(ATTRS)?;

            // No-op, but called in libctru.
            apt.notify_to_wait(app_id)?;

            events
        };

        let apt = Self {
            events,
            app_id,
            apt: mutex,
        };

        let command = apt.wait_for_wakeup()?;

        if command != Command::WakeupJumpToHome {
            let apt = apt.lock()?;
            apt.unlock_transition(0x10)?;
            apt.sleep_if_shell_closed()?;
        } else {
            unimplemented!("Received `Command::WakeupJumpToHome` in APT initialization")
        }

        Ok(apt)
    }

    fn schedule_exit(self) -> Result<()> {
        let AptLock { apt, .. } = self;
        let guard = AptGuard::lock(&apt)?;

        let do_chainload = env::system_runflags().chainload_apt();
        if do_chainload && guard.is_registered(AppId::HomeMenu)? {
            guard.prepare_to_do_application_jump(JumpTarget::Ourself, 0, 0)?;
            static DELIVER_ARG: [u8; 0x300] = [0; _];
            static HMAC: [u8; 0x20] = [0; _];
            guard.do_application_jump(&DELIVER_ARG, &HMAC)?;
        } else {
            const RETURN_TO_SYSTEM: bool = true;
            guard.prepare_to_close_application(RETURN_TO_SYSTEM)?;
            guard.close_application(&[])?;
        }

        Ok(())
    }

    pub fn exit(self) -> ! {
        if let Err(err) = self.schedule_exit() {
            log::error!("Failed to exit via APT: {err:?}");
        }
        svc::exit_process()
    }

    pub fn lock(&self) -> Result<AptGuard<'_>> {
        AptGuard::lock(&self.apt)
    }

    pub(crate) fn wait_for_parameter(&self, buffer: &mut [u8]) -> Result<Parameter> {
        self.events.parameter.wait(Timeout::forever())?;
        self.lock()?.receive_parameter(self.app_id, buffer)
    }

    pub(crate) fn wait_for_wakeup(&self) -> Result<Command> {
        let mut buf = alloc::vec![0u8; 0x1000].into_boxed_slice();
        loop {
            let parameter = match self.wait_for_parameter(&mut buf) {
                Ok(parameter) => parameter,
                Err(err) => {
                    log::warn!("Ignoring failed Apt::wait_for_parameter: {err:?}");
                    continue;
                }
            };

            match parameter.command {
                Ok(command) if command.is_wakeup() => return Ok(command),
                Ok(other) => log::debug!("Ignoring non-wakeup command {other:?}"),
                Err(unkn) => log::warn!(
                    "Ignoring unknown command {:#x} from {:#x}",
                    unkn.number,
                    parameter.sender
                ),
            }
        }
    }
}

pub struct AptGuard<'lock> {
    // Fields in drop order:
    apt: Apt,                           // 1. close the APT session,
    _guard: MutexGuard<'lock, LazyApt>, // 2. release the access lock.
}

impl<'lock> AptGuard<'lock> {
    fn lock(mutex: &'lock Mutex<LazyApt>) -> Result<Self> {
        let mut guard = mutex.lock();
        let apt = guard.open_session()?;
        Ok(Self { apt, _guard: guard })
    }
}

impl Deref for AptGuard<'_> {
    type Target = Apt;

    fn deref(&self) -> &Self::Target {
        &self.apt
    }
}

#[derive(Debug, IntoPrimitive, TryFromPrimitive, Clone, Copy)]
#[repr(u32)]
enum AppId {
    HomeMenu = 0x101,
    Camera = 0x110,
    FriendsList = 0x112,
    GameNotes = 0x113,
    Web = 0x114,
    InstructionManual = 0x115,
    Notifications = 0x116,
    MiiVerse = 0x117,
    MiiVersePosting = 0x118,
    AmiiboSettings = 0x119,
    Application = 0x300,
    EShop = 0x301,
    SoftwareKeyboard = 0x401,
    AppletEd = 0x402,
    PNoteAp = 0x404,
    SNoteAp = 0x405,
    Error = 0x406,
    Mint = 0x407,
    Extrapad = 0x408,
    Memolib = 0x409,
}

impl AppId {
    pub fn from_env() -> core::result::Result<AppId, u32> {
        Self::try_from_primitive(env::app_id()).map_err(|err| err.number)
    }
}

#[derive(Debug, IntoPrimitive)]
#[repr(u8)]
enum AppPosition {
    App,
    AppLib,
    System,
    SystemLib,
    Resident,
    None = 0x7,
}

impl ipc::Parameter for AppId {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(u32::from(*self))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
pub(crate) enum Command {
    None = 0x0,
    Wakeup,
    Request,
    Response,
    Exit,
    Message,
    HomebuttonOnce,
    HomebuttonTwice,
    DspSleep,
    DspWakeup,
    WakeupExit,
    WakeupPause,
    WakeupCancel,
    WakeupCancelAll,
    WakeupPowerbutton,
    WakeupJumpToHome,
    SysappletRequest,
    WakeupLaunchApp,
}

type UnknownCommand = num_enum::TryFromPrimitiveError<Command>;

impl Command {
    fn is_wakeup(&self) -> bool {
        matches!(
            self,
            Command::Wakeup
                | Command::WakeupExit
                | Command::WakeupPause
                | Command::WakeupCancel
                | Command::WakeupCancelAll
                | Command::WakeupPowerbutton
                | Command::WakeupJumpToHome
                | Command::WakeupLaunchApp
        )
    }
}

pub(crate) struct Parameter {
    sender: u32,
    command: core::result::Result<Command, UnknownCommand>,
    bytes_written: usize,
    handle: Option<OwnedHandle>,
}

#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(u32)]
enum Utility {
    ClearPowerButtonState = 0,
    ClearExclusiveControl = 3,
    SleepIfShellClosed = 4,
    LockTransition = 5,
    TryLockTransition = 6,
    UnlockTransition = 7,
    StartExitTask = 10,
    SetInitialSenderId = 11,
    SetPowerButtonClick = 12,
    UnlockCartAndSdSlot = 16,
}

impl ipc::Parameter for Utility {
    fn encode(&self, cmdbuf: &mut CommandBufferWriter) {
        cmdbuf.write(u32::from(*self));
    }
}

/// Flags for [`APT:PrepareToDoApplicationJump`](https://www.3dbrew.org/wiki/APT:PrepareToDoApplicationJump).
#[derive(Debug, IntoPrimitive)]
#[repr(u32)]
enum JumpTarget {
    /// Jump to the given program ID and media type.
    Explicit = 0,
    /// Jump back to the caller, using the parameters from the NS state.
    Caller = 1,
    /// Jump to the parameters already set for the application (app ID 0x300), i.e. ourselves.
    Ourself = 2,
}
