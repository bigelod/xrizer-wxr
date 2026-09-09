# xrizer-wxr On-Device Deployment Guide

How to build `xrizer_wxr.dll` and deploy it inside a WinlatorXR container so a real OpenVR game gets 6DOF tracking, controller input, haptics, and SBS/AER rendering from the standalone VR device.

The wire protocol this DLL speaks is documented in `docs/PROTOCOL.md`; what is implemented vs. not is in `docs/PROGRESS_PLAN.md`.

## What this DLL is

`xrizer-wxr` is a Rust `cdylib` that reimplements the OpenVR API on top of the WinlatorXR XrAPI (UDP). It contains two complete OpenVR loading paths in a single binary:

- **Runtime path** — exports `VRClientCoreFactory` / `HmdSystemFactory` (the way OpenVR runtimes are loaded).
- **Drop-in client path** — exports `VR_InitInternal`, `VR_ShutdownInternal`, `VR_GetGenericInterface`, `VR_IsHmdPresent`, `VR_GetStringForHmdError`, `VR_GetInitTokenAndVersion`, so games that link `openvr_api.dll` directly work.

Deploying the DLL as `openvr_api.dll` covers the drop-in path; the runtime path adds SteamVR-style discovery.

## Build

Do this on your development machine (the Winlator container does not need to build anything).

Prerequisites: a Rust toolchain (`rust-version = 1.85`, edition 2024) and a Windows MSVC target with the MSVC build tools (the crate produces a native `.dll` and uses the `windows` crate / DX11).

```powershell
# release build (recommended for on-device use)
cargo build --release

# dev build (faster, slower runtime)
cargo build

# optional: enable Tracy instrumentation
cargo build --release --features tracing
```

Output:

```
target/release/xrizer_wxr.dll      (~10 MB)
target/debug/xrizer_wxr.dll        (dev build)
```

Run the test suite before deploying if you want a sanity check:

```powershell
cargo test --workspace   # 94 lib + 2 integration tests
```

> Note the repo README's `cargo xbuild` refers to the original xrizer project; this fork builds with plain `cargo build`.

## Deploy (WinlatorXR container)

The DLL is an x86_64 Windows DLL. Copy it into the game's directory inside the Winlator container. Two ways to arrange it:

### Option 1 — Drop-in `openvr_api.dll` (preferred, simplest)

Games load `openvr_api.dll` from their own directory (or from Wine's search path) when SteamVR is not hooked. Rename the file and drop it beside the main game executable:

```
<game dir>\openvr_api.dll        <- rename of xrizer_wxr.dll
<game dir>\Game.exe
```

No registry, `openvrpaths.vrpath`, or environment variables needed. This is the setup WinlatorXR should ship per game shortcut.

### Option 2 — Runtime directory

Copy it into a runtime layout and let the game discover it:

```
<runtime dir>\bin\win64\openvr_api.dll
```

Then point the game at that directory with either:

- an `openvrpaths.vrpath` `runtime` entry at `$XDG_CONFIG_HOME/openvr/openvrpaths.vrpath`:
  ```json
  { "version": 1, "runtime": ["<runtime dir>"] }
  ```
- or the `VR_OVERRIDE` environment variable — **a directory, not a DLL path**:
  ```
  VR_OVERRIDE=<runtime dir>
  ```

Inside the container, paths under `/usr` are reachable at `/run/host/usr`; if the game runs under a Steam Linux Runtime container, expose extra paths with `PRESSURE_VESSEL_FILESYSTEMS_RW`.

## Wire protocol quick reference

- **Receive (Rx)** — the DLL binds `127.0.0.1:7872` (fallback `7873`) and reads 6DOF + controller + FOV data on a background thread.
- **Send (Tx)** — the DLL sends CSV to `127.0.0.1:7278`:
  - Startup handshake `0 0 1 0 104.5 104.5` (VR immersive, monocular) is sent the moment the real session is created, before the first frame.
  - Per-frame `end_frame` sends keepalives that double as retries, with `L_VIBE,R_VIBE,VR,SBS,FOV_W,FOV_H`.
- WinlatorXR only begins streaming data after it receives at least one Tx packet — the startup handshake handles this automatically.

## Logging

Enable debug logging by setting `RUST_LOG=debug` on the game process. Logs go to stderr and to:

```
$XDG_STATE_HOME/xrizer/xrizer.txt        (or)
$HOME/.local/state/xrizer/xrizer.txt
```

On startup you should see `Initializing XRizer version <x.y.z>` followed by ClientCore creation lines. The `openvr_calls` RUST_LOG target logs every OpenVR function call.

## Environment variables

- `RUST_LOG` — logging level (see `docs`); `debug` is the useful level on-device.
- `XRIZER_INTERACTION_PROFILE` — override the default `/interaction_profiles/oculus/touch_controller` profile.
- `XRIZER_CUSTOM_BINDINGS_DIR` — directory xrizer will search for controller bindings files.
- `XRIZER_TRACKER_SERIALS` — semicolon-separated serials for generic FBT trackers.

## What is verified vs. not

Verified (off-device): crate builds; `cargo test --workspace` is green (94 lib + 2 integration including a full init → interface fetch → shutdown sequence against the real DLL); release export table contains all 8 symbols; clippy clean.

Not yet verified (needs a real WinlatorXR device):

- DX11 texture pipeline producing visible frames (`copy_texture_to_swapchain` visibility).
- Skeletal joint orientation/placement from the synthetic hand tracker.
- Per-profile binding behavior and controller connection on real hardware.
- Whether Valve's client honors `VR_OVERRIDE`/`openvrpaths.vrpath` inside the Winlator container (drop-in mode avoids this question entirely).

## Troubleshooting

| Symptom | Likely cause / fix |
|---|---|
| No tracking data, HMD pose stuck at identity | No Tx packet received by XrAPI. Confirm the startup handshake fired: check the log for warnings from `SessionData::new`; ensure port `7278` is reachable in the container. |
| Game fails to start, "openvr_api.dll" not found | DLL must be named `openvr_api.dll` and be beside the executable (or on the Wine/Proton search path). |
| Crash during startup | Panic landmines were removed, but if it still crashes, capture `RUST_LOG=debug` output and the `xrizer.txt` log; the panic hook logs a backtrace. |
| VR runtime not found (runtime mode) | `VR_OVERRIDE` must be a **directory**, and an existing valid `openvrpaths.vrpath` is required. Prefer drop-in mode. |
| Haptics not felt | Tx haptic values only send when the game triggers haptics; XrAPI decays them to 0 on its own, so a single value may be too brief. |