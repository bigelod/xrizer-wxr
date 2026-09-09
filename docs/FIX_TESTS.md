# FIX_TESTS.md

Status of the test suite for the `xrizer-wxr` (OpenVR → WinlatorXR) port, and what
needs to happen to make `cargo test` compile and pass again.

## Current status

The test suite is **green** as of 2026-08-13:

| Command | Result |
|---|---|
| `cargo check` (lib) | 0 errors (pre-existing warnings only) |
| `cargo check --tests` | compiles |
| `cargo test` | **81 passed / 2 ignored** (lib) + **1 passed** (`tests/tests.rs` smoke test) |

### What changed to get here (see items below)

- `fakexr` was rebuilt as an in-crate test double (`src/fakexr.rs`, `src/fakexr/`)
  providing `UserPath`, `ActionState`, `set_action_state`, `LeftHand`/`RightHand`,
  and a fake Vulkan dispatcher (`fakexr/src/vulkan.rs`).
- Shim aliases added in `winlatorxr.rs`: `pub type Haptic = HapticTy`,
  `sys::RawAction`, `Action::as_raw`, `Path: Deref`.
- `src/openxr_data.rs` was renamed/replaced by the `winlatorxr` module; `FakeCompositor`
  moved into `compositor.rs`'s test module.
- `compositor.rs` `FakeApi`/`FakeGraphicsData` were rewritten against the shim
  `Graphics`/`GraphicsBackend` traits (item 8).
- `tests/tests.rs` smoke test now uses the `test-cdylib = "1.1.0"` dev-dependency
  (item 7).
- Session lifecycle semantics (should_render/synchronized states) implemented for
  the fake compositor (`FrameWaiter::wait`, `FrameStream::end`, `poll_events`,
  `SessionData::create_swapchain`, `SessionData::check_format`, `temp_vulkan`).
- `vulkan_legacy_*` shim functions return the fake `VK_foo`/`VK_bar` extension list
  under `#[cfg(test)]`; `GetVulkan*ExtensionsRequired` joins with a space per OpenVR.

## What was done (historical record)

The library itself is ported and builds. The test-only code originally targeted the
old `openxr`-crate API and several helper crates that were dropped during the port,
so none of it compiled. The root causes and fixes that resolved it are recorded
below for reference.

## Error distribution by file

```
117  src/input/custom_bindings.rs
 86  src/input/tests.rs
 58  src/input/legacy.rs
 48  src/compositor.rs
  4  src/input/devices.rs
  4  src/rendermodels.rs
  3  src/input/profiles/simple_controller.rs
  2  src/input/profiles/knuckles.rs
  1  src/input/profiles/vive_controller.rs
  1  src/input/profiles/vive_focus3.rs
  1  src/input/profiles/oculus_touch.rs
  1  src/graphics_backends/vulkan.rs
  1  src/graphics_backends.rs
  1  src/overlay.rs
  1  tests/tests.rs
```

Most failures trace back to a small set of root causes. Fix those first and the
bulk of the errors collapse.

## Root causes and fixes

### 1. `fakexr` is not a dependency and does not compile in this workspace

The input tests do `use fakexr::UserPath::*`, `fakexr::ActionState`,
`fakexr::set_action_state`, etc. but:

- `Cargo.toml` has **no `[dev-dependencies]`** — add `fakexr = { path = "fakexr" }`.
- `fakexr` itself references crates that are not declared in its `Cargo.toml` and
  not part of this workspace: `openxr_sys`, `openxr_mndx_xdev_space`
  (`fakexr/src/lib.rs:8`, `fakexr/src/monado_xdev.rs:4-5`, `fakexr/src/vulkan.rs:2`).
  These were OpenXR-crate deps from the original `xrizer`; the port removed OpenXR.
- The API the tests need is not exported by `fakexr` either. `fakexr/src/lib.rs`
  only exports `monado_xdev::add_trackers` and `mod vulkan`. The tests need
  `UserPath`, `ActionState`, `set_action_state`, and hand constants.

