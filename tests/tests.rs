use libloading::{Library, Symbol};
use std::ffi::{c_char, c_void};

type EvrInitError = i32;
const ERR_NONE: EvrInitError = 0;
const ERR_NOT_INITIALIZED: EvrInitError = 109;

#[test]
#[cfg_attr(miri, ignore)]
fn smoke_test() {
    let path = test_cdylib::build_current_project();
    let lib = unsafe { Library::new(&path) }.unwrap();
    let factory: Symbol<fn(*const c_char, *mut i32) -> *mut c_void> =
        unsafe { lib.get(b"VRClientCoreFactory\0") }.unwrap();

    let i = factory(c"IVRClientCore_003".as_ptr(), std::ptr::null_mut());
    assert!(!i.is_null());
}

/// Exercises the drop-in `openvr_api.dll` client entry points the same way a game would:
/// init, fetch interfaces, shutdown.
#[test]
#[cfg_attr(miri, ignore)]
fn drop_in_openvr_api_smoke() {
    let path = test_cdylib::build_current_project();
    let lib = unsafe { Library::new(&path) }.unwrap();

    type InitFn = unsafe extern "C" fn(*mut EvrInitError, i32, *const c_char) -> EvrInitError;
    type GetInterfaceFn = unsafe extern "C" fn(*const c_char, *mut EvrInitError) -> *mut c_void;
    type ShutdownFn = unsafe extern "C" fn();
    type IsHmdPresentFn = unsafe extern "C" fn() -> bool;

    let init = unsafe { lib.get::<InitFn>(b"VR_InitInternal\0") }.unwrap();
    let get_interface =
        unsafe { lib.get::<GetInterfaceFn>(b"VR_GetGenericInterface\0") }.unwrap();
    let shutdown = unsafe { lib.get::<ShutdownFn>(b"VR_ShutdownInternal\0") }.unwrap();
    let is_hmd_present =
        unsafe { lib.get::<IsHmdPresentFn>(b"VR_IsHmdPresent\0") }.unwrap();

    // Before init, interface requests must fail cleanly instead of crashing.
    let mut err: EvrInitError = -1;
    let early = unsafe { get_interface(c"IVRSystem_023".as_ptr(), &mut err) };
    assert!(early.is_null());
    assert_eq!(err, ERR_NOT_INITIALIZED);

    // VRApplication_Scene = 1.
    let mut err: EvrInitError = -1;
    let init_err = unsafe { init(&mut err, 1, std::ptr::null()) };
    assert_eq!(init_err, ERR_NONE);
    assert_eq!(err, ERR_NONE);
    assert!(unsafe { is_hmd_present() });

    let system = unsafe { get_interface(c"IVRSystem_023".as_ptr(), &mut err) };
    assert!(!system.is_null(), "expected IVRSystem_023");
    let compositor = unsafe { get_interface(c"IVRCompositor_029".as_ptr(), &mut err) };
    assert!(!compositor.is_null(), "expected IVRCompositor_029");
    let input = unsafe { get_interface(c"IVRInput_010".as_ptr(), &mut err) };
    assert!(!input.is_null(), "expected IVRInput_010");

    // After shutdown, interface requests fail cleanly again.
    unsafe { shutdown() };
    let mut err: EvrInitError = -1;
    let after = unsafe { get_interface(c"IVRSystem_023".as_ptr(), &mut err) };
    assert!(after.is_null());
    assert_eq!(err, ERR_NOT_INITIALIZED);
}
