# WinlatorXR Integration - Current Status & Progress Plan

## Current Status (as of Tue Sep 08 2026)
- **Compilation Errors**: 0 (`cargo check --workspace` clean, exit 0)
- **Test Suite**: 94 lib tests passed, 2 ignored (monado-gated); 2 integration tests passed (runtime-DLL smoke + drop-in `openvr_api.dll` smoke)
- **Status**: **GREEN** - port compiles and tests pass; runtime integration testing requires hardware

## Architecture Summary

The OpenXR dependency has been fully replaced with an in-crate shim (`src/winlatorxr.rs`, 2724 lines) that speaks the WinlatorXR UDP protocol. The original xrizer codebase's OpenVR interface implementations (`IVRCompositor`, `IVRInput`, `IVROverlay`, `IVRSystem`, etc.) are preserved on top of this shim. Test infrastructure uses an in-crate `fakexr` module (`src/fakexr.rs`, `src/fakexr/vulkan.rs`) compiled under `#[cfg(test)]` that provides controllable fake runtime state for compositor and input tests.

## Implemented (real, functional code)

### UDP Communication (`src/udp_communication.rs`)
- `UdpComm::new()` — binds Rx to 127.0.0.1:7872 (fallback 7873), Tx to random port
- `receive_pose()` — parses CSV into `WinlatorPoseData` (HMD + two hands + 19-button state + FOV/IPD)
- `send_haptic()` — sends `L_VIBE,R_VIBE,VR,SBS,FOV_W,FOV_H` CSV to 127.0.0.1:7278

### Session (`src/winlatorxr.rs` Session impl)
- `Session::new()` — spawns `pose_receiver_thread`; parses incoming UDP, stores latest pose
- `Session::wait_frame()` — busy-loops on frame ID change (matches WinlatorXR frame cadence)
- `Session::end_frame()` — detects SBS from `CompositionLayerProjection`, sends haptic Tx with VR/SBS flags
- `Session::begin()` / `end()` — state machine transitions (Synchronized → Stopping)
- `Session::send_haptic()` — forwards haptic data to UDP

### Shim (`src/winlatorxr.rs`)
- `WinlatorXrData::new()` — creates Instance, SessionData, input/compositor injection
- `SessionData::locate_views()` — real stereo view computation from HMD pose + IPD + FOV
- `Space::locate_with_pose()` — real HMD pose for View/Local/Stage references
- `hash_path()` — FNV-1a 64-bit path hashing with global cache
- `get_app_name()` — Windows `std::env::current_exe()`

### Graphics Backends (`src/graphics_backends/`)
- `DirectX11` — primary backend, selected first via `select_preferred_backend()`
- `Vulkan` — fallback backend (ash-based)
- OpenGL backend removed; OpenGL types return `InvalidGraphicsBinding`

### OpenVR Interfaces (preserved from original xrizer)
- `IVRCompositor` versions 009–029 (`src/compositor.rs`, 1978 lines)
- `IVRInput` versions 004–010 (`src/input.rs`, 1759 lines)
- `IVROverlay` versions 007–028 (`src/overlay.rs`, 1563 lines)
- `IVRSystem` (`src/system.rs`, 1056 lines)
- IVRRenderModels, IVRSettings, IVRScreenshots, IVRChaperone, IVROverlayView, IVRApplications, IVRMiscellaneousUnknown

### Test Infrastructure (`src/fakexr.rs`, `src/fakexr/vulkan.rs`)
- In-crate `fakexr` module: controllable fake runtime (action states, poses, haptics, frame states)
- Fake Vulkan dispatcher: `ash::Entry::from_static_fn` + `get_instance_proc_addr` (test mode only)
- Compositor tests: 16 tests covering bounds, submit ordering, swapchain recreation, frame timing, overlays
- Smoke test: `test-cdylib` loads the DLL, asserts `VRClientCoreFactory` returns `IVRClientCore_003`, and exercises the full drop-in `openvr_api.dll` client sequence (init → `IVRSystem_023`/`IVRCompositor_029`/`IVRInput_010` → shutdown)

## Stubbed (compile but no real behavior in production path)

