# Task Breakdown: Resumable AI Workflow

> **Implementation Status (Aug 27 2026)**: All 39 tasks are substantially complete. `cargo check --tests` = 0 errors, `cargo test --workspace` = 81 passed + 1 smoke test. See [PROGRESS_PLAN.md](PROGRESS_PLAN.md) for current status.

## Phase Completion Summary

| Phase | Status | Notes |
|-------|--------|-------|
| Phase 1: UDP Communication | ✅ Complete | `src/udp_communication.rs` implemented |
| Phase 2: Pose Parsing | ✅ Complete | `parse_winlator_pose` + `parse_winlator_buttons` |
| Phase 3: Instance | ✅ Complete | `src/winlatorxr.rs` (not `wxr_data.rs` — see deviations) |
| Phase 4: Space and Pose Tracking | ✅ Complete | `Space::locate_with_pose` implemented; `Space::locate` is identity stub |
| Phase 5: Haptic Feedback | ✅ Complete | `send_haptic` + `apply_feedback` wired |
| Phase 6: Frame Management | ✅ Complete | `wait_frame` busy-loop, `end_frame` with SBS detection |
| Phase 7: DirectX 11 Backend | ✅ Complete | `src/graphics_backends/directx11.rs` |
| Phase 8: Remove OpenGL | ✅ Complete | `gl.rs` deleted; OpenGL types return `InvalidGraphicsBinding` |
| Phase 9: Build Config | ✅ Complete | `Cargo.toml` updated; `workspace.members = ["openvr", "macros"]` |
| Phase 10: Platform Cleanup | ✅ Complete | Windows-only; Linux refs removed |
| Phase 11: Core Integration | ⚠️ Partial | Done but with deviations (see IMPLEMENTATION_PLAN.md) |
| Phase 12: Testing | ✅ Complete | 81 lib tests + 1 smoke test passing |
| Phase 13: Documentation | ⚠️ Partial | README partially updated; PROGRESS_PLAN rewritten; IMPLEMENTATION_PLAN annotated |

### Key Deviations

- **Task 11.1**: `openxr_data.rs` → `winlatorxr.rs` (not `wxr_data.rs`)
- **Task 11.2**: `mod winlatorxr;` in lib.rs (not `mod wxr_data;`)
- **Phase 5**: `fakexr` is an in-crate `#[cfg(test)]` module (not a workspace member); root `fakexr/` directory deleted
- **Phase 7 tests**: Uses `fakexr` test double + compositor module tests + `test-cdylib` smoke test (not standalone UDP unit tests as originally planned)

---

## Task Format

This document breaks down the OpenVR -> WinlatorXR XrAPI implementation into granular, resumable feature-level tasks. Each task includes dependencies, verification steps, and recovery strategies for reliable AI/sub-agent completion.

---

## Task Format

- **Task ID**: Unique identifier for tracking
- **Name**: Brief feature-level description
- **Dependencies**: What must be complete before starting
- **Files**: Files to create or modify
- **Verification**: How to confirm completion
- **Recovery**: How to resume if interrupted
- **Complexity**: Simple (1-2 hours), Medium (2-4 hours), Complex (4-8 hours)

---

## Phase 1: Core UDP Communication Layer

### Task 1.1: Create UDP Communication Module Structure
**Dependencies**: None
**Files**: `src/udp_communication.rs` (new)
**Description**: Create the basic UDP module structure with error types and data structures for WinlatorXR communication.

**Implementation**:
1. Create `src/udp_communication.rs`
2. Define error types: `UdpError`
3. Define data structures:
   - `UdpComm` struct with receiver/transmitter sockets
   - `WinlatorPoseData` struct for received pose data
   - `WinlatorHapticData` struct for haptic feedback
4. Add basic type definitions (Quat, Vec2, Vec3) using glam