**Fix:** either (a) rebuild `fakexr` as a thin test double over the shim
(`winlatorxr` types) that provides `UserPath`, `ActionState`, `set_action_state`,
`LeftHand`/`RightHand`, or (b) delete the `fakexr` dependency and add an in-crate
test helper module that provides the same surface. Option (b) is less work and
removes the dead `openxr_sys` linkage. See item 2 for the hand constants.

### 2. `LeftHand` / `RightHand` constants no longer exist (~50 uses)

`custom_bindings.rs`, `legacy.rs`, `tests.rs` all reference `LeftHand`/`RightHand`.
Nothing defines them anymore. Map them to the shim enum `winlatorxr::Hand`
(`Hand::Left`, `Hand::Right`, `src/winlatorxr.rs:2076`). Add the constants in the
new test-helper module (item 1) as `use winlatorxr::Hand::{Left as LeftHand, Right as RightHand};`.

### 3. `crate::openxr_data` module was renamed to `winlatorxr`

`src/input/tests.rs:10`: `use crate::openxr_data::{FakeCompositor, Hand, OpenXrData}`.

- `Hand` and `OpenXrData` exist in `winlatorxr` → update the path.
- `FakeCompositor` does **not** exist. `Input<C: winlatorxr::Compositor>` requires a
  `Compositor` implementor (`src/winlatorxr.rs` `pub trait Compositor`). A fake
  `Compositor` must be recreated for tests (the old one lived in the removed
  `openxr_data` module). See also item 8 — `compositor.rs` has a
  `FakeGraphicsData`/`FakeApi` test module that pairs with this.

### 4. Missing shim type names that tests expect

- `xr::Haptic` — the shim only has `HapticTy` (`src/winlatorxr.rs:17`). Add
  `pub type Haptic = HapticTy;` (and `sys::Haptic`). Fixes `xr::Haptic` errors in
  `profiles/*.rs` and `tests.rs`, and the `HapticTy: input::tests::ActionType`
  bound in `knuckles.rs:231` once `tests.rs:67` resolves.
- `xr::sys::Action` / `xr::sys::Session` "missing generics" (`tests.rs:48,54,109,257,279,359`).
  The openxr `sys::Action` was a non-generic raw handle. The shim `sys::Action<T>`
  is generic (`src/winlatorxr.rs:2238`). Provide a raw, non-generic handle type in
  `sys` (e.g. `pub type RawAction = u64`) and use it as the return type of
  `ActionType::get_xr_action`, or give `Action::as_raw()` a concrete return type
  (item 5).
- `xr::Vector2f` exists (`src/winlatorxr.rs:25`) — OK.

### 5. `Action::as_raw()` is missing (~30 uses)

Tests call `.as_raw()` on `&Action<T>`/`Action<T>`. The shim `Action<T>`
(`src/winlatorxr.rs:1931`) has no `as_raw`. Either:

- add `pub fn as_raw(&self) -> RawAction` to the shim (needs a raw handle field or
  a stub), or
- rewrite the test helpers to read the action fields directly.

### 6. `Path` is not dereferenceable

`custom_bindings.rs:1240` does `*path`. The shim `Path(pub u64)`
(`src/winlatorxr.rs:1925`) has no `Deref`. The old openxr `Path` deref'd to `str`.
Add `impl Deref for Path` returning a `&str`, or change the test to use `path.0`.

### 7. `test_cdylib` helper crate is missing

`tests/tests.rs:7` uses `test_cdylib::build_current_project()` — a helper that
builds the current crate as a `cdylib` and returns its path. It is not a workspace
member and no `test_cdylib/` dir exists. Either recreate the crate
(workspace member that shells out to `cargo build` and returns the `dll` path) or
rewrite the smoke test to build the cdylib itself. The rest of `tests/tests.rs`
(load `VRClientCoreFactory`, check `IVRClientCore_003` is non-null) is still valid.