| Component | What it does | Why stubbed | Path |
|-----------|-------------|-------------|------|
| `Space::locate()` | Returns identity pose always | Real behavior only via `locate_with_pose` | Space (1428) |
| `HandTracker::locate_hand_joints()` | Returns `joint_count:0` | No skeletal data from WinlatorXR UDP | HandTracker (2545) |
| `Swapchain::acquire/wait/release` | Return `Ok(0)` / `Ok(())` | No real framebuffer; DX11 backend handles textures internally | Swapchain (1609) |
| `Swapchain::enumerate_images()` | Returns `Vec![0]` | Same | Swapchain (1620) |
| `WinlatorXrData::reset/set_tracking_space()` | Returns `Err("Not implemented")` | Not exposed by WinlatorXR protocol | WinlatorXrData (233) |
| `poll_events_impl()` | Returns `None` | Event loop not implemented; state managed via fakexr in tests | WinlatorXrData (186) |
| `Action::state()` (non-test) | Resolves from live UDP pose data | Now wired for legacy + name-mapped action queries; full binding-resolution still inert | Action (2267) |
| `Action::is_active()` (non-test) | True when the action maps to a resolvable physical input | Now wired; only true once pose data has arrived | Action (2303) |
| `Action::apply_feedback()` (non-test) | Sends haptic only | Works, but action itself never appears active | Action (2418) |
| `Session::create_swapchain()` | Returns `Swapchain` (no GPU state) | Compositor tests use fake backend; real swapchains go through DX11 | Session (1299) |

## Intentionally Excluded

- **OpenGL backend**: Removed per plan; DX11 is primary, Vulkan is fallback. OpenGL types remain for API compat.
- **Monado GenericTracker**: Gated behind `#[cfg(feature = "monado")]` which is not declared in Cargo.toml. Depends on the OpenXR-era `openxr_mndx_xdev_space` crate (not ported). Two tests (`get_tracker_pose`, `get_tracker_serial`) are `#[ignore]` per `cfg_attr`.
- **`openxr_mndx_xdev_space`**: Not a workspace dependency. The tracker subsystem was OpenXR-specific (Monado's XDEV extension) and is dead code in this port.

## Key Architectural Decisions

1. **File named `winlatorxr.rs`**, not `wxr_data.rs` (the original plan's rename to `wxr_data.rs` was not carried out; `openxr_data.rs` became `winlatorxr.rs` directly).
2. **`fakexr` is an in-crate `#[cfg(test)]` module**, not a workspace member. The root `fakexr/` directory was deleted as orphaned dead code.
3. **`test-cdylib` in `[dev-dependencies]`** for the smoke test (builds the DLL, loads `VRClientCoreFactory`, asserts non-null `IVRCoreClient_003` pointer).
4. **Frame timing** uses WinlatorXR's frame ID change detection (not `xrWaitFrame` / `xrBeginFrame`).

## Remaining Work (runtime testing required)

1. **Runtime integration test** — load DLL in SteamVR-compatible runner, verify HMD pose arrives over UDP, verify compositor frames render
2. **DX11 texture pipeline** — verify `copy_texture_to_swapchain` / `copy_overlay_to_swapchain` actually produce visible frames
3. **Skeletal tracking** — `locate_hand_joints` now synthesizes 26 OpenXR joints from the controller pose + button states (interpolating the SteamVR open-hand/fist reference skeletons). It reports `Full` tracking level; joint orientation/placement still needs runtime verification on hardware.
4. **Swapchain lifecycle** — currently returns placeholder values; may need real GPU texture management for production
5. **Per-profile binding resolution** — `current_interaction_profile` now returns a real interaction profile in production (default `/interaction_profiles/oculus/touch_controller`, overridable via `XRIZER_INTERACTION_PROFILE`), and the production session auto-creates/connects both controllers on the first poll once a real session exists. The `state_from_bindings` per-profile override path (dpad/grab/toggle) and interaction-profile-change handling are wired; behavior on real hardware still needs runtime verification.

## Implemented: Drop-in openvr_api.dll client exports (2026-09-08)

The DLL now works both as an OpenVR **runtime** (via `VRClientCoreFactory`) and as a drop-in **client** DLL renamed `openvr_api.dll` and placed beside the game executable. This lets WinlatorXR drop the file per game shortcut without any registry / `openvrpaths.vrpath` setup.

- Exported client entry points (verified in the release DLL's export table):
  - `VR_InitInternal(EVRInitError*, EVRApplicationType, const char*) -> EVRInitError` — creates (once) and `Init`s the shared `ClientCore`, `Scene`/`Background` app types only.
  - `VR_ShutdownInternal()` — `ClientCore::Cleanup` (clears the interface store, drops the OpenXR data); a subsequent `VR_InitInternal` restarts cleanly.
  - `VR_GetGenericInterface(const char*, EVRInitError*) -> void*` — exact same dispatch as `ClientCore::GetGenericInterface` (System/Compositor/Input/RenderModels/Overlay/Chaperone/Applications/OverlayView/Screenshots/Settings/Unknown); guarded so it returns `Init_NotInitialized` instead of panicking before or after init.
  - `VR_IsHmdPresent() -> bool`, `VR_GetStringForHmdError(EVRInitError, char*, u32) -> i32`, `VR_GetInitTokenAndVersion(u64*, EVRInitError*) -> u32`.
- The runtime and client roles share one `ClientCore` held in a global `OnceLock` (`src/lib.rs`, `ClientCoreRef`), so a factory-created core and a `VR_InitInternal`-created core are the same session. `Arc::into_raw` leak removed.
- New integration test `drop_in_openvr_api_smoke` (tests/tests.rs) does the full game sequence: verify clean failure before init → `VR_InitInternal` (Scene) → fetch `IVRSystem_023` / `IVRCompositor_029` / `IVRInput_010` → `VR_ShutdownInternal` → clean failure again.
- `HmdSystemFactory` marked `unsafe extern "C"` (derefs the raw `return_code` pointer) with a `# Safety` doc; this satisfies the denied `not_unsafe_ptr_arg_deref` / `missing_safety_doc` clippy lints.

## Implemented: XrAPI startup Tx packet (2026-09-08)

WinlatorXR only begins streaming tracking data after it receives at least one Tx UDP packet (`docs/PROTOCOL.md` "UDP Tx Startup"). Previously the first packet was only sent by the per-frame `end_frame`, so XrAPI wouldn't start until the game submitted its first frame.

- `Session::send_startup_packet()` sends the `0 0 1 0 104.5 104.5` handshake (VR immersive, monocular, default FOV).
- `SessionData::new` now calls it the moment a real session exists (`!temp_vulkan`), i.e. as soon as the compositor initializes the real DX11/Vulkan session — before any frame is submitted. A failure only warns; the per-frame `end_frame` keepalives double as retries.
- `udp_communication::format_haptic_message` was extracted from `send_haptic` so the wire format is unit-testable; tests assert the exact protocol example and an SBS/vibration variant.
- Test: `real_session_creation_sends_startup_packet`.

## Implemented: Startup panic-landmine hardening (2026-09-08)

Games calling certain compositor/input helpers previously hit `todo!()`/`unimplemented!()` panics at startup or mid-frame. These no longer crash:

- `Compositor::GetPosesForFrame` now fills the pose array via `GetLastPoses` (same data as `WaitGetPoses`), instead of panicking.
- `Compositor::IsMotionSmoothingSupported`/`IsMotionSmoothingEnabled` return `false` (no motion smoothing).
- `Compositor::SubmitWithArrayIndex` delegates to `Submit` (array index ignored, matching the single-sampled swapchain).
- GL shared-texture/mirror, `ForceReconnectProcess`, `CompositorDumpImages`, mirror-window visibility, `GetLastFrameRenderer`, `CompositorQuit`, `GetCurrentFadeColor` (transparent), `GetCumulativeStats` (zeroed), `GetFrameTimings` (0), `RequestScreenshot`/`GetCurrentScreenshotType` — all return safe defaults.
- `Compositor::GetCurrentSceneFocusProcess` returns the process's own PID (correct: the game and this runtime share a process).
- Input: `GetComponentStateForBinding` (zeroed state), `ShowBindingsForActionSet`/`ShowActionOrigins`/`GetBoneName`/`GetBoneHierarchy` (None), compressed-skeletal `DecompressSkeletalBoneData`/`GetSkeletalBoneDataCompressed` (InvalidParam — we only emit uncompressed bones).
- `HmdSystemFactory` export returns NULL + `Init_InterfaceNotFound` instead of `unimplemented!()`.

3 regression tests added (motion smoothing, `GetPosesForFrame`, `SubmitWithArrayIndex`).

## Implemented: Skeletal tracking (2026-09-08)

- `HandTracker::locate_hand_joints` (winlatorxr.rs) is no longer a stub. `HandTracker` now captures a clone of the session's `latest_pose_data` Arc and a smoothed per-hand curl state; each call maps the WinlatorXR buttons to per-finger curls (trigger→index, grip→middle/ring/pinky, thumbstick-click→thumb), smooths them, and synthesizes 26 OpenXR joints.
- New `input::skeletal::synthesize_hand_joints(hand, curls)` interpolates the SteamVR open-hand/fist reference bones by curl amount, converts parent-space → model-space, and maps them to OpenXR `HandJoint` indices relative to the base (grip-pose) space — i.e. "controller position + angle" anchors the hand. Palm is approximated between wrist and index metacarpal.
- This feeds both `GetSkeletalBoneData` (bone transforms) and `GetSkeletalSummaryData` (finger curl values computed from the joint geometry), and removes the latent production panic where the 0-joint stub produced out-of-bounds indexing in `get_bones_from_hand_tracking`.
- Note: reports `EVRSkeletalTrackingLevel::Full` since raw joints exist; true fidelity is hardware-tuning territory.

## Implemented: Per-profile binding resolution (2026-09-08)

- `Session::current_interaction_profile` returns `interaction_profile_path()` in production: a process-lifetime cached path, default Oculus Touch (matches WinlatorXR's thumbstick + A/B + grip/trigger layout), overridable via `XRIZER_INTERACTION_PROFILE`. The `#[cfg(test)]` fakexr branch is unchanged.
- New `Input::ensure_production_controllers()` (input.rs:1451) — WinlatorXR never emits interaction-profile events, so `poll_events` now calls this once controllers are absent: it drives the existing `interaction_profile_changed` path, which creates both controllers (indices 1/2), marks them `connected`, and attaches `ProfileData` for the chosen profile. It is idempotent once both controllers exist.
- This unblocks: `GetControllerState` resolving device indices to hands, `is_device_connected`, and controller `connected` state propagation to SteamVR — previously controllers were never created/connected on a real session.

## Implemented: Button/thumbstick wiring (2026-09-03)

Legacy `GetControllerState` (and any `Action::state` whose path name maps to a WinlatorXR input) now returns live values from the UDP pose data:

- `Session` gains an `input_state: RwLock<HashMap<(action, hand), InputActionEntry>>` used for `last_change_time`/`is_active`/`changed_since_last_sync` tracking.
- `Action::state()` (winlatorxr.rs:2267) resolves the action path + hand against `latest_pose_data` in production; `Action::is_active()` (winlatorxr.rs:2303) reports whether the input resolves.
- `Session::sync_actions()` commits the "previous synced" baseline so `changed_since_last_sync` only reports transitions.
- The `store_button_states`/`store_thumbstick_axes` stubs were **removed** — the live resolver (`action_value_from_input`) reads the already-parsed `latest_pose_data` directly instead.
- Layout mapping: grip(L0/R12), thumbstick click(L2/R13), x/y-a/b(L8/L9/R10/R11), trigger(L7/R18), menu(left only, index 1), thumbstick Vec2 from `LTHUMB`/`RTHUMB`. Trigger/squeeze are boolean in WinlatorXR, so float actions read as 0/1.
- 4 new unit tests cover the resolver mapping (grip, thumbstick analog+click, trigger-as-float, and right-hand menu absence).

## Test Results

```
cargo check --tests          → 0 errors (pre-existing warnings only)
cargo test --workspace       → 94 passed, 2 ignored, 0 failed (lib)
                              + 2 passed (integration: smoke_test, drop_in_openvr_api_smoke)
clippy                       → pre-existing openvr/build.rs lints only
```

## Files Modified

```
src/winlatorxr.rs            → shim (2724 lines, replaces OpenXR)
src/udp_communication.rs     → UDP protocol layer (modified)
src/compositor.rs            → FakeApi/FakeGraphicsData test fixtures + 16 compositor tests
src/graphics_backends.rs     → Fake variant for tests, removed GL
src/graphics_backends/vulkan.rs → FakeApi test double, fakexr Vulkan dispatcher
src/fakexr.rs                → NEW: in-crate test double (450 lines)
src/fakexr/vulkan.rs         → NEW: fake Vulkan dispatcher (107 lines)
src/lib.rs                   → module declarations, updated macros
Cargo.toml                   → test-cdylib dev-dep, features updated
tests/tests.rs               → smoke test (loads DLL, asserts factory)
openvr/src/convert.rs        → HandJoint Wrist variant added (pre-existing)
```