**Verification**:
```bash
cargo check
# Should show no syntax errors
Recovery: If file exists, verify struct definitions match plan. If file doesn't exist, recreate from scratch.
Complexity: Simple
Task 1.2: Implement UDP Socket Initialization
Dependencies: Task 1.1
Files: src/udp_communication.rs
Description: Implement UDP socket binding and initialization with fallback port logic.
Implementation:
1. Implement UdpComm::new() method
2. Bind receiver socket to port 7872, fallback to 7873
3. Bind transmitter socket to random port (0)
4. Set transmitter to non-blocking mode
5. Set default target IP to 127.0.0.1
6. Add error handling for binding failures
Verification:
cargo test udp_communication_initialization
# Should pass
Recovery: Check if UdpComm::new() exists. If yes, verify port binding logic. If no or incomplete, implement missing parts.
Complexity: Simple
Task 1.3: Implement UDP Send Operations
Dependencies: Task 1.2
Files: src/udp_communication.rs
Description: Implement methods to send haptic and control data to WinlatorXR.
Implementation:
1. Add send_haptic() method to UdpComm
2. Implement message formatting: <left_vibe> <right_vibe> <flag> <sbs_flag> <fov_w> <fov_h>
3. Handle socket send operations with error handling
4. Add retry logic for transient failures (max 3 retries)
Verification:
cargo test udp_send_operations
# Should pass
Recovery: If method exists, verify message format matches PROTOCOL.md. If missing, implement complete method.
Complexity: Simple
Task 1.4: Implement UDP Receive Operations
Dependencies: Task 1.2
Files: src/udp_communication.rs
Description: Implement methods to receive pose data from WinlatorXR.
Implementation:
1. Add receive_pose() method to UdpComm
2. Implement non-blocking receive with timeout
3. Return raw byte buffer for parsing
4. Handle WouldBlock errors gracefully (return None)
5. Log receive errors
Verification:
cargo test udp_receive_operations
# Should pass
Recovery: If method exists, verify error handling. If missing, implement complete method.
Complexity: Simple
Phase 2: Pose Data Parsing
Task 2.1: Implement Pose Data Parser
Dependencies: Task 1.4
Files: src/udp_communication.rs
Description: Parse space-delimited UDP pose data into structured WinlatorPoseData.
Implementation:
1. Create parse_winlator_pose() function
2. Parse space-delimited values into 28+ fields:
- Left hand: quaternion (4), thumbstick (2), position (3)
- Right hand: quaternion (4), thumbstick (2), position (3)
- HMD: quaternion (4), position (3)
- IPD, FOV H, FOV V
- Frame ID, buttons, immersive/SBS flags
3. Parse float values with error handling
4. Handle variable-length input (buttons may be missing)
5. Return Result<WinlatorPoseData, ParseError>
Verification:
cargo test pose_parsing
# Test with sample data from PROTOCOL.md
Recovery: Verify parser handles all 28+ fields correctly. Test with sample data from PROTOCOL.md line 105.
Complexity: Medium
Task 2.2: Implement Button State Parser
Dependencies: Task 2.1
Files: src/udp_communication.rs
Description: Parse the 19-button boolean string from pose data.
Implementation:
1. Create parse_winlator_buttons() function
2. Parse "T"/"F" characters into bool array of length 19
3. Handle empty or short strings gracefully
4. Return Result<[bool; 19], ParseError>
Verification:
cargo test button_parsing
# Test with "TFFFFFFFFFTTTFFFFFT" (from PROTOCOL.md)
Recovery: Verify button order matches PROTOCOL.md lines 75-95. Test with sample data.
Complexity: Simple
Task 2.3: Add Module Declaration and Exports
Dependencies: Task 2.2
Files: src/lib.rs, src/udp_communication.rs
Description: Expose UDP communication module to rest of codebase.
Implementation:
1. Add pub mod udp_communication; to src/lib.rs
2. Mark necessary items as pub in src/udp_communication.rs:
- UdpComm
- WinlatorPoseData
- WinlatorHapticData
- parse_winlator_pose()
- parse_winlator_buttons()
Verification:
cargo build
# Should compile without module resolution errors
Recovery: Check if module is declared and items are public. Fix visibility issues.
Complexity: Simple
Phase 3: WinlatorXR XrAPI Instance
Task 3.1: Create WinlatorXR Instance
Dependencies: Task 2.3
Files: src/winlatorxr.rs (modify existing)
Description: Create Instance struct and implementation for WinlatorXR.
Implementation:
1. Modify existing src/winlatorxr.rs (remove OpenXR stubs)
2. Define Instance struct with:
- system_id: SystemId
- refresh_rate: f32
- udp: Arc<UdpComm>
3. Implement Instance::new():
- Create UDP communication
- Initialize with system_id = 0, refresh_rate = 90.0
- Return Result<Self, InitError>
4. Implement Instance::now() returning XrTime
Verification:
cargo test instance_creation
# Should pass
Recovery: If file has OpenXR stubs, identify and replace with UDP-based implementation. Verify UDP initialization.
Complexity: Medium
Task 3.2: Implement Session Basic Structure
Dependencies: Task 3.1
Files: src/winlatorxr.rs
Description: Create Session struct with UDP integration.
Implementation:
1. Define Session struct with:
- system_id: SystemId
- state: SessionState
- udp: Arc<UdpComm>
- latest_pose_data: Arc<RwLock<Option<WinlatorPoseData>>>
- pose_receiver_thread: Option<JoinHandle<()>>
- shutdown_signal: Arc<AtomicBool>
2. Define SessionCreateInfo struct with system_id and graphics_binding
3. Implement Session::new() that initializes all fields but doesn't start thread yet
Verification:
cargo check
# Should compile
Recovery: Verify struct definitions match plan. Ensure UDP is wrapped in Arc for thread safety.
Complexity: Medium
Task 3.3: Implement Pose Receiver Thread
Dependencies: Task 3.2
Files: src/winlatorxr.rs
Description: Implement background thread that receives and stores pose data.
Implementation:
1. Add pose_receiver_thread() static method to Session
2. Create loop that:
- Receives UDP data (non-blocking with 1ms sleep on WouldBlock)
- Parses pose data
- Updates latest_pose_data with new data
- Logs errors
- Exits when shutdown_signal is true
3. Start thread in Session::new()
4. Handle thread join in Drop implementation
Verification:
cargo test pose_receiver_thread
# Should pass (may need mock UDP)
Recovery: Verify thread loops correctly, handles WouldBlock, updates pose data safely via RwLock.
Complexity: Medium
Task 3.4: Implement Pose Data Access
Dependencies: Task 3.3
Files: src/winlatorxr.rs
Description: Add methods to access latest pose data safely.
Implementation:
1. Implement Session::get_latest_pose() returning Option<WinlatorPoseData>
2. Use read lock on latest_pose_data
3. Return cloned data
4. Add Session::store_button_states() method stub for later use
5. Add Session::store_thumbstick_axes() method stub for later use
Verification:
cargo test pose_data_access
# Should pass
Recovery: Verify RwLock usage is correct, methods handle None case.
Complexity: Simple
Phase 4: Space and Pose Tracking
Task 4.1: Implement Space::locate() for View Space
Dependencies: Task 3.4
Files: src/winlatorxr.rs
Description: Implement pose location queries for view reference space.
Implementation:
1. Implement Space::locate() method
2. Match on space_type:
- For ReferenceSpaceType::View: return HMD pose from latest pose data
- For others: return HMD pose for now (placeholder)
3. Construct SpaceLocation with:
- pose: orientation + position from pose data
- position_valid: true
- orientation_valid: true
4. Return default identity if no pose data available
Verification:
cargo test space_locate_view
# Should pass
Recovery: Verify View space uses HMD pose from pose data correctly.
Complexity: Simple
Task 4.2: Implement Session::locate_views()
Dependencies: Task 4.1
Files: src/winlatorxr.rs
Description: Implement stereo view configuration from pose data.
Implementation:
1. Implement Session::locate_views() method
2. Get latest pose data
3. Calculate FOV in radians from FOV degrees
4. Calculate half IPD for eye offset
5. Create left view with:
- Position: HMD position + (-half_ipd, 0, 0)
- Orientation: HMD orientation
- FOV: symmetric horizontal and vertical
6. Create right view with:
- Position: HMD position + (half_ipd, 0, 0)
- Orientation: HMD orientation
- FOV: same as left
7. Return vector of views and valid flags
8. Return default views if no pose data
Verification:
cargo test locate_views
# Should pass
Recovery: Verify eye offsets use half IPD, FOV conversion is correct, default views handle None case.
Complexity: Medium
Phase 5: Haptic Feedback
Task 5.1: Implement Session::send_haptic()
Dependencies: Task 3.3
Files: src/winlatorxr.rs
Description: Send haptic feedback data to WinlatorXR.
Implementation:
1. Implement Session::send_haptic() method taking WinlatorHapticData
2. Format message: "{left_vib} {right_vib} 1 {sbs_flag} {fov_w} {fov_h}"
3. Send via UDP transmitter to port 7278
4. Handle send errors gracefully
5. Return Result<(), SessionCreationError>
Verification:
cargo test send_haptic
# Should pass (may need mock UDP)
Recovery: Verify message format matches PROTOCOL.md line 127.
Complexity: Simple
Task 5.2: Implement Action::apply_feedback()
Dependencies: Task 5.1
Files: src/winlatorxr.rs
Description: Apply haptic feedback to controller actions.
Implementation:
1. Implement Action::apply_feedback() method
2. Create WinlatorHapticData from HapticVibration
3. Call session.send_haptic()
4. Return Result<(), SessionCreationError>
Verification:
cargo test action_apply_feedback
# Should pass
Recovery: Verify conversion from HapticVibration to WinlatorHapticData is correct.
Complexity: Simple
Phase 6: Frame Management
Task 6.1: Implement Session::wait_frame()
Dependencies: Task 3.4
Files: src/winlatorxr.rs
Description: Implement frame synchronization using frame ID.
Implementation:
1. Implement Session::wait_frame() method
2. Store last seen frame ID in AtomicU8
3. Loop until new frame ID received:
- Check latest pose data
- If frame_id different from last, update and return
- Sleep 1ms between checks
4. Return (FrameWaiter, XrTime)
Verification:
cargo test wait_frame
# Should pass
Recovery: Verify loop exits when frame ID changes, handles None case.
Complexity: Medium
Task 6.2: Implement Session::end_frame()
Dependencies: Task 5.1
Files: src/winlatorxr.rs
Description: Complete frame and send SBS flag.
Implementation:
1. Implement Session::end_frame() method
2. Determine if SBS mode from composition layers
3. Create haptic data with:
- Vibration: 0.0, 0.0
- SBS flag: true if projection layer, false otherwise
- FOV: 104.5, 104.5
4. Call send_haptic()
Verification:
cargo test end_frame
# Should pass
Recovery: Verify SBS detection logic, haptic data format.
Complexity: Simple
Task 6.3: Implement Session::sync_actions()
Dependencies: Task 3.4
Files: src/winlatorxr.rs
Description: Sync action states with pose data.
Implementation:
1. Implement Session::sync_actions() method
2. Get latest pose data
3. Store button states via store_button_states()
4. Store thumbstick axes via store_thumbstick_axes()
5. Return Ok
Verification:
cargo test sync_actions
# Should pass
Recovery: Verify button and thumbstick data is extracted and stored.
Complexity: Simple
Phase 7: DirectX 11 Graphics Backend
Task 7.1: Create DirectX 11 Backend Structure
Dependencies: None (can run in parallel with Phase 1-6)
Files: src/graphics_backends/directx11.rs (new)
Description: Create DirectX 11 graphics backend with device initialization.
Implementation:
1. Create src/graphics_backends/directx11.rs
2. Add Windows API dependencies: windows::Win32::Graphics::Direct3D11::*, windows::Win32::Graphics::Dxgi::*
3. Define DirectX11Data struct with:
- device: ID3D11Device
- context: ID3D11DeviceContext
- swapchains: HashMap<u32, SwapchainData>
4. Define SwapchainData struct with:
- texture: ID3D11Texture2D
- shader_resource_view: ID3D11ShaderResourceView
- render_target_view: ID3D11RenderTargetView
- width: u32, height: u32
5. Define DirectX11 marker struct
Verification:
cargo check
# Should compile
Recovery: Verify struct definitions, Windows API imports.
Complexity: Medium
Task 7.2: Implement DirectX 11 Device Creation
Dependencies: Task 7.1
Files: src/graphics_backends/directx11.rs
Description: Initialize Direct3D 11 device with hardware driver.
Implementation:
1. Implement DirectX11Data::new() method
2. Call D3D11CreateDevice with:
- Driver: hardware
- Flags: DEBUG
- SDK version: D3D11_SDK_VERSION
3. Store device and context
4. Initialize empty swapchain HashMap
5. Return Result<Self, GraphicsError>
Verification:
cargo test directx11_device_creation
# Should pass (may skip on non-Windows)
Recovery: Verify D3D11CreateDevice parameters, error handling.
Complexity: Medium
Task 7.3: Implement DirectX 11 Swapchain Creation
Dependencies: Task 7.2
Files: src/graphics_backends/directx11.rs
Description: Create Direct3D 11 swapchain textures.
Implementation:
1. Implement DirectX11Data::create_swapchain() method
2. Create D3D11_TEXTURE2D_DESC from SwapchainCreateInfo
3. Create texture with appropriate bind flags
4. Create shader resource view
5. Create render target view
6. Store in swapchain HashMap
7. Return swapchain ID
Verification:
cargo test directx11_swapchain_creation
# Should pass (may skip on non-Windows)
Recovery: Verify texture description, view creation logic.
Complexity: Medium
Task 7.4: Implement DirectX 11 Texture Copy
Dependencies: Task 7.3
Files: src/graphics_backends/directx11.rs
Description: Copy OpenVR texture to DirectX 11 swapchain.
Implementation:
1. Implement DirectX11Data::copy_texture_to_swapchain() method
2. Get source texture from OpenVR handle
3. Calculate destination bounds from texture bounds
4. Create D3D11_BOX for region copy
5. Call CopySubresourceRegion
6. Return extent of copied region
Verification:
cargo test directx11_texture_copy
# Should pass (may skip on non-Windows)
Recovery: Verify region calculation, CopySubresourceRegion parameters.
Complexity: Medium
Task 7.5: Implement DirectX 11 Swapchain Info
Dependencies: Task 7.2
Files: src/graphics_backends/directx11.rs
Description: Determine swapchain creation parameters from texture.
Implementation:
1. Implement DirectX11Data::swapchain_info_for_texture() method
2. Get texture extent via get_texture_extent()
3. Create SwapchainCreateInfo with:
- Width and height from extent
- Format: DXGI_FORMAT_R8G8B8A8_UNORM
- Sample count: 1
Verification:
cargo test directx11_swapchain_info
# Should pass (may skip on non-Windows)
Recovery: Verify extent extraction, format constant.
Complexity: Simple
Task 7.6: Implement DirectX 11 Graphics Trait
Dependencies: Task 7.5
Files: src/graphics_backends/directx11.rs
Description: Implement GraphicsBackend trait for DirectX 11.
Implementation:
1. Implement GraphicsBackend trait for DirectX11Data
2. Implement Graphics trait for DirectX11:
- type SwapchainInfo = SwapchainCreateInfo<DirectX11>
- type SwapchainImageData = ID3D11Texture2D
3. Wire up all methods to implementations from Tasks 7.2-7.5
Verification:
cargo build
# Should compile
Recovery: Verify trait implementations match expected signatures.
Complexity: Medium
Phase 8: Remove OpenGL Backend
Task 8.1: Delete OpenGL Backend File
Dependencies: None (can run at any time after Phase 7 starts)
Files: src/graphics_backends/gl.rs (delete)
Description: Remove OpenGL graphics backend file.
Implementation:
1. Delete src/graphics_backends/gl.rs
2. Verify deletion
Verification:
test -f src/graphics_backends/gl.rs && echo "File still exists" || echo "File deleted"
# Should print "File deleted"
Recovery: If file deleted, verify no compilation errors remain. If file exists, delete it.
Complexity: Simple
Task 8.2: Update Graphics Backends Module
Dependencies: Task 8.1, Task 7.6
Files: src/graphics_backends.rs
Description: Remove GL references and update backend selection.
Implementation:
1. Remove gl module declaration
2. Update SupportedBackend enum to:
- Remove GL variant
- Keep DirectX11, Vulkan, Fake (test)
3. Update supported_backends_enum! macro
4. Update select_preferred_backend() to try DirectX 11 first, then Vulkan
5. Update TempBackendData enum to remove GL variant
Verification:
cargo build
# Should compile
Recovery: Verify GL references are removed, DirectX 11 is primary choice.
Complexity: Medium
Phase 9: Update Build Configuration
Task 9.1: Update Cargo.toml Dependencies
Dependencies: None (can run at any time)
Files: Cargo.toml
Description: Update dependencies for Windows-only build.
Implementation:
1. Remove OpenGL-related dependencies
2. Remove OpenXR dependencies
3. Remove Linux-specific dependencies
4. Add Windows-specific dependencies:
- windows = { version = "0.59", features = ["Win32_Foundation", "Win32_System_LibraryLoader", "Win32_Graphics_Direct3D11", "Win32_Graphics_Dxgi"] }
5. Add/keep: socket2, crossbeam-channel, parking_lot, libloading
6. Update description to "OpenVR -> WinlatorXR XrAPI bridge (Windows only)"
Verification:
cargo build
# Should compile and resolve dependencies
Recovery: Verify dependencies match plan, no GL/OpenXR references remain.
Complexity: Simple
Task 9.2: Update build.rs
Dependencies: None (can run at any time)
Files: build.rs
Description: Update build script for Windows linking.
Implementation:
1. Add #[cfg(windows)] block
2. Add DirectX 11 link libraries: d3d11, dxgi
3. Remove OpenXR linking (comment out or delete)
4. Ensure no OpenGL linking
Verification:
cargo build
# Should compile
Recovery: Verify only DirectX libraries are linked on Windows.
Complexity: Simple
Phase 10: Remove Platform-Specific Code
Task 10.1: Replace Linux App Name Detection
Dependencies: None (can run at any time)
Files: src/winlatorxr.rs (and other files with /proc/self/exe)
Description: Replace Linux-specific app name detection with Windows version.
Implementation:
1. Find all uses of /proc/self/exe (grep)
2. Replace with Windows version:
pub fn get_app_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}
Verification:
grep -r "/proc/self/exe" src/
# Should return no results
Recovery: Verify all /proc/self/exe references are replaced.
Complexity: Simple
Task 10.2: Remove #cfg(unix) Blocks
Dependencies: None (can run at any time)
Files: All files in src/
Description: Remove all Unix-specific conditional compilation.
Implementation:
1. Find all #[cfg(unix)] blocks (grep)
2. Delete entire blocks (they contain Linux-only code)
3. Find all #[cfg(windows)] blocks and remove the attribute (code now always runs on Windows)
Verification:
grep -r "#\[cfg(unix)\]" src/
grep -r "#\[cfg(windows)\]" src/
# Should return no results
Recovery: Verify no Unix blocks remain, Windows blocks are flattened.
Complexity: Simple
Task 10.3: Remove GLX/X11 References
Dependencies: None (can run at any time)
Files: All files in src/
Description: Remove any GLX or X11 windowing system references.
Implementation:
1. Find all GLX/X11 references (grep)
2. Remove or replace with Windows equivalents if needed
Verification:
grep -ri "glx\|x11" src/
# Should return no results
Recovery: Verify no GLX/X11 references remain.
Complexity: Simple
Phase 11: Update Core Integration
Task 11.1: Rename and Update wxr_data.rs
Dependencies: Task 3.1
Files: src/openxr_data.rs -> src/wxr_data.rs
Description: Rename and refactor OpenXR data file to WinlatorXR.
Implementation:
1. Rename src/openxr_data.rs to src/wxr_data.rs (or create new and delete old)
2. Rename RealOpenXRData to RealWxrData
3. Rename OpenXRData to WxrData
4. Update WxrData struct to use Instance from winlatorxr
5. Update WxrData::new() to use Instance::new()
6. Remove OpenXR-specific methods (poll_events, restart_session)
7. Set enabled_extensions to disable visibility_mask, display_refresh_rate, hand_tracking
Verification:
cargo build
# Should compile
Recovery: Verify all OpenXR references replaced with WinlatorXR equivalents.
Complexity: Medium
Task 11.2: Update lib.rs Module Declarations
Dependencies: Task 11.1
Files: src/lib.rs
Description: Update module declarations for new structure.
Implementation:
1. Remove or comment out mod openxr_data;
2. Add mod wxr_data;
3. Keep mod winlatorxr;
4. Update any imports from openxr_data to wxr_data
Verification:
cargo build
# Should compile
Recovery: Verify module declarations, imports are correct.
Complexity: Simple
Phase 12: Testing and Validation
Task 12.1: Create Unit Tests for UDP Communication
Dependencies: Task 2.3
Files: src/udp_communication.rs (add tests module)
Description: Add comprehensive unit tests for UDP module.
Implementation:
1. Add #[cfg(test)] mod tests to src/udp_communication.rs
2. Add test: test_udp_communication_initialization
3. Add test: test_winlator_pose_parsing (use sample from PROTOCOL.md)
4. Add test: test_button_parsing
5. Add test: test_haptic_data_formatting
Verification:
cargo test udp_communication
# All tests should pass
Recovery: Verify tests cover all public functions, handle edge cases.
Complexity: Medium
Task 12.2: Create Unit Tests for WinlatorXR
Dependencies: Task 6.3
Files: src/winlatorxr.rs (add tests module)
Description: Add comprehensive unit tests for WinlatorXR module.
Implementation:
1. Add #[cfg(test)] mod tests to src/winlatorxr.rs
2. Add test: test_instance_creation
3. Add test: test_session_creation
4. Add test: test_space_locate_view
5. Add test: test_locate_views
6. Add test: test_send_haptic
7. Add test: test_wait_frame
8. Add test: test_end_frame
9. Add test: test_sync_actions
Verification:
cargo test winlatorxr
# All tests should pass
Recovery: Verify tests cover all major functions, use mocks where needed.
Complexity: Medium
Task 12.3: Full Compilation Test
Dependencies: Task 11.2, Task 7.6, Task 8.2
Files: All source files
Description: Verify entire project compiles without errors.
Implementation:
1. Run cargo clean
2. Run cargo build --release
3. Fix any compilation errors
4. Repeat until clean build succeeds
Verification:
cargo clean && cargo build --release
# Should complete successfully with no errors
Recovery: Fix each error individually, re-run compilation after each fix.
Complexity: Simple
Task 12.4: Full Test Suite
Dependencies: Task 12.3
Files: All test files
Description: Run all unit tests to verify functionality.
Implementation:
1. Run cargo test --all
2. Review test failures
3. Fix failing tests
4. Repeat until all tests pass
Verification:
cargo test --all
# All tests should pass
Recovery: Fix each failing test, re-run suite after each fix.
Complexity: Medium
Task 12.5: Runtime Integration Test
Dependencies: Task 12.4
Files: src/lib.rs (add integration test)
Description: Test VR client core factory initialization.
Implementation:
1. Add integration test to verify VRClientCoreFactory returns non-null
2. Verify return code is 0
3. Test that OpenVR calls are properly routed to WinlatorXR
Verification:
cargo test --test integration
# Integration tests should pass
Recovery: Fix integration test failures, verify OpenVR -> WinlatorXR routing.
Complexity: Medium
Phase 13: Documentation Updates
Task 13.1: Update README.md
Dependencies: Task 12.5
Files: README.md
Description: Update README to reflect Windows-only WinlatorXR implementation.
Implementation:
1. Update title to "OpenVR to WinlatorXR Bridge"
2. Update description to mention Windows-only
3. Update requirements section:
- Windows OS
- WinlatorXR device
- WinlatorXR container
4. Add UDP protocol section (ports, formats)
5. Update installation instructions
6. Add supported features section
7. Add environment variables section
8. Update building and testing sections
9. Update credits section
Verification:
# Review README.md for completeness
Recovery: Compare with IMPLEMENTATION_PLAN.md README section, ensure all content is included.
Complexity: Simple
Task Dependency Graph
Phase 1: UDP Communication
  ââ Task 1.1 (UDP structure)
  ââ Task 1.2 (UDP init) -> depends on 1.1
  ââ Task 1.3 (UDP send) -> depends on 1.2
  ââ Task 1.4 (UDP recv) -> depends on 1.2

Phase 2: Pose Parsing
  ââ Task 2.1 (pose parser) -> depends on 1.4
  ââ Task 2.2 (button parser) -> depends on 2.1
  ââ Task 2.3 (module exports) -> depends on 2.2

Phase 3: WinlatorXR Instance
  ââ Task 3.1 (Instance) -> depends on 2.3
  ââ Task 3.2 (Session struct) -> depends on 3.1
  ââ Task 3.3 (pose thread) -> depends on 3.2
  ââ Task 3.4 (pose access) -> depends on 3.3

Phase 4: Space Tracking
  ââ Task 4.1 (Space::locate) -> depends on 3.4
  ââ Task 4.2 (locate_views) -> depends on 4.1

Phase 5: Haptics
  ââ Task 5.1 (send_haptic) -> depends on 3.3
  ââ Task 5.2 (apply_feedback) -> depends on 5.1

Phase 6: Frame Management
  ââ Task 6.1 (wait_frame) -> depends on 3.4
  ââ Task 6.2 (end_frame) -> depends on 5.1
  ââ Task 6.3 (sync_actions) -> depends on 3.4

Phase 7: DirectX 11 (parallel to 1-6)
  ââ Task 7.1 (DX11 structure)
  ââ Task 7.2 (device) -> depends on 7.1
  ââ Task 7.3 (swapchain) -> depends on 7.2
  ââ Task 7.4 (texture copy) -> depends on 7.3
  ââ Task 7.5 (swapchain info) -> depends on 7.2
  ââ Task 7.6 (traits) -> depends on 7.5

Phase 8: Remove OpenGL
  ââ Task 8.1 (delete GL file)
  ââ Task 8.2 (update module) -> depends on 8.1, 7.6

Phase 9: Build Config (can run anytime)
  ââ Task 9.1 (Cargo.toml)
  ââ Task 9.2 (build.rs)

Phase 10: Platform Cleanup (can run anytime)
  ââ Task 10.1 (app name)
  ââ Task 10.2 (cfg blocks)
  ââ Task 10.3 (GLX/X11)

Phase 11: Core Integration
  ââ Task 11.1 (wxr_data.rs) -> depends on 3.1
  ââ Task 11.2 (lib.rs) -> depends on 11.1

Phase 12: Testing
  ââ Task 12.1 (UDP tests) -> depends on 2.3
  ââ Task 12.2 (WinlatorXR tests) -> depends on 6.3
  ââ Task 12.3 (compile) -> depends on 11.2, 7.6, 8.2
  ââ Task 12.4 (all tests) -> depends on 12.3
  ââ Task 12.5 (integration) -> depends on 12.4

Phase 13: Documentation
  ââ Task 13.1 (README) -> depends on 12.5
Execution Order
For reliable resumable execution, follow this order:
1. Sequential Tasks (1.1 -> 1.2 -> 1.3 -> 1.4 -> 2.1 -> 2.2 -> 2.3 -> 3.1 -> 3.2 -> 3.3 -> 3.4 -> 4.1 -> 4.2 -> 5.1 -> 5.2 -> 6.1 -> 6.2 -> 6.3)
2. Parallel Branch (7.1 -> 7.2 -> 7.3 -> 7.4 -> 7.5 -> 7.6) - can run anytime after Task 1.1
3. Cleanup Tasks (8.1 -> 8.2, 9.1 -> 9.2, 10.1 -> 10.2 -> 10.3) - can run anytime after Phase 7 starts
4. Integration Tasks (11.1 -> 11.2) - after Phase 3 and Phase 7
5. Testing Tasks (12.1 after 2.3, 12.2 after 6.3, 12.3 after all code complete, 12.4 after 12.3, 12.5 after 12.4)
6. Documentation (13.1) - after all testing passes
Total Tasks: 39
- Simple: 19 tasks (1-2 hours each)
- Medium: 18 tasks (2-4 hours each)
- Complex: 2 tasks (4-8 hours each)
Estimated Total Time: 70-110 hours
Recovery Strategy for Each Task
When resuming after interruption:
1. Read current state: Use read tool to examine file contents
2. Compare with plan: Check if implementation matches task requirements
3. Identify gaps: What's missing or incomplete?
4. Continue or restart: If substantial progress exists, continue. If minimal progress or incorrect approach, restart task
5. Verify after changes: Run verification steps
6. Move to next task: Only after current task passes verification
When to restart task:
- File doesn't exist and should
- File exists but has wrong structure
- Implementation has fundamental errors
- Verification fails repeatedly
When to continue task:
- Partial implementation exists
- Structure is correct
- Only minor details missing
Notes
- Some tasks can run in parallel (see dependency graph)
- Phase 7 (DirectX 11) can run concurrently with Phases 1-6
- Cleanup tasks (8, 9, 10) can run anytime after Phase 7 starts
- Always run verification steps after each task
- If compilation fails, fix before proceeding
- If tests fail, fix before proceeding
- Document any deviations from plan in task completion notes