#![deny(clippy::all)]

mod applications;
mod chaperone;
mod clientcore;
mod compositor;
#[cfg(test)]
mod fakexr;
mod graphics_backends;
mod input;
mod misc_unknown;
mod overlay;
mod overlayview;
mod rendermodels;
mod screenshots;
mod settings;
mod system;
mod udp_communication;
mod winlatorxr;

// #[cfg(not(test))]
// mod error_dialog;

use clientcore::ClientCore;
use openvr as vr;
use std::ffi::{CStr, c_char, c_void};
use std::sync::OnceLock;
use std::sync::{
    Arc,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

pub use winlatorxr::*;

macro_rules! warn_unimplemented {
    ($function:literal) => {
        crate::warn_once!("{} unimplemented ({}:{})", $function, file!(), line!());
    };
}
use warn_unimplemented;
macro_rules! warn_once {
    ($literal:literal $(,$($tt:tt)*)?) => {{
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            log::warn!(concat!("[ONCE] ", $literal) $(,$($tt)*)?);
        });
    }}
}
use warn_once;

#[cfg(feature = "tracing")]
macro_rules! tracy_span {
    ($($tt:tt)*) => {
        let _span = tracy_client::span!($($tt)*);
    }
}

#[cfg(not(feature = "tracing"))]
macro_rules! tracy_span {
    ($($tt:tt)*) => {};
}
use tracy_span;

#[cfg(feature = "tracing")]
tracy_client::register_demangler!();

macro_rules! atomic_float {
    ($name:ident, $float:ty, $atomic:ty) => {
        #[derive(Default)]
        struct $name($atomic);

        impl $name {
            fn new(value: $float) -> Self {
                Self(value.to_bits().into())
            }

            #[allow(dead_code)]
            #[inline]
            fn load(&self) -> $float {
                <$float>::from_bits(self.0.load(Ordering::Relaxed))
            }

            #[allow(dead_code)]
            #[inline]
            fn store(&self, value: $float) {
                self.0.store(value.to_bits(), Ordering::Relaxed)
            }

            #[allow(dead_code)]
            #[inline]
            fn swap(&self, value: $float) -> $float {
                <$float>::from_bits(self.0.swap(value.to_bits(), Ordering::Relaxed))
            }
        }

        impl From<$float> for $name {
            fn from(value: $float) -> Self {
                Self::new(value)
            }
        }
    };
}

atomic_float!(AtomicF32, f32, AtomicU32);
atomic_float!(AtomicF64, f64, AtomicU64);

fn init_logging() {
    static ONCE: std::sync::Once = std::sync::Once::new();

    ONCE.call_once(|| {
        let mut builder = env_logger::Builder::new();
        #[allow(unused_mut)]
        let mut startup_err: Option<String> = None;

        #[cfg(not(test))]
        {
            use std::path::Path;

            struct ComboWriter(std::fs::File, std::io::Stderr);

            impl std::io::Write for ComboWriter {
                fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                    let _ = self.0.write(buf)?;
                    self.1.write(buf)
                }

                fn flush(&mut self) -> std::io::Result<()> {
                    self.0.flush()?;
                    self.1.flush()
                }
            }

            let state_dir = std::env::var("XDG_STATE_HOME")
                .or_else(|_| std::env::var("HOME").map(|h| h + "/.local/state"));

            if let Ok(state) = state_dir {
                let path = Path::new(&state).join("xrizer");
                let mut setup = || {
                    let path = path.join("xrizer.txt");
                    match std::fs::File::create(path) {
                        Ok(file) => {
                            let writer = ComboWriter(file, std::io::stderr());
                            builder.target(env_logger::Target::Pipe(Box::new(writer)));
                        }
                        Err(e) => startup_err = Some(format!("Failed to create log file: {e:?}")),
                    }
                };

                match std::fs::create_dir_all(&path) {
                    Ok(_) => setup(),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => setup(),
                    err => {
                        startup_err = Some(format!(
                            "Failed to create log directory ({path:?}): {err:?}"
                        ))
                    }
                }
            }

            std::panic::set_hook(Box::new(|info| {
                log::error!("{info}");
                let backtrace = std::backtrace::Backtrace::force_capture();
                log::error!("Backtrace: \n{backtrace}");
                eprintln!("{info}");
                eprintln!("Backtrace: \n{backtrace}");
                std::process::abort();
            }));
        }

        builder
            .filter_level(log::LevelFilter::Info)
            .parse_default_env()
            .is_test(cfg!(test))
            .format(|buf, record| {
                use std::io::Write;
                use time::macros::format_description;

                let style = buf.default_level_style(record.level());
                let now = time::OffsetDateTime::now_utc();
                let now = now
                    .format(format_description!(
                        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]"
                    ))
                    .unwrap();

                write!(buf, "[{now} {style}{:5}{style:#}", record.level())?;
                if let Some(path) = record.module_path() {
                    write!(buf, " {path}")?;
                }
                writeln!(buf, " {:?}] {}", std::thread::current().id(), record.args())
            })
            .init();

        let mut version = env!("VERGEN_GIT_DESCRIBE");
        if version == "VERGEN_IDEMPOTENT_OUTPUT" {
            version = env!("CARGO_PKG_VERSION");
        }
        log::info!("Initializing XRizer version {version}");
        if let Some(err) = startup_err {
            log::warn!("{err}");
        }
    });
}

/// The underlying runtime object backing every entry point. Stored once and kept alive for the
/// lifetime of the process so both the runtime role (`VRClientCoreFactory`) and the drop-in client
/// role (`VR_InitInternal` etc.) share the same session.
struct ClientCoreRef {
    core: Arc<ClientCore>,
    vtable: *mut c_void,
}

// SAFETY: The vtable pointer points at static memory that lives as long as the leaked core.
unsafe impl Send for ClientCoreRef {}
unsafe impl Sync for ClientCoreRef {}

static CLIENT_CORE: OnceLock<ClientCoreRef> = OnceLock::new();

fn create_client_core(version: &CStr) -> Option<ClientCoreRef> {
    let core = ClientCore::new(version)?;
    let vtable = match core.base.get().unwrap() {
        clientcore::Vtable::V2(v) => v as *const _ as *const vr::IVRClientCore002 as _,
        clientcore::Vtable::V3(v) => v as *const _ as *const vr::IVRClientCore003 as _,
    };
    Some(ClientCoreRef { core, vtable })
}

fn client_core_ref(version: &CStr) -> Option<&'static ClientCoreRef> {
    if CLIENT_CORE.get().is_none() {
        let _ = CLIENT_CORE.set(create_client_core(version)?);
    }
    CLIENT_CORE.get()
}

/// # Safety
///
/// interface_name must be valid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn VRClientCoreFactory(
    interface_name: *const c_char,
    return_code: *mut i32,
) -> *mut c_void {
    let interface = unsafe { CStr::from_ptr(interface_name) };
    match client_core_ref(interface) {
        Some(core) => {
            if let Some(code) = unsafe { return_code.as_mut() } {
                *code = 0;
            }
            core.vtable
        }
        None => std::ptr::null_mut(),
    }
}

// -----------------------------------------------------------------------------------------------
// Drop-in openvr_api.dll entry points.
//
// Some clients do not load this DLL as a runtime via VRClientCoreFactory, but import the client
// API symbols directly (usually by renaming this DLL to `openvr_api.dll` next to the executable).
// Exporting these lets both styles work from a single binary.
// -----------------------------------------------------------------------------------------------

/// Called by the client's `VR_Init` to initialize the session.
///
/// # Safety
///
/// `pe_error` and `startup_info` must be valid as described by the OpenVR API.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn VR_InitInternal(
    pe_error: *mut vr::EVRInitError,
    application_type: vr::EVRApplicationType,
    startup_info: *const c_char,
) -> vr::EVRInitError {
    use openvr::IVRClientCore003_Interface;

    let err = match client_core_ref(c"IVRClientCore_003") {
        Some(core) => {
            <ClientCore as IVRClientCore003_Interface>::Init(core.core.as_ref(), application_type, startup_info)
        }
        None => vr::EVRInitError::Init_FactoryNotFound,
    };
    if let Some(err_out) = unsafe { pe_error.as_mut() } {
        *err_out = err;
    }
    err
}

/// Called by the client's `VR_Shutdown` to tear down the session.
#[unsafe(no_mangle)]
pub extern "C" fn VR_ShutdownInternal() {
    use openvr::IVRClientCore003_Interface;

    if let Some(core) = CLIENT_CORE.get() {
        <ClientCore as IVRClientCore003_Interface>::Cleanup(core.core.as_ref());
    }
}

/// Called by the client's `VR_GetGenericInterface` to fetch a runtime interface.
///
/// # Safety
///
/// `name_and_version` must be a valid NUL-terminated string, and `pe_error` a valid pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn VR_GetGenericInterface(
    name_and_version: *const c_char,
    pe_error: *mut vr::EVRInitError,
) -> *mut c_void {
    use openvr::IVRClientCore003_Interface;

    let Some(core) = CLIENT_CORE.get().filter(|core| core.core.is_initialized()) else {
        if let Some(err) = unsafe { pe_error.as_mut() } {
            *err = vr::EVRInitError::Init_NotInitialized;
        }
        return std::ptr::null_mut();
    };
    <ClientCore as IVRClientCore003_Interface>::GetGenericInterface(core.core.as_ref(), name_and_version, pe_error)
}

/// Called by the client's `VR_IsHmdPresent`.
#[unsafe(no_mangle)]
pub extern "C" fn VR_IsHmdPresent() -> bool {
    true
}

fn hmd_error_string(error: vr::EVRInitError) -> &'static str {
    match error {
        vr::EVRInitError::None => "No Error",
        vr::EVRInitError::Init_NotInitialized => "Not Initialized",
        vr::EVRInitError::Init_FactoryNotFound => "Factory Not Found",
        vr::EVRInitError::Init_InterfaceNotFound => "Interface Not Found",
        vr::EVRInitError::Init_InvalidApplicationType => "Invalid Application Type",
        vr::EVRInitError::Init_VRServiceStartupFailed => "VR Service Startup Failed",
        _ => "Unknown Error",
    }
}

/// Called by the client's `VR_GetStringForHmdError`. Copies the error message into `buffer` and
/// returns the number of bytes copied (excluding the NUL terminator).
///
/// # Safety
///
/// `buffer` must point to `buffer_size` valid bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn VR_GetStringForHmdError(
    error: vr::EVRInitError,
    buffer: *mut c_char,
    buffer_size: u32,
) -> i32 {
    let msg = hmd_error_string(error);
    if buffer.is_null() || buffer_size == 0 {
        return msg.len() as i32;
    }
    let n = (msg.len()).min(buffer_size as usize);
    unsafe { std::ptr::copy_nonoverlapping(msg.as_ptr(), buffer.cast::<u8>(), n) };
    n as i32
}

/// Called by the client's `VR_GetInitTokenAndVersion`. Writes a compatibility API version and
/// returns an init token that changes whenever the runtime changes.
///
/// # Safety
///
/// `version` and `pe_error` must be valid pointers.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn VR_GetInitTokenAndVersion(
    version: *mut u64,
    pe_error: *mut vr::EVRInitError,
) -> u32 {
    if let Some(err) = unsafe { pe_error.as_mut() } {
        *err = vr::EVRInitError::None;
    }
    if let Some(version) = unsafe { version.as_mut() } {
        *version = 1_000_115;
    }
    1
}

/// Needed for Proton, but seems unused.
///
/// # Safety
///
/// `return_code` must be valid if it is non-null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn HmdSystemFactory(
    _interface_name: *const c_char,
    return_code: *mut i32,
) -> *mut c_void {
    if let Some(code) = unsafe { return_code.as_mut() } {
        *code = vr::EVRInitError::Init_InterfaceNotFound as i32;
    }
    std::ptr::null_mut()
}