### 8. `compositor.rs` `#[cfg(test)]` module is still on the old `openxr` API (48 errors)

The `FakeApi: xr::Graphics` impl and `FakeGraphicsData: GraphicsBackend` impl
(`src/compositor.rs:1430-1500`) reference:

- `xr::vulkan::{Requirements, SessionCreateInfo}` — no `vulkan` module in the shim.
- `openxr::*`, `opencrate`, `openResult` — removed crates.
- Old `Graphics` trait members `raise_format`, `lower_format`, `requirements`,
  `create_session`, `enumerate_swapchain_images`, `type Requirements` — the shim
  `Graphics` trait now only has 4 associated types: `SessionCreateInfo`, `Format`,
  `SwapchainImage`, `SwapchainCreateInfo` (`src/winlatorxr.rs`).
- `xr::SwapchainCreateInfo { ... }` struct literal — fails because
  `SwapchainCreateInfo::_phantom` is private; use `SwapchainCreateInfo::new(...)`.
- `Extent2Di` import is private (`graphics_backends.rs:9` re-exports it, make it
  `pub` or import from `directx11`).
- `FakeGraphicsData`'s `GraphicsBackend` impl is missing the new
  `swapchain_images_from_handles` method.
- `fakexr` references in this module (`compositor.rs:1608-1998`).

This module needs to be rewritten against the shim `Graphics`/`GraphicsBackend`
traits (see how `vulkan.rs`/`directx11.rs` implement them) and the new
`swapchain_images_from_handles` helper.

### 9. `legacy.rs` const-offset assertion panics at compile time

`src/input/legacy.rs:455` asserts `offset_of!(vr::VREvent_t, eventType) ==
offset_of!(MyEvent, ty)` (and `trackedDeviceIndex`, `eventAgeSeconds`, `data`) in
a `const _`. The assertion on `data` fails — `MyEvent` no longer matches the
`VREvent_t` layout. This is partly because the 0.9.12 header field `eventType` was
changed to `uint32_t` (`openvr/headers/openvr-0.9.12.h`) and the shim structs
were reworked. Recheck `MyEvent`'s definition against the current `VREvent_t`
layout and fix the struct or drop the failing assertion.

### 10. Misc type mismatches in test code

- `input/tests.rs`: `Vec3` vs `Vector3f`, `Quat` vs `Quaternionf`
  (`tests.rs:531,536,708,713,718,723,754,759,764,769,783,789`). The shim uses glam
  `Vec3`/`Quat` — replace the OpenXR structs with glam.
- `rendermodels.rs:963-964`: `state.uProperties & visible` where
  `EVRComponentProperty::IsVisible.0` is `i32` but `uProperties` is `u32` — cast
  to `u32` (mirror the earlier fix at `rendermodels.rs:223`).
- `overlay.rs:570`: `<&mut ... as TryFrom<...>>::Error` doesn't impl `Display` —
  add `Error: std::fmt::Display` to the conversion bound used in the test path.
- `legacy.rs:742-770`: bare `xr::` paths in tests — should be
  `crate::winlatorxr as xr` (module alias missing in the test module).

## Suggested order of work

1. Add `[dev-dependencies]` and either fix `fakexr` or replace it with an in-crate
   test helper (item 1/2/3/6). This clears the majority of `custom_bindings.rs`,
   `tests.rs`, `legacy.rs`, and `profiles/*` errors.
2. Add the small shim aliases: `pub type Haptic = HapticTy`, `sys::RawAction`,
   `Action::as_raw`, `Path: Deref`. (Non-test, additive, safe.)
3. Rewrite `compositor.rs` `FakeApi`/`FakeGraphicsData` against the shim `Graphics`
   trait (item 8).
4. Recreate `test_cdylib` or inline the smoke-test build (item 7).
5. Fix the remaining scalar/type mismatches (item 10) and the `legacy.rs` const
   assertions (item 9).
6. Verify: `cargo check --tests --message-format short`, then `cargo test`.
