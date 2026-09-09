#![allow(clippy::new_without_default, clippy::missing_safety_doc)]
use glam::f32::{Quat, Vec2, Vec3};
use log::warn;
use openvr as vr;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::sync::{
    OnceLock, RwLock,
    atomic::{AtomicI64, AtomicBool, Ordering},
};
use std::thread;

use crate::clientcore::Injected;
use crate::clientcore::Injector;
use crate::graphics_backends::{Extent2Di, GraphicsBackend};

pub trait ActionTy {}

pub struct HapticTy;

impl ActionTy for bool {}
impl ActionTy for f32 {}
impl ActionTy for XrVector2f {}
impl ActionTy for HapticTy {}

pub type Posef = XrPosef;
pub type Vector2f = XrVector2f;
pub type Haptic = HapticTy;

pub use std::result::Result;

pub type XrResult<T> = std::result::Result<T, SessionCreationError>;

pub struct AnyGraphics;

pub use std::time::Duration;
pub const MIN_HAPTIC: Duration = Duration::from_micros(1);
pub const INFINITE: Duration = Duration::MAX;

pub trait Graphics {
    type SessionCreateInfo;
    type Format;
    type SwapchainImage;
    type SwapchainCreateInfo;
}

impl Graphics for ash::vk::Format {
    type SessionCreateInfo = VulkanSwapchainCreateInfoKHR;
    type Format = ash::vk::Format;
    type SwapchainImage = SwapchainImageVulkanKHR;
    type SwapchainCreateInfo = VulkanSwapchainCreateInfoKHR;
}

impl Graphics for u32 {
    type SessionCreateInfo = OpenGLSessionCreateInfo;
    type Format = u32;
    type SwapchainImage = SwapchainImageOpenGLKHR;
    type SwapchainCreateInfo = OpenGLSwapchainCreateInfoKHR;
}

#[derive(Debug, Clone, Copy)]
pub struct Vulkan;

impl Graphics for Vulkan {
    type SessionCreateInfo = VulkanSessionCreateInfo;
    type Format = u64;
    type SwapchainImage = SwapchainImageVulkanKHR;
    type SwapchainCreateInfo = VulkanSwapchainCreateInfoKHR;
}

impl Graphics for AnyGraphics {
    type SessionCreateInfo = ();
    type Format = ();
    type SwapchainImage = ();
    type SwapchainCreateInfo = ();
}

pub use crate::graphics_backends::DirectX11;

pub trait Compositor: openvr::InterfaceImpl {
    fn post_session_restart(
        &self,
        session: &SessionData<DirectX11>,
        waiter: FrameWaiter,
        stream: FrameStream<DirectX11>,
    );

    fn get_session_create_info(
        &self,
        data: &crate::compositor::CompositorSessionData,
    ) -> SessionCreateInfo;

    #[cfg(test)]
    fn on_restart(&self) {}
}

pub type RealWinlatorXrData = WinlatorXrData<crate::compositor::Compositor>;
pub type OpenXrData<C> = WinlatorXrData<C>;
pub type RealOpenXrData = RealWinlatorXrData;
pub type WxrData = RealWinlatorXrData;
pub type RealSessionData = SessionData<DirectX11>;

pub struct WinlatorXrData<C: Compositor> {
    pub instance: Instance,
    pub system_id: SystemId,
    pub session_data: SessionReadGuard,
    pub display_time: AtomicXrTime,
    pub display_period_nanos: AtomicI64,
    pub enabled_extensions: ExtensionSet,

    pub(crate) input: Injected<crate::input::Input<C>>,
    pub(crate) compositor: Injected<C>,
}

#[derive(Debug)]
pub struct Event;

impl<C: Compositor> WinlatorXrData<C> {
    pub fn new(injector: &Injector) -> std::result::Result<Self, InitError> {
        let instance = Instance::new()?;
        let system_id = instance.system_id;

        let session_data = SessionReadGuard::new(
            SessionData::new(
                &instance,
                system_id,
                vr::ETrackingUniverseOrigin::Standing,
                None,
            )?,
        );

        let display_time = instance.now().0;

        let enabled_extensions = ExtensionSet {
            display_refresh_rate: true,
            htc_vive_focus3_controller_interaction: true,
            ..Default::default()
        };

        Ok(Self {
            instance,
            system_id,
            session_data,
            display_time: AtomicXrTime::new(XrTime(display_time)),
            display_period_nanos: AtomicI64::new(11111111),
            enabled_extensions,
            input: injector.inject(),
            compositor: injector.inject(),
        })
    }

    pub fn poll_events(&self) {
        // A completed frame ("EndFrame") marks the session as synchronized,
        // but only once the app polls for the resulting event. This mirrors
        // when the real runtime reports `shouldRender = true`.
        #[cfg(test)]
        {
            let data = self.session_data.get();
            if let Some(session) = &data.session {
                crate::fakexr::apply_synchronize(session.as_raw());
            }
        }
        let data = self.session_data.get();
        if let Some(state) = self.poll_events_impl(&data) {
            drop(data);
            self.session_data.0.write().unwrap().state = state;
            let data = self.session_data.get();
            self.handle_profile_changes(&data);
            return;
        }
        self.handle_profile_changes(&data);
    }

fn handle_profile_changes(&self, data: &SessionData<DirectX11>) {
        #[cfg(test)]
        {
            let Some(session) = &data.session else { return };
            if !crate::fakexr::has_profile_changes(session.as_raw()) {
                return;
            }
            if let Some(input) = self.input.get() {
                input.interaction_profile_changed(data);
            }
            crate::fakexr::clear_profile_changes(session.as_raw());
        }
        #[cfg(not(test))]
        {
            if data.session.is_none() || !data.is_real_session() {
                return;
            }
            if let Some(input) = self.input.get() {
                input.ensure_production_controllers(data);
            }
        }
    }

    fn poll_events_impl(&self, _data: &SessionData<DirectX11>) -> Option<SessionState> {
        None
    }

    pub fn restart_session(&self) {
        let session_data = self.session_data.get();
        let create_info = self
            .compositor
            .get()
            .expect("Need to restart session, but compositor hasn't been set up...")
            .get_session_create_info(&session_data.comp_data);
        drop(session_data);

        let new_session_data = SessionData::new(
            &self.instance,
            self.system_id,
            vr::ETrackingUniverseOrigin::Standing,
            Some(create_info),
        );

        match new_session_data {
            Ok(new_data) => {
                let mut guard = self.session_data.0.write().unwrap();
                *guard = ManuallyDrop::new(new_data);
                // Re-initialize the input side against the fresh session data
                // (pose data, and the action manifest if one was loaded).
                if let Some(input) = self.input.get() {
                    input.post_session_restart(&guard);
                }
                // Re-initialize the compositor side (swapchain / frame
                // controller) against the fresh session data. Must happen while
                // the session write lock is held.
                if let Some(comp) = self.compositor.get() {
                    let session = guard
                        .session
                        .as_ref()
                        .expect("new session data should have a session");
                    comp.post_session_restart(
                        &guard,
                        FrameWaiter::new(session.as_raw()),
                        FrameStream::new(session.as_raw()),
                    );
                }
            }
            Err(e) => {
                warn!("Failed to restart session: {:?}", e);
            }
        }
    }

    pub fn begin_session(&self) {
        let mut data = self.session_data.0.write().unwrap();
        if let Some(session) = &mut data.session {
            if let Err(e) = session.begin(ViewConfigurationType::PrimaryStereo) {
                warn!("Failed to begin session: {:?}", e);
            }
        }
    }

    pub fn end_session(&self, data: &mut SessionData<DirectX11>) {
        if let Some(session) = &mut data.session {
            if let Err(e) = session.end() {
                warn!("Failed to end session: {:?}", e);
            }
        }
        data.session = None;
    }

    pub fn get_refresh_rate(&self) -> f32 {
        self.instance.refresh_rate
    }

    pub fn reset_tracking_space(&self, _origin: ReferenceSpaceType) -> Result<(), SessionCreationError> {
        Err(SessionCreationError::InitializationFailed("Not implemented".to_string()))
    }

    pub fn get_tracking_space(&self) -> ReferenceSpaceType {
        ReferenceSpaceType::Local
    }

    pub fn set_tracking_space(&self, _origin: ReferenceSpaceType) -> Result<(), SessionCreationError> {
        Err(SessionCreationError::InitializationFailed("Not implemented".to_string()))
    }

    pub fn vulkan_legacy_instance_extensions(&self, _system_id: SystemId) -> Vec<String> {
        if cfg!(test) {
            vec!["VK_foo".to_string(), "VK_bar".to_string()]
        } else {
            Vec::new()
        }
    }

    pub fn vulkan_legacy_device_extensions(&self, _system_id: SystemId) -> Vec<String> {
        if cfg!(test) {
            vec!["VK_foo".to_string(), "VK_bar".to_string()]
        } else {
            Vec::new()
        }
    }
}

impl<C: Compositor> Drop for WinlatorXrData<C> {
    fn drop(&mut self) {
        let mut data = unsafe { ManuallyDrop::take(&mut *self.session_data.0.get_mut().unwrap()) };
        self.end_session(&mut data);
    }
}

#[derive(Debug)]
#[allow(dead_code)]
#[allow(clippy::enum_variant_names)]
pub enum InitError {
    InitializationFailed(String),
    SessionCreationFailed(SessionCreationError),
}

impl From<SessionCreationError> for InitError {
    fn from(value: SessionCreationError) -> Self {
        Self::SessionCreationFailed(value)
    }
}

impl From<crate::udp_communication::UdpError> for InitError {
    fn from(value: crate::udp_communication::UdpError) -> Self {
        Self::InitializationFailed(value.to_string())
    }
}

pub struct Instance {
    pub system_id: SystemId,
    pub refresh_rate: f32,
    udp: std::sync::Arc<crate::udp_communication::UdpComm>,
}

impl Instance {
    pub fn new() -> Result<Self, InitError> {
        let udp = std::sync::Arc::new(crate::udp_communication::UdpComm::new()?);
        Ok(Self {
            system_id: SystemId(0),
            refresh_rate: 90.0,
            udp,
        })
    }

    pub fn now(&self) -> XrTime {
        XrTime(std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64)
    }

    pub fn string_to_path(&self, path: &str) -> Result<Path, SessionCreationError> {
        Ok(string_to_path_global(path))
    }

    pub fn vulkan_graphics_device(&self, _system_id: SystemId, _instance: ash::vk::Instance) -> ash::vk::PhysicalDevice {
        ash::vk::PhysicalDevice::null()
    }

    pub fn create_action_set(
        &self,
        name: &str,
        localized_name: &str,
        priority: u32,
    ) -> Result<ActionSet, SessionCreationError> {
        Ok(ActionSet::new(name, localized_name, priority))
    }

    pub fn suggest_interaction_profile_bindings(
        &self,
        profile: Path,
        bindings: &[Binding],
    ) -> Result<(), SessionCreationError> {
        #[cfg(test)]
        crate::fakexr::suggest_bindings(profile, bindings);
        Ok(())
    }

    pub fn path_to_string(&self, path: Path) -> Result<String, SessionCreationError> {
        path_to_string_global(path).ok_or_else(|| {
            SessionCreationError::InitializationFailed(format!("Unknown path: {path:?}"))
        })
    }

    pub fn vulkan_legacy_instance_extensions(&self, _system_id: SystemId) -> Vec<String> {
        if cfg!(test) {
            vec!["VK_foo".to_string(), "VK_bar".to_string()]
        } else {
            Vec::new()
        }
    }

    pub fn vulkan_legacy_device_extensions(&self, _system_id: SystemId) -> Vec<String> {
        if cfg!(test) {
            vec!["VK_foo".to_string(), "VK_bar".to_string()]
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XrTime(pub i64);

impl XrTime {
    pub const ZERO: Self = Self(0);

    pub fn from_nanos(nanos: i64) -> Self {
        Self(nanos)
    }

    pub fn as_nanos(&self) -> i64 {
        self.0
    }
}

impl From<i64> for XrTime {
    fn from(value: i64) -> Self {
        Self(value)
    }
}

pub type Time = XrTime;

impl std::ops::Sub for XrTime {
    type Output = Duration;

    fn sub(self, rhs: Self) -> Duration {
        Duration::from_nanos((self.0 - rhs.0).max(0) as u64)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XrPosef {
    pub orientation: Quat,
    pub position: Vec3,
}

impl From<vr::HmdMatrix34_t> for XrPosef {
    fn from(value: vr::HmdMatrix34_t) -> Self {
        let m = value.m;
        let tr = m[0][0] + m[1][1] + m[2][2];
        let (qw, qx, qy, qz) = if tr > 0.0 {
            let s = (tr + 1.0).sqrt() * 2.0;
            (0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s)
        } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
            let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
            ((m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s)
        } else if m[1][1] > m[2][2] {
            let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
            ((m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s)
        } else {
            let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
            ((m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s)
        };
        Self {
            orientation: Quat::from_xyzw(qx, qy, qz, qw).normalize(),
            position: Vec3::new(m[0][3], m[1][3], m[2][3]),
        }
    }
}

impl From<XrPosef> for vr::HmdMatrix34_t {
    fn from(value: XrPosef) -> Self {
        let q = value.orientation.normalize();
        let v = value.position;
        let x2 = q.x + q.x;
        let y2 = q.y + q.y;
        let z2 = q.z + q.z;
        let xx = q.x * x2;
        let xy = q.x * y2;
        let xz = q.x * z2;
        let yy = q.y * y2;
        let yz = q.y * z2;
        let zz = q.z * z2;
        let wx = q.w * x2;
        let wy = q.w * y2;
        let wz = q.w * z2;
        vr::HmdMatrix34_t {
            m: [
                [1.0 - (yy + zz), xy - wz, xz + wy, v.x],
                [xy + wz, 1.0 - (xx + zz), yz - wx, v.y],
                [xz - wy, yz + wx, 1.0 - (xx + yy), v.z],
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XrVector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct XrVector2f {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Vector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Quaternionf {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

impl Quaternionf {
    pub const IDENTITY: Self = Self {
        x: 0.0,
        y: 0.0,
        z: 0.0,
        w: 1.0,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct EnvironmentBlendMode {
    pub opaque: bool,
    pub additive: bool,
    pub alpha_blend: bool,
}

impl EnvironmentBlendMode {
    pub const OPAQUE: Self = Self {
        opaque: true,
        additive: false,
        alpha_blend: false,
    };
}

#[derive(Debug, Clone, Copy)]
pub struct ViewConfigurationView {
    pub recommended_image_rect_width: u32,
    pub recommended_image_rect_height: u32,
    pub maximum_image_rect_width: u32,
    pub maximum_image_rect_height: u32,
    pub recommended_swapchain_sample_count: u32,
    pub maximum_swapchain_sample_count: u32,
    pub fov: XrFovf,
}

#[derive(Debug, Clone)]
pub struct SystemProperties {
    pub system_id: SystemId,
    pub vendor_id: u32,
    pub tracking_system_name: String,
    pub form_factor: FormFactor,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XrColor4f {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

pub type Color4f = XrColor4f;

#[derive(Debug, Clone, Copy)]
pub struct XrExtent2Df {
    pub width: f32,
    pub height: f32,
}

pub type Extent2Df = XrExtent2Df;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Unknown,
    Idle,
    Ready,
    Synchronized,
    Visible,
    Focused,
    Stopping,
    LossPending,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisibilityMaskType {
    HiddenTriangleMesh,
    VisibleTriangleMesh,
    LineLoop,
}

#[derive(Debug, Clone, Copy)]
pub struct VisibilityMaskKHR {
    pub vertex_capacity_input: u32,
    pub vertex_count_output: u32,
    pub vertices: *mut XrVector2f,
    pub index_capacity_input: u32,
    pub index_count_output: u32,
    pub indices: *mut u32,
}

impl XrPosef {
    pub const IDENTITY: Self = Self {
        orientation: Quat::IDENTITY,
        position: Vec3::ZERO,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReferenceSpaceType {
    View,
    Local,
    Stage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormFactor {
    HeadMountedDisplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewConfigurationType {
    PrimaryStereo,
}

impl ViewConfigurationType {
    pub const PRIMARY_STEREO: Self = Self::PrimaryStereo;
}

#[derive(Debug, Clone, Copy)]
pub struct ViewStateFlags {
    pub position_valid: bool,
    pub orientation_valid: bool,
}

impl ViewStateFlags {
    pub const POSITION_VALID: Self = Self {
        position_valid: true,
        orientation_valid: false,
    };
    pub const ORIENTATION_VALID: Self = Self {
        position_valid: false,
        orientation_valid: true,
    };

    pub fn contains(&self, flags: Self) -> bool {
        (self.position_valid && flags.position_valid)
            || (self.orientation_valid && flags.orientation_valid)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct View {
    pub pose: XrPosef,
    pub fov: XrFovf,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct XrFovf {
    pub angle_left: f32,
    pub angle_right: f32,
    pub angle_up: f32,
    pub angle_down: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SwapchainCreateFlags(pub u32);

impl SwapchainCreateFlags {
    pub const EMPTY: Self = Self(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwapchainUsageFlags {
    pub bits: u64,
}

impl SwapchainUsageFlags {
    pub const EMPTY: Self = Self { bits: 0 };
    pub const COLOR_ATTACHMENT: Self = Self { bits: 1 };
    pub const TRANSFER_DST: Self = Self { bits: 2 };
}

impl std::ops::BitOr for SwapchainUsageFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits | rhs.bits,
        }
    }
}

pub struct SessionData<G> {
    pub session: Option<Session<G>>,
    pub state: SessionState,
    pub frame_waiter: Option<FrameWaiter>,
    pub frame_stream: Option<FrameStream<G>>,
    pub input_data: crate::input::InputSessionData,
    pub comp_data: crate::compositor::CompositorSessionData,
    pub overlay_data: crate::overlay::OverlayData,
    /// Whether this session was created with a temporary (fake) graphics
    /// setup. Only sessions created from an application-provided
    /// `SessionCreateInfo` are "real".
    temp_vulkan: bool,
}

impl<G: Graphics> SessionData<G> {
    pub fn new(
        _instance: &Instance,
        system_id: SystemId,
        _universe_origin: vr::ETrackingUniverseOrigin,
        create_info: Option<SessionCreateInfo>,
    ) -> std::result::Result<Self, SessionCreationError> {
        let temp_vulkan = create_info.is_none();
        let create_info = create_info.unwrap_or(SessionCreateInfo {
            system_id,
            graphics_binding: GraphicsBinding::Vulkan(VulkanGraphicsBinding {
                instance: ash::vk::Instance::null(),
                physical_device: ash::vk::PhysicalDevice::null(),
                device: ash::vk::Device::null(),
                queue_family_index: 0,
                queue_index: 0,
            }),
        });

let session = Session::new(create_info)?;
        let state = session.state;

        // WinlatorXR only starts streaming tracking data after it receives a
        // Tx UDP packet ("UDP Tx Startup" in docs/PROTOCOL.md). Send one the
        // moment a real session exists, so poses arrive before rendering.
        // The per-frame `end_frame` keepalives double as retries.
        if !temp_vulkan {
            if let Err(e) = session.send_startup_packet() {
                log::warn!("Failed to send XrAPI startup packet: {e}");
            }
        }

        Ok(Self {
            session: Some(session),
            state,
            frame_waiter: None,
            frame_stream: None,
            input_data: crate::input::InputSessionData::default(),
            comp_data: Default::default(),
            overlay_data: crate::overlay::OverlayData::default(),
            temp_vulkan,
        })
    }

    pub fn is_real_session(&self) -> bool {
        !self.temp_vulkan
    }

    pub fn check_format<T: GraphicsBackend>(&self, create_info: &mut SwapchainCreateInfo<T::Api>)
    where
        <T::Api as Graphics>::Format: PartialEq + Clone,
    {
        #[cfg(test)]
        {
            let supported = [unsafe { std::mem::zeroed::<<T::Api as Graphics>::Format>() }];
            if !supported.contains(&create_info.format) {
                create_info.format = supported[0].clone();
            }
        }
    }

    pub fn current_origin_as_reference_space(&self) -> ReferenceSpaceType {
        ReferenceSpaceType::Local
    }

    pub fn view_space(&self) -> Space {
        Space {
            space_type: ReferenceSpaceType::View,
            ..Default::default()
        }
    }

    pub fn current_origin(&self) -> vr::ETrackingUniverseOrigin {
        vr::ETrackingUniverseOrigin::Standing
    }

    pub fn get_space_for_origin(&self, origin: vr::ETrackingUniverseOrigin) -> Space {
        let space_type = match origin {
            vr::ETrackingUniverseOrigin::Standing => ReferenceSpaceType::Stage,
            vr::ETrackingUniverseOrigin::Seated | vr::ETrackingUniverseOrigin::RawAndUncalibrated => {
                ReferenceSpaceType::Local
            }
        };
        Space {
            space_type,
            ..Default::default()
        }
    }

    pub fn tracking_space(&self) -> Space {
        Space {
            space_type: ReferenceSpaceType::Local,
            ..Default::default()
        }
    }

    pub fn begin_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn end_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn wait_frame(&mut self) -> Result<(FrameWaiter, XrTime), SessionCreationError> {
        let session = self
            .session
            .as_ref()
            .expect("Cannot wait for frame without a session")
            .as_raw();
        Ok((FrameWaiter::new(session), XrTime(0)))
    }

    pub fn locate_views(&self, _display_time: XrTime, _view_type: ViewConfigurationType) -> (Vec<View>, ViewStateFlags) {
        if let Some(session) = &self.session {
            if let Some(pose) = session.get_latest_pose() {
                let fov_h_rad = (pose.fov_h / 2.0).to_radians();
                let fov_v_rad = (pose.fov_v / 2.0).to_radians();
                let half_ipd = pose.ipd * 0.5;

                let left_view = View {
                    pose: XrPosef {
                        orientation: pose.hmd_quat,
                        position: pose.hmd_pos + Vec3::new(-half_ipd, 0.0, 0.0),
                    },
                    fov: XrFovf {
                        angle_left: -fov_h_rad,
                        angle_right: fov_h_rad,
                        angle_up: fov_v_rad,
                        angle_down: -fov_v_rad,
                    },
                };

                let right_view = View {
                    pose: XrPosef {
                        orientation: pose.hmd_quat,
                        position: pose.hmd_pos + Vec3::new(half_ipd, 0.0, 0.0),
                    },
                    fov: XrFovf {
                        angle_left: -fov_h_rad,
                        angle_right: fov_h_rad,
                        angle_up: fov_v_rad,
                        angle_down: -fov_v_rad,
                    },
                };

                return (vec![left_view, right_view], ViewStateFlags {
                    position_valid: true,
                    orientation_valid: true,
                });
            }
        }

        (vec![Self::default_view(), Self::default_view()], ViewStateFlags {
            position_valid: false,
            orientation_valid: false,
        })
    }

    fn default_view() -> View {
        View {
            pose: XrPosef::IDENTITY,
            fov: XrFovf {
                angle_left: -1.0,
                angle_right: 1.0,
                angle_up: 1.0,
                angle_down: -1.0,
            },
        }
    }

    pub fn get_view_configuration_views(&self, _view_type: ViewConfigurationType) -> Vec<ViewConfigurationView> {
        vec![
            ViewConfigurationView {
                recommended_image_rect_width: 1024,
                recommended_image_rect_height: 1024,
                maximum_image_rect_width: 2048,
                maximum_image_rect_height: 2048,
                recommended_swapchain_sample_count: 1,
                maximum_swapchain_sample_count: 1,
                fov: XrFovf {
                    angle_left: -1.0,
                    angle_right: 1.0,
                    angle_up: 1.0,
                    angle_down: -1.0,
                },
            },
            ViewConfigurationView {
                recommended_image_rect_width: 1024,
                recommended_image_rect_height: 1024,
                maximum_image_rect_width: 2048,
                maximum_image_rect_height: 2048,
                recommended_swapchain_sample_count: 1,
                maximum_swapchain_sample_count: 1,
                fov: XrFovf {
                    angle_left: -1.0,
                    angle_right: 1.0,
                    angle_up: 1.0,
                    angle_down: -1.0,
                },
            },
        ]
    }

    pub fn get_system_properties(&self, _system_id: SystemId) -> SystemProperties {
        SystemProperties {
            system_id: _system_id,
            vendor_id: 0,
            tracking_system_name: "WinlatorXR".to_string(),
            form_factor: FormFactor::HeadMountedDisplay,
        }
    }
}

impl SessionData<crate::graphics_backends::DirectX11> {
    pub fn create_swapchain<T: GraphicsBackend>(
        &self,
        create_info: &SwapchainCreateInfo<T::Api>,
    ) -> Result<Swapchain<T::Api>, SessionCreationError>
    where
        <T::Api as Graphics>::Format: Clone,
    {
        let session = self
            .session
            .as_ref()
            .expect("Cannot create a swapchain without a session");
        let session = unsafe {
            &*(session as *const Session<crate::graphics_backends::DirectX11>
                as *const Session<T::Api>)
        };
        session.create_swapchain(create_info)
    }
}

pub struct Session<G> {
    pub system_id: SystemId,
    pub state: SessionState,
    pub udp: std::sync::Arc<crate::udp_communication::UdpComm>,
    pub latest_pose_data: std::sync::Arc<RwLock<Option<crate::udp_communication::WinlatorPoseData>>>,
    pub pose_receiver_thread: Option<thread::JoinHandle<()>>,
    pub shutdown_signal: std::sync::Arc<AtomicBool>,
    /// Production input state: current/last value per (action path, hand path),
    /// used to back `Action::state`/`is_active` from the UDP pose data.
    pub input_state: std::sync::Arc<RwLock<HashMap<(u64, u64), InputActionEntry>>>,
    _phantom: std::marker::PhantomData<G>,
}

/// Tracks the production value of an action for change/active reporting.
#[derive(Debug, Clone, Copy)]
pub struct InputActionEntry {
    pub(crate) value: XrActionValue,
    pub(crate) previous_synced: XrActionValue,
    pub(crate) is_active: bool,
    pub(crate) last_change_time: XrTime,
}

impl Default for InputActionEntry {
    fn default() -> Self {
        Self {
            value: XrActionValue::Unresolved,
            previous_synced: XrActionValue::Unresolved,
            is_active: false,
            last_change_time: XrTime(0),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VulkanGraphicsBinding {
    pub instance: ash::vk::Instance,
    pub physical_device: ash::vk::PhysicalDevice,
    pub device: ash::vk::Device,
    pub queue_family_index: u32,
    pub queue_index: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenGLGraphicsBinding {
    pub context: *mut std::ffi::c_void,
    pub display: *mut std::ffi::c_void,
}

#[derive(Debug, Clone)]
pub enum GraphicsBinding {
    Vulkan(VulkanGraphicsBinding),
    OpenGL(OpenGLGraphicsBinding),
}

#[derive(Debug, Clone, Copy)]
pub struct OpenGLSessionCreateInfo {
    pub h_dc: *mut std::ffi::c_void,
    pub h_glrc: *mut std::ffi::c_void,
}

#[derive(Debug, Clone)]
pub struct VulkanSessionCreateInfo {
    pub instance: ash::vk::Instance,
    pub physical_device: ash::vk::PhysicalDevice,
    pub device: ash::vk::Device,
    pub queue_family_index: u32,
    pub queue_index: u32,
}

pub struct VulkanSwapchainCreateInfoKHR {
    pub create_info: SwapchainCreateInfo<Vulkan>,
}

pub struct OpenGLSwapchainCreateInfoKHR {
    pub create_info: SwapchainCreateInfo<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct ExtensionSet {
    pub hand_tracking: bool,
    pub khr_visibility_mask: bool,
    pub display_refresh_rate: bool,
    pub khr_composition_layer_cylinder: bool,
    pub khr_composition_layer_equirect2: bool,
    pub khr_composition_layer_color_scale_bias: bool,
    pub htc_vive_focus3_controller_interaction: bool,
}

#[derive(Debug)]
pub enum SessionCreationError {
    GraphicsBindingRequired,
    InvalidGraphicsBinding,
    InitializationFailed(String),
}

impl std::fmt::Display for SessionCreationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GraphicsBindingRequired => write!(f, "graphics binding required"),
            Self::InvalidGraphicsBinding => write!(f, "invalid graphics binding"),
            Self::InitializationFailed(msg) => write!(f, "initialization failed: {msg}"),
        }
    }
}

impl std::error::Error for SessionCreationError {}

pub struct SessionCreateInfo {
    pub system_id: SystemId,
    pub graphics_binding: GraphicsBinding,
}

impl SessionCreateInfo {
    pub fn from_info<G: Graphics>(_info: G::SessionCreateInfo) -> Self {
        Self {
            system_id: SystemId(0),
            graphics_binding: GraphicsBinding::Vulkan(VulkanGraphicsBinding {
                instance: ash::vk::Instance::null(),
                physical_device: ash::vk::PhysicalDevice::null(),
                device: ash::vk::Device::null(),
                queue_family_index: 0,
                queue_index: 0,
            }),
        }
    }
}

pub struct GraphicalSession<G> {
    pub session: GenericSession<G>,
}

pub struct GenericSession<G> {
    pub session: SessionData<G>,
    pub graphics: G,
}

impl<'a> From<&'a GraphicalSession<DirectX11>> for &'a Session<DirectX11> {
    fn from(gs: &'a GraphicalSession<DirectX11>) -> Self {
        gs.session.session
            .session
            .as_ref()
            .expect("session not initialized")
    }
}

impl<'a> From<&'a GraphicalSession<DirectX11>> for &'a Session<crate::graphics_backends::Vulkan> {
    fn from(gs: &'a GraphicalSession<DirectX11>) -> Self {
        let session = gs.session.session
            .session
            .as_ref()
            .expect("session not initialized");
        unsafe {
            &*(session as *const Session<DirectX11>
                as *const Session<crate::graphics_backends::Vulkan>)
        }
    }
}

impl<'a> From<&'a GraphicalSession<DirectX11>> for &'a SessionData<DirectX11> {
    fn from(gs: &'a GraphicalSession<DirectX11>) -> Self {
        &gs.session.session
    }
}

impl From<FrameStream<crate::graphics_backends::DirectX11Data>> for FrameStream<DirectX11> {
    fn from(stream: FrameStream<crate::graphics_backends::DirectX11Data>) -> Self {
        FrameStream::new(stream.session)
    }
}

impl From<FrameStream<crate::graphics_backends::VulkanData>> for FrameStream<crate::graphics_backends::Vulkan> {
    fn from(stream: FrameStream<crate::graphics_backends::VulkanData>) -> Self {
        FrameStream::new(stream.session)
    }
}

impl From<FrameStream<DirectX11>> for FrameStream<crate::graphics_backends::Vulkan> {
    fn from(stream: FrameStream<DirectX11>) -> Self {
        FrameStream::new(stream.session)
    }
}

#[cfg(test)]
impl<'a> From<&'a GraphicalSession<DirectX11>>
    for &'a Session<crate::compositor::FakeApi>
{
    fn from(gs: &'a GraphicalSession<DirectX11>) -> Self {
        let session = gs
            .session
            .session
            .session
            .as_ref()
            .expect("session not initialized");
        unsafe {
            &*(session as *const Session<DirectX11>
                as *const Session<crate::compositor::FakeApi>)
        }
    }
}

#[cfg(test)]
impl From<FrameStream<DirectX11>> for FrameStream<crate::compositor::FakeApi> {
    fn from(stream: FrameStream<DirectX11>) -> Self {
        FrameStream::new(stream.session)
    }
}

pub type GraphicalSessionType = GraphicalSession<ash::vk::PhysicalDevice>;

#[derive(Debug, Clone)]
pub struct CreateInfo {
    pub system_id: SystemId,
    pub graphics_binding: GraphicsBinding,
}

impl CreateInfo {
    pub fn new(system_id: SystemId, graphics_binding: GraphicsBinding) -> Self {
        Self {
            system_id,
            graphics_binding,
        }
    }
}

impl From<GraphicsBinding> for SessionCreateInfo {
    fn from(binding: GraphicsBinding) -> Self {
        SessionCreateInfo {
            system_id: SystemId(0),
            graphics_binding: binding,
        }
    }
}

impl From<CreateInfo> for SessionCreateInfo {
    fn from(info: CreateInfo) -> Self {
        SessionCreateInfo {
            system_id: info.system_id,
            graphics_binding: info.graphics_binding,
        }
    }
}

impl<G: Graphics> Session<G> {
    pub fn new(create_info: SessionCreateInfo) -> Result<Self, SessionCreationError> {
        let udp = match &create_info.graphics_binding {
            GraphicsBinding::Vulkan(binding) => {
                std::sync::Arc::new(crate::udp_communication::UdpComm::new()
                    .map_err(|e| SessionCreationError::InitializationFailed(e.to_string()))?)
            }
            GraphicsBinding::OpenGL(_binding) => {
                return Err(SessionCreationError::InvalidGraphicsBinding);
            }
        };

        let latest_pose_data = std::sync::Arc::new(RwLock::new(None));
        let input_state = std::sync::Arc::new(RwLock::new(HashMap::new()));
        let shutdown_signal = std::sync::Arc::new(AtomicBool::new(false));

        let udp_clone = std::sync::Arc::clone(&udp);
        let pose_clone = std::sync::Arc::clone(&latest_pose_data);
        let shutdown_clone = std::sync::Arc::clone(&shutdown_signal);

        let thread = thread::spawn(move || {
            Self::pose_receiver_thread(udp_clone, pose_clone, shutdown_clone);
        });

        Ok(Self {
            system_id: create_info.system_id,
            state: SessionState::Ready,
            udp,
            latest_pose_data,
            input_state,
            pose_receiver_thread: Some(thread),
            shutdown_signal,
            _phantom: std::marker::PhantomData,
        })
    }

    fn pose_receiver_thread(
        udp: std::sync::Arc<crate::udp_communication::UdpComm>,
        pose_data: std::sync::Arc<RwLock<Option<crate::udp_communication::WinlatorPoseData>>>,
        shutdown: std::sync::Arc<AtomicBool>,
    ) {
        while !shutdown.load(Ordering::Relaxed) {
            match udp.receive_pose() {
                Ok(Some(data_str)) => {
                    if let Ok(pose) = crate::udp_communication::parse_winlator_pose(&data_str) {
                        let mut writer = pose_data.write().unwrap();
                        *writer = Some(pose);
                    }
                }
                Ok(None) => {
                    thread::sleep(Duration::from_millis(1));
                }
                Err(ref e) if matches!(e, crate::udp_communication::UdpError::ReceiveFailed(_)) => {
                    log::error!("Pose receive error: {:?}", e);
                    thread::sleep(Duration::from_millis(100));
                }
                Err(_) => {
                    thread::sleep(Duration::from_millis(1));
                }
            }
        }
    }

    pub fn get_latest_pose(&self) -> Option<crate::udp_communication::WinlatorPoseData> {
        self.latest_pose_data.read().unwrap().as_ref().cloned()
    }

    /// Read the current production value of an action for a given hand path,
    /// resolving the OpenVR action path against the latest UDP pose data.
    fn action_value_from_input(
        &self,
        action: Path,
        subaction: Path,
        now: XrTime,
    ) -> XrActionValue {
        let key = (action.0, subaction.0);
        let Some(pose) = self.get_latest_pose() else {
            // No runtime data yet: fall back to whatever we last stored.
            return self
                .input_state
                .read()
                .unwrap()
                .get(&key)
                .map(|e| e.value)
                .unwrap_or(XrActionValue::Unresolved);
        };
        let name = action_name_from_path(action.0);
        let hand = if subaction == Path::NULL {
            None
        } else {
            hand_from_path(subaction.0)
        };
        let value = resolve_action(name, hand, &pose);
        if value != XrActionValue::Unresolved {
            let mut guard = self.input_state.write().unwrap();
            update_entry(guard.entry(key).or_default(), value, now);
        }
        value
    }

    fn now_time(&self) -> XrTime {
        XrTime(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i64,
        )
    }

    pub fn begin(&mut self, _view_type: ViewConfigurationType) -> Result<(), SessionCreationError> {
        self.state = SessionState::Synchronized;
        Ok(())
    }

    pub fn end(&mut self) -> Result<(), SessionCreationError> {
        self.state = SessionState::Stopping;
        Ok(())
    }

    pub fn request_exit(&mut self) {
        self.state = SessionState::Exiting;
    }

    pub fn create_reference_space(&self, _space_type: ReferenceSpaceType) -> Result<Space, SessionCreationError> {
        Ok(Space {
            space_type: _space_type,
            ..Default::default()
        })
    }

pub fn send_haptic(&self, data: crate::udp_communication::WinlatorHapticData) -> Result<(), SessionCreationError> {
        self.udp.send_haptic(&data).map_err(|e| SessionCreationError::InitializationFailed(e.to_string()))
    }

    /// Sends the "start XrAPI" packet. WinlatorXR only begins streaming
    /// tracking data back to us after it receives at least one Tx UDP packet
    /// (see docs/PROTOCOL.md "UDP Tx Startup"), so this is sent once as soon
    /// as a real session exists instead of waiting for the first frame to be
    /// submitted.
    pub fn send_startup_packet(&self) -> Result<(), SessionCreationError> {
        log::info!("Sending XrAPI startup packet");
        self.send_haptic(crate::udp_communication::WinlatorHapticData {
            left_vibration: 0.0,
            right_vibration: 0.0,
            vr_flag: 1,
            sbs_flag: false,
            target_fov_w: 104.5,
            target_fov_h: 104.5,
        })
    }

    pub fn as_raw(&self) -> RawSession {
        RawSession(self.system_id.0)
    }

    pub fn attach_action_sets(&self, _sets: &[&ActionSet]) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn sync_actions(&self, _sets: &[ActiveActionSet]) -> Result<(), SessionCreationError> {
        #[cfg(test)]
        crate::fakexr::sync_actions(self.as_raw());
        // Commit the currently-seen values as the "previous synced" baseline so
        // that `changed_since_last_sync` only reports transitions after a sync.
        let mut guard = self.input_state.write().unwrap();
        for entry in guard.values_mut() {
            entry.previous_synced = entry.value;
        }
        Ok(())
    }

    pub fn current_interaction_profile(&self, _top_level_user_path: Path) -> Result<Path, SessionCreationError> {
        #[cfg(test)]
        {
            if let Some(profile) = crate::fakexr::current_interaction_profile(self.as_raw(), _top_level_user_path) {
                return Ok(profile);
            }
        }
        Ok(interaction_profile_path())
    }

    pub fn get_visibility_mask_khr(
        &self,
        _view_configuration_type: ViewConfigurationType,
        _view_index: u32,
        _mask_type: VisibilityMaskType,
    ) -> Result<VisibilityMaskKHR, SessionCreationError> {
        Ok(VisibilityMaskKHR {
            vertex_capacity_input: 0,
            vertex_count_output: 0,
            vertices: std::ptr::null_mut(),
            index_capacity_input: 0,
            index_count_output: 0,
            indices: std::ptr::null_mut(),
        })
    }

    pub fn create_hand_tracker(&self, hand: Hand) -> Result<HandTracker, SessionCreationError> {
        HandTracker::new(self, hand)
    }

pub fn create_swapchain(&self, _create_info: &SwapchainCreateInfo<G>) -> Result<Swapchain<G>, SessionCreationError>
    where
        <G as Graphics>::Format: Clone,
    {
        Ok(Swapchain {
            width: _create_info.width,
            height: _create_info.height,
            format: _create_info.format.clone(),
            sample_count: _create_info.sample_count,
            create_flags: _create_info.create_flags,
            usage_flags: _create_info.usage_flags,
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn wait_frame(&mut self) -> Result<(FrameWaiter, XrTime), SessionCreationError> {
        use std::sync::atomic::{AtomicU8, Ordering};
        let last_frame = std::sync::Arc::new(AtomicU8::new(0));

        loop {
            if let Some(pose) = self.get_latest_pose() {
                if pose.frame_id != last_frame.load(Ordering::Relaxed) {
                    last_frame.store(pose.frame_id, Ordering::Relaxed);
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as i64;
                    return Ok((FrameWaiter::new(self.as_raw()), XrTime(now)));
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn begin_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn end_frame(&mut self, _layers: &[CompositionLayerBase<G>]) -> Result<(), SessionCreationError> {
        let is_sbs = _layers
            .iter()
            .any(|layer| layer.ty == StructureType::CompositionLayerProjection);

        let haptic_data = crate::udp_communication::WinlatorHapticData {
            left_vibration: 0.0,
            right_vibration: 0.0,
            vr_flag: 1,
            sbs_flag: is_sbs,
            target_fov_w: 104.5,
            target_fov_h: 104.5,
        };

        self.send_haptic(haptic_data)
    }
}

impl<G> Drop for Session<G> {
    fn drop(&mut self) {
        self.shutdown_signal.store(true, Ordering::Relaxed);
        if let Some(thread) = self.pose_receiver_thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SpaceHandle(pub u64);

impl SpaceHandle {
    pub fn into_raw(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Space {
    pub space_type: ReferenceSpaceType,
    /// Only set for action spaces created in test builds.
    pub action_handle: Option<Path>,
    pub hand_path: Option<Path>,
    pub session: Option<RawSession>,
    pub offset: XrPosef,
}

impl Default for Space {
    fn default() -> Self {
        Self {
            space_type: ReferenceSpaceType::Local,
            action_handle: None,
            hand_path: None,
            session: None,
            offset: XrPosef::IDENTITY,
        }
    }
}

impl Space {
    pub fn as_raw(&self) -> SpaceHandle {
        SpaceHandle(self.space_type as u64)
    }

    pub fn locate_hand_joints(
        &self,
        tracker: &HandTracker,
        time: XrTime,
    ) -> Result<HandJointLocations, SessionCreationError> {
        tracker.locate_hand_joints(self, time)
    }

    pub fn locate(&self, _base_space: &Space, _time: XrTime) -> SpaceLocation {
        SpaceLocation {
            pose: XrPosef {
                orientation: Quat::IDENTITY,
                position: Vec3::ZERO,
            },
            position_valid: true,
            orientation_valid: true,
        }
    }

    pub fn locate_with_pose(&self, pose: &Option<crate::udp_communication::WinlatorPoseData>) -> SpaceLocation {
        if let Some(pose_data) = pose {
            match self.space_type {
                ReferenceSpaceType::View => {
                    SpaceLocation {
                        pose: XrPosef {
                            orientation: pose_data.hmd_quat,
                            position: pose_data.hmd_pos,
                        },
                        position_valid: true,
                        orientation_valid: true,
                    }
                }
                ReferenceSpaceType::Local | ReferenceSpaceType::Stage => {
                    SpaceLocation {
                        pose: XrPosef {
                            orientation: pose_data.hmd_quat,
                            position: pose_data.hmd_pos,
                        },
                        position_valid: true,
                        orientation_valid: true,
                    }
                }
            }
        } else {
            SpaceLocation {
                pose: XrPosef::IDENTITY,
                position_valid: false,
                orientation_valid: false,
            }
        }
    }

    pub fn relate(&self, _target_space: &Space, _time: XrTime) -> SpaceRelation {
        #[cfg(test)]
        {
            if let (Some(action), Some(hand), Some(session)) =
                (self.action_handle, self.hand_path, self.session)
            {
                if let Some(pose) = crate::fakexr::get_pose_for_space(session, hand) {
                    let mat = pose_to_mat(pose);
                    let offset = pose_to_mat(self.offset);
                    return SpaceRelation {
                        pose: mat_to_pose(mat * offset),
                        linear_velocity: Vec3::ZERO,
                        angular_velocity: Vec3::ZERO,
                        position_valid: true,
                        orientation_valid: true,
                        linear_velocity_valid: true,
                        angular_velocity_valid: true,
                    };
                }
                let _ = action;
            }
        }
        SpaceRelation {
            pose: XrPosef {
                orientation: Quat::IDENTITY,
                position: Vec3::ZERO,
            },
            linear_velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            position_valid: true,
            orientation_valid: true,
            linear_velocity_valid: true,
            angular_velocity_valid: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpaceLocation {
    pub pose: XrPosef,
    pub position_valid: bool,
    pub orientation_valid: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpaceRelation {
    pub pose: XrPosef,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub linear_velocity_valid: bool,
    pub angular_velocity_valid: bool,
}

impl SpaceRelation {
    pub fn ok(self) -> Option<Self> {
        (self.position_valid && self.orientation_valid).then_some(self)
    }
}

#[cfg(test)]
fn pose_to_mat(pose: XrPosef) -> glam::f32::Mat4 {
    glam::f32::Mat4::from_translation(pose.position)
        * glam::f32::Mat4::from_quat(pose.orientation)
}

#[cfg(test)]
fn mat_to_pose(mat: glam::f32::Mat4) -> XrPosef {
    let (_, _, translation) = mat.to_scale_rotation_translation();
    XrPosef {
        orientation: Quat::from_mat4(&mat).normalize(),
        position: translation,
    }
}

pub struct SwapchainCreateInfo<G: Graphics> {
    pub width: u32,
    pub height: u32,
    pub format: <G as Graphics>::Format,
    pub sample_count: u32,
    pub create_flags: SwapchainCreateFlags,
    pub usage_flags: SwapchainUsageFlags,
    pub face_count: u32,
    pub array_size: u32,
    pub mip_count: u32,
    _phantom: std::marker::PhantomData<G>,
}

impl<G: Graphics> SwapchainCreateInfo<G> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        width: u32,
        height: u32,
        format: <G as Graphics>::Format,
        sample_count: u32,
        create_flags: SwapchainCreateFlags,
        usage_flags: SwapchainUsageFlags,
        face_count: u32,
        array_size: u32,
        mip_count: u32,
    ) -> Self {
        Self {
            width,
            height,
            format,
            sample_count,
            create_flags,
            usage_flags,
            face_count,
            array_size,
            mip_count,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SwapchainImageVulkanKHR {
    pub image: ash::vk::Image,
    pub array_size: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct SwapchainImageOpenGLKHR {
    pub image: u32,
}

pub struct Swapchain<G: Graphics> {
    pub width: u32,
    pub height: u32,
    pub format: <G as Graphics>::Format,
    pub sample_count: u32,
    pub create_flags: SwapchainCreateFlags,
    pub usage_flags: SwapchainUsageFlags,
    _phantom: std::marker::PhantomData<G>,
}

impl<G: Graphics> Clone for Swapchain<G>
where
    <G as Graphics>::Format: Clone,
{
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            format: self.format.clone(),
            sample_count: self.sample_count,
            create_flags: self.create_flags,
            usage_flags: self.usage_flags,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<G: Graphics> Swapchain<G> {
    pub fn acquire_image(&self) -> Result<u32, SessionCreationError> {
        Ok(0)
    }

    pub fn wait_image(&self, _timeout: Duration) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn release_image(&self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn enumerate_images(&self) -> Result<Vec<u64>, SessionCreationError> {
        Ok(vec![0])
    }
}

pub struct FrameWaiter {
    session: RawSession,
}

#[derive(Debug, Clone, Copy)]
pub struct FrameState {
    pub should_render: bool,
    pub predicted_display_time: XrTime,
    pub predicted_display_period: Duration,
}

impl FrameWaiter {
    pub fn new(session: RawSession) -> Self {
        Self { session }
    }

    pub fn wait(&mut self) -> Result<FrameState, SessionCreationError> {
        #[cfg(test)]
        crate::fakexr::set_frame_state(self.session, crate::fakexr::FrameState::Waited);
        Ok(FrameState {
            should_render: {
                #[cfg(test)]
                {
                    crate::fakexr::session_synchronized(self.session)
                }
                #[cfg(not(test))]
                {
                    true
                }
            },
            predicted_display_time: XrTime(0),
            predicted_display_period: Duration::from_micros(11111),
        })
    }
}

pub struct FrameStream<G> {
    session: RawSession,
    _phantom: std::marker::PhantomData<G>,
}

impl<G> FrameStream<G> {
    pub fn new(session: RawSession) -> Self {
        Self {
            session,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn begin(&mut self) -> Result<(), SessionCreationError> {
        #[cfg(test)]
        crate::fakexr::set_frame_state(self.session, crate::fakexr::FrameState::Begun);
        Ok(())
    }

    pub fn end(
        &mut self,
        _display_time: XrTime,
        _blend_mode: EnvironmentBlendMode,
        _layers: &[&CompositionLayerBase<G>],
    ) -> Result<(), SessionCreationError> {
        #[cfg(test)]
        crate::fakexr::set_frame_state(self.session, crate::fakexr::FrameState::Ended);
        #[cfg(test)]
        crate::fakexr::queue_synchronize(self.session);
        Ok(())
    }
}

#[repr(C)]
pub struct CompositionLayerBase<G> {
    pub ty: StructureType,
    pub next: *const BaseInStructure,
    pub space: Space,
    pub layer_flags: CompositionLayerFlags,
    pub eye_visibility: EyeVisibility,
    _phantom: std::marker::PhantomData<G>,
}

pub struct CompositionLayerProjection<G> {
    pub ty: StructureType,
    pub next: *const BaseInStructure,
    pub space: Space,
    pub layer_flags: CompositionLayerFlags,
    pub eye_visibility: EyeVisibility,
    pub views: Vec<CompositionLayerProjectionView<G>>,
}

impl<G: Graphics> CompositionLayerProjection<G> {
    pub fn new() -> Self {
        Self {
            ty: StructureType::CompositionLayerProjection,
            next: std::ptr::null(),
            space: Space { space_type: ReferenceSpaceType::Local, ..Default::default() },
            layer_flags: CompositionLayerFlags::EMPTY,
            eye_visibility: EyeVisibility::Both,
            views: Vec::new(),
        }
    }

    pub fn space(mut self, value: Space) -> Self {
        self.space = value;
        self
    }

    pub fn layer_flags(mut self, value: CompositionLayerFlags) -> Self {
        self.layer_flags = value;
        self
    }

    pub fn eye_visibility(mut self, value: EyeVisibility) -> Self {
        self.eye_visibility = value;
        self
    }

    pub fn views(mut self, value: &[CompositionLayerProjectionView<G>]) -> Self {
        self.views = value.to_vec();
        self
    }
}

impl<G: Graphics> std::ops::Deref for CompositionLayerProjection<G> {
    type Target = CompositionLayerBase<G>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: CompositionLayerProjection is repr(C) and starts with the
        // same fields as CompositionLayerBase, so the common prefix is valid
        // as a base layer. This mirrors the approach used by openxr-rs.
        unsafe { &*(self as *const Self as *const CompositionLayerBase<G>) }
    }
}

pub struct CompositionLayerProjectionView<G> {
    pub pose: XrPosef,
    pub fov: XrFovf,
    pub sub_image: SwapchainSubImage<G>,
    _phantom: std::marker::PhantomData<G>,
}

impl<G> Clone for CompositionLayerProjectionView<G> {
    fn clone(&self) -> Self {
        Self {
            pose: self.pose,
            fov: self.fov,
            sub_image: self.sub_image.clone(),
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<G: Graphics> CompositionLayerProjectionView<G> {
    pub fn new() -> Self {
        Self {
            pose: XrPosef::IDENTITY,
            fov: XrFovf::default(),
            sub_image: SwapchainSubImage::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn pose(mut self, value: XrPosef) -> Self {
        self.pose = value;
        self
    }

    pub fn fov(mut self, value: XrFovf) -> Self {
        self.fov = value;
        self
    }

    pub fn sub_image(mut self, value: SwapchainSubImage<G>) -> Self {
        self.sub_image = value;
        self
    }
}

pub struct CompositionLayerQuad<G> {
    pub ty: StructureType,
    pub next: *const BaseInStructure,
    pub space: Space,
    pub layer_flags: CompositionLayerFlags,
    pub eye_visibility: EyeVisibility,
    pub sub_image: SwapchainSubImage<G>,
    pub pose: XrPosef,
    pub size: XrExtent2Df,
}

impl<G: Graphics> CompositionLayerQuad<G> {
    pub fn new() -> Self {
        Self {
            ty: StructureType::CompositionLayerQuad,
            next: std::ptr::null(),
            space: Space { space_type: ReferenceSpaceType::Local, ..Default::default() },
            layer_flags: CompositionLayerFlags::EMPTY,
            eye_visibility: EyeVisibility::Both,
            sub_image: SwapchainSubImage::new(),
            pose: XrPosef::IDENTITY,
            size: XrExtent2Df {
                width: 0.0,
                height: 0.0,
            },
        }
    }

    pub fn space(mut self, value: Space) -> Self {
        self.space = value;
        self
    }

    pub fn layer_flags(mut self, value: CompositionLayerFlags) -> Self {
        self.layer_flags = value;
        self
    }

    pub fn eye_visibility(mut self, value: EyeVisibility) -> Self {
        self.eye_visibility = value;
        self
    }

    pub fn sub_image(mut self, value: SwapchainSubImage<G>) -> Self {
        self.sub_image = value;
        self
    }

    pub fn pose(mut self, value: XrPosef) -> Self {
        self.pose = value;
        self
    }

    pub fn size(mut self, value: XrExtent2Df) -> Self {
        self.size = value;
        self
    }
}

impl<G: Graphics> std::ops::Deref for CompositionLayerQuad<G> {
    type Target = CompositionLayerBase<G>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: see CompositionLayerProjection::deref.
        unsafe { &*(self as *const Self as *const CompositionLayerBase<G>) }
    }
}

pub struct CompositionLayerCylinder<G> {
    pub ty: StructureType,
    pub next: *const BaseInStructure,
    pub space: Space,
    pub layer_flags: CompositionLayerFlags,
    pub eye_visibility: EyeVisibility,
    pub sub_image: SwapchainSubImage<G>,
    pub pose: XrPosef,
    pub radius: f32,
    pub central_angle: f32,
    pub aspect_ratio: f32,
}

impl<G: Graphics> CompositionLayerCylinder<G> {
    pub fn new() -> Self {
        Self {
            ty: StructureType::CompositionLayerCylinderKHR,
            next: std::ptr::null(),
            space: Space { space_type: ReferenceSpaceType::Local, ..Default::default() },
            layer_flags: CompositionLayerFlags::EMPTY,
            eye_visibility: EyeVisibility::Both,
            sub_image: SwapchainSubImage::new(),
            pose: XrPosef::IDENTITY,
            radius: 0.0,
            central_angle: 0.0,
            aspect_ratio: 0.0,
        }
    }

    pub fn space(mut self, value: Space) -> Self {
        self.space = value;
        self
    }

    pub fn layer_flags(mut self, value: CompositionLayerFlags) -> Self {
        self.layer_flags = value;
        self
    }

    pub fn eye_visibility(mut self, value: EyeVisibility) -> Self {
        self.eye_visibility = value;
        self
    }

    pub fn sub_image(mut self, value: SwapchainSubImage<G>) -> Self {
        self.sub_image = value;
        self
    }

    pub fn pose(mut self, value: XrPosef) -> Self {
        self.pose = value;
        self
    }

    pub fn radius(mut self, value: f32) -> Self {
        self.radius = value;
        self
    }

    pub fn central_angle(mut self, value: f32) -> Self {
        self.central_angle = value;
        self
    }

    pub fn aspect_ratio(mut self, value: f32) -> Self {
        self.aspect_ratio = value;
        self
    }
}

impl<G: Graphics> std::ops::Deref for CompositionLayerCylinder<G> {
    type Target = CompositionLayerBase<G>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: see CompositionLayerProjection::deref.
        unsafe { &*(self as *const Self as *const CompositionLayerBase<G>) }
    }
}

pub struct CompositionLayerEquirect<G> {
    pub ty: StructureType,
    pub next: *const BaseInStructure,
    pub space: Space,
    pub layer_flags: CompositionLayerFlags,
    pub eye_visibility: EyeVisibility,
    pub sub_image: SwapchainSubImage<G>,
    pub pose: XrPosef,
    pub radius: f32,
    pub central_horizontal_angle: f32,
    pub upper_vertical_angle: f32,
    pub lower_vertical_angle: f32,
}

impl<G: Graphics> CompositionLayerEquirect<G> {
    pub fn new() -> Self {
        Self {
            ty: StructureType::CompositionLayerEquirectKHR,
            next: std::ptr::null(),
            space: Space { space_type: ReferenceSpaceType::Local, ..Default::default() },
            layer_flags: CompositionLayerFlags::EMPTY,
            eye_visibility: EyeVisibility::Both,
            sub_image: SwapchainSubImage::new(),
            pose: XrPosef::IDENTITY,
            radius: 0.0,
            central_horizontal_angle: 0.0,
            upper_vertical_angle: 0.0,
            lower_vertical_angle: 0.0,
        }
    }

    pub fn space(mut self, value: Space) -> Self {
        self.space = value;
        self
    }

    pub fn layer_flags(mut self, value: CompositionLayerFlags) -> Self {
        self.layer_flags = value;
        self
    }

    pub fn eye_visibility(mut self, value: EyeVisibility) -> Self {
        self.eye_visibility = value;
        self
    }

    pub fn sub_image(mut self, value: SwapchainSubImage<G>) -> Self {
        self.sub_image = value;
        self
    }

    pub fn pose(mut self, value: XrPosef) -> Self {
        self.pose = value;
        self
    }

    pub fn radius(mut self, value: f32) -> Self {
        self.radius = value;
        self
    }

    pub fn central_horizontal_angle(mut self, value: f32) -> Self {
        self.central_horizontal_angle = value;
        self
    }

    pub fn upper_vertical_angle(mut self, value: f32) -> Self {
        self.upper_vertical_angle = value;
        self
    }

    pub fn lower_vertical_angle(mut self, value: f32) -> Self {
        self.lower_vertical_angle = value;
        self
    }
}

impl<G: Graphics> std::ops::Deref for CompositionLayerEquirect<G> {
    type Target = CompositionLayerBase<G>;

    fn deref(&self) -> &Self::Target {
        // SAFETY: see CompositionLayerProjection::deref.
        unsafe { &*(self as *const Self as *const CompositionLayerBase<G>) }
    }
}

pub struct SwapchainSubImage<G> {
    pub image_rect: Rect2Di,
    pub image_array_index: u32,
    _phantom: std::marker::PhantomData<G>,
}

impl<G> Clone for SwapchainSubImage<G> {
    fn clone(&self) -> Self {
        Self {
            image_rect: self.image_rect,
            image_array_index: self.image_array_index,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<G: Graphics> SwapchainSubImage<G> {
    pub fn new() -> Self {
        Self {
            image_rect: Rect2Di::default(),
            image_array_index: 0,
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn swapchain(self, _value: &Swapchain<G>) -> Self {
        self
    }

    pub fn image_rect(mut self, value: Rect2Di) -> Self {
        self.image_rect = value;
        self
    }

    pub fn image_array_index(mut self, value: u32) -> Self {
        self.image_array_index = value;
        self
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Offset2D {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct Rect2D {
    pub offset: Offset2D,
    pub extent: Extent2D,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Offset2Di {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Rect2Di {
    pub offset: Offset2Di,
    pub extent: Extent2Di,
}

#[derive(Clone)]
pub struct ActionSet {
    pub name: String,
    pub localized_name: String,
    pub priority: u32,
}

impl ActionSet {
    pub fn new(name: &str, localized_name: &str, priority: u32) -> Self {
        Self {
            name: name.to_string(),
            localized_name: localized_name.to_string(),
            priority,
        }
    }

    pub fn create_action<T: 'static>(&self, name: &str, _localized_name: &str, _subaction_paths: &[Path]) -> Result<Action<T>, SessionCreationError> {
        let path = string_to_path_global(&format!("/actions/{}/in/{name}", self.name));
        Ok(Action {
            action_type: std::any::TypeId::of::<T>(),
            name: name.to_string(),
            path,
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn suggest_interaction_profile_bindings(&self, _profile: Path, _bindings: &[Path]) -> Result<(), SessionCreationError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Path(pub u64);

impl Path {
    pub const NULL: Self = Self(0);
}

/// Raw handle for an OpenXR action, used by the test doubles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawAction(pub u64);

impl RawAction {
    pub fn as_ffi(&self) -> u64 {
        self.0
    }
}

/// Raw handle for an OpenXR session, used by the test doubles.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RawSession(pub u64);

impl RawSession {
    pub fn as_ffi(&self) -> u64 {
        self.0
    }
}

/// FNV-1a 64-bit hash, used to produce stable `Path` handles from strings.
pub fn hash_path(s: &str) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for b in s.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    hash
}

static PATH_CACHE: OnceLock<std::sync::Mutex<HashMap<u64, String>>> =
    OnceLock::new();

fn path_cache() -> &'static std::sync::Mutex<HashMap<u64, String>> {
    PATH_CACHE.get_or_init(Default::default)
}

/// Register a path string and return its (stable) handle. This is shared by
/// `Instance::string_to_path` and used by the test double to resolve handles
/// back to strings.
pub fn string_to_path_global(path: &str) -> Path {
    let handle = Path(hash_path(path));
    path_cache()
        .lock()
        .unwrap()
        .entry(handle.0)
        .or_insert_with(|| path.to_string());
    handle
}

/// Resolve a path handle back to its string, if it has been registered.
pub fn path_to_string_global(path: Path) -> Option<String> {
    if path == Path::NULL {
        return None;
    }
    path_cache().lock().unwrap().get(&path.0).cloned()
}

/// The interaction profile reported by `Session::current_interaction_profile` in
/// production. WinlatorXR does not report one, so we select a controller profile
/// that matches its physical layout (a thumbstick controller with grip/trigger
/// and X/Y + A/B buttons). Overridable via `XRIZER_INTERACTION_PROFILE`.
pub fn interaction_profile_path() -> Path {
    static PROFILE: OnceLock<Path> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        let name = std::env::var("XRIZER_INTERACTION_PROFILE").unwrap_or_else(|_| {
            "/interaction_profiles/oculus/touch_controller".to_string()
        });
        log::info!("using WinlatorXR interaction profile: {name}");
        string_to_path_global(&name)
    })
}

pub struct Action<T> {
    pub action_type: std::any::TypeId,
    pub name: String,
    pub path: Path,
    _phantom: std::marker::PhantomData<T>,
}

impl<T: 'static> Action<T> {
    pub fn as_raw(&self) -> RawAction {
        RawAction(self.path.0)
    }

    pub fn state<G: Graphics>(&self, session: &Session<G>, subaction_path: Path) -> Result<ActionState<T>, SessionCreationError> {
        #[cfg(test)]
        {
            if let Some(state) = crate::fakexr::get_action_state(self.path, subaction_path) {
                return Ok(ActionState {
                    current_state: convert_action_state::<T>(&state.state),
                    changed_since_last_sync: state.changed,
                    is_active: state.is_active,
                    last_change_time: state.last_change_time,
                });
            }
        }
        let now = session.now_time();
        let value = session.action_value_from_input(self.path, subaction_path, now);
        let entry = session
            .input_state
            .read()
            .unwrap()
            .get(&(self.path.0, subaction_path.0))
            .copied();
        let (changed, is_active, last_change_time) = match (entry, &value) {
            (Some(e), v) => (
                *v != XrActionValue::Unresolved && e.value != e.previous_synced,
                e.is_active,
                e.last_change_time,
            ),
            (None, v) => (*v != XrActionValue::Unresolved, *v != XrActionValue::Unresolved, now),
        };
        Ok(ActionState {
            current_state: convert_value_to_action_state::<T>(&value),
            changed_since_last_sync: changed,
            is_active,
            last_change_time,
        })
    }

    pub fn is_active<G: Graphics>(&self, session: &Session<G>, subaction_path: Path) -> Result<bool, SessionCreationError> {
        #[cfg(test)]
        {
            if let Some(active) = crate::fakexr::action_is_active(self.path, subaction_path, session.as_raw()) {
                return Ok(active);
            }
        }
        let now = session.now_time();
        let value = session.action_value_from_input(self.path, subaction_path, now);
        Ok(value != XrActionValue::Unresolved)
    }

    pub fn create_space<G: Graphics>(
        &self,
        session: &Session<G>,
        subaction_path: Path,
        pose_in_action_space: XrPosef,
    ) -> Result<Space, SessionCreationError> {
        Ok(Space {
            space_type: ReferenceSpaceType::Local,
            action_handle: Some(self.path),
            hand_path: Some(subaction_path),
            session: Some(session.as_raw()),
            offset: pose_in_action_space,
        })
    }

    pub fn apply_feedback<G: Graphics>(&self, session: &Session<G>, subaction_path: Path, haptic: &HapticVibration) -> Result<(), SessionCreationError> {
        #[cfg(test)]
        {
            crate::fakexr::trigger_haptic(self.path, subaction_path);
            return Ok(());
        }
        let haptic_data = crate::udp_communication::WinlatorHapticData {
            left_vibration: haptic.amplitude,
            right_vibration: haptic.amplitude,
            vr_flag: 1,
            sbs_flag: false,
            target_fov_w: 104.5,
            target_fov_h: 104.5,
        };

        session.send_haptic(haptic_data)
    }
}

/// Converts a `crate::fakexr::ActionState` into the concrete type `T` of an action.
/// This is only used by the test double, where `T` is one of the known action types.
#[cfg(test)]
fn convert_action_state<T: 'static>(state: &crate::fakexr::ActionState) -> T {
    use std::any::TypeId;

    let id = TypeId::of::<T>();
    if id == TypeId::of::<bool>() {
        let v = match state {
            crate::fakexr::ActionState::Bool(b) => *b,
            _ => false,
        };
        unsafe { std::mem::transmute_copy(&v) }
    } else if id == TypeId::of::<f32>() {
        let v = match state {
            crate::fakexr::ActionState::Float(f) => *f,
            _ => 0.0,
        };
        unsafe { std::mem::transmute_copy(&v) }
    } else if id == TypeId::of::<XrVector2f>() {
        let v = match state {
            crate::fakexr::ActionState::Vector2(x, y) => XrVector2f { x: *x, y: *y },
            _ => XrVector2f { x: 0.0, y: 0.0 },
        };
        unsafe { std::mem::transmute_copy(&v) }
    } else if id == TypeId::of::<HapticTy>() {
        unsafe { std::mem::zeroed() }
    } else {
        // Pose actions aren't read via `state`, so default to identity.
        unsafe { std::mem::zeroed() }
    }
}

/// The production value of an action, resolved from the UDP pose data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum XrActionValue {
    Bool(bool),
    Vector2(XrVector2f),
    /// The action could not be mapped to a known physical input for the hand.
    Unresolved,
}

/// Converts a production `XrActionValue` into the concrete type `T` of an action.
/// Only the boolean/vector2 action types exist in the WinlatorXR protocol;
/// float actions (e.g. the legacy trigger/squeeze axes) are synthesised from
/// their boolean buttons.
fn convert_value_to_action_state<T: 'static>(value: &XrActionValue) -> T {
    use std::any::TypeId;
    let id = TypeId::of::<T>();
    if id == TypeId::of::<bool>() {
        let v = match value {
            XrActionValue::Bool(b) => *b,
            _ => false,
        };
        unsafe { std::mem::transmute_copy(&v) }
    } else if id == TypeId::of::<f32>() {
        let v = match value {
            XrActionValue::Bool(b) => *b as u8 as f32,
            _ => 0.0,
        };
        unsafe { std::mem::transmute_copy(&v) }
    } else if id == TypeId::of::<XrVector2f>() {
        let v = match value {
            XrActionValue::Vector2(v) => *v,
            _ => XrVector2f { x: 0.0, y: 0.0 },
        };
        unsafe { std::mem::transmute_copy(&v) }
    } else {
        // Pose/haptic actions aren't read via `state`, so default to identity.
        unsafe { std::mem::zeroed() }
    }
}

/// Merge a freshly-resolved value into a tracked entry, updating the last
/// change time when the value transitions.
fn update_entry(entry: &mut InputActionEntry, value: XrActionValue, now: XrTime) {
    if entry.value != value {
        entry.last_change_time = now;
    }
    entry.value = value;
    entry.is_active = value != XrActionValue::Unresolved;
}

/// Extract the final segment of an action path (the part after `/in/`), if any.
fn action_name_from_path(handle: u64) -> Option<String> {
    let s = path_to_string_global(Path(handle))?;
    let (_, rest) = s.rsplit_once("/in/")?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Map a sub-action user path to a `Hand`, if it is a recognised hand path.
fn hand_from_path(handle: u64) -> Option<Hand> {
    let s = path_to_string_global(Path(handle))?;
    let s = s.trim_end_matches('/');
    if s.ends_with("/hand/left") {
        Some(Hand::Left)
    } else if s.ends_with("/hand/right") {
        Some(Hand::Right)
    } else {
        None
    }
}

/// Map a logical action name to its digital button state within the WinlatorXR
/// 19-button array for the given hand.
fn resolve_digital(name: Option<String>, hand: Hand, buttons: &[bool; 19]) -> Option<bool> {
    let key = name?.to_ascii_lowercase();
    let button = if key.contains("squeeze") || key.contains("grip") {
        button_index(hand, ButtonKind::Grip)
    } else if key.contains("menu") || key.contains("system") || key.contains("app-menu") {
        match hand {
            Hand::Left => Some(1),
            _ => None, // right menu button is intentionally absent in WinlatorXR
        }
    } else if key.contains("thumbstick-click")
        || key.contains("trackpad-click")
        || key.contains("joystick-click")
    {
        button_index(hand, ButtonKind::ThumbstickClick)
    } else if key.contains("thumbstick") || key.contains("trackpad") || key.contains("joystick") {
        // bare thumbstick/trackpad name is analog and resolved elsewhere
        None
    } else if key == "a" || key == "x" {
        button_index(hand, ButtonKind::ButtonA)
    } else if key == "b" || key == "y" {
        button_index(hand, ButtonKind::ButtonB)
    } else if key.contains("trigger") {
        button_index(hand, ButtonKind::TriggerClick)
    } else {
        None
    };
    button.map(|i| buttons[i])
}

/// Map a logical analog action name to an analog value from a thumbstick axis.
/// Only the thumbstick/trackpad/joystick axis is analog in WinlatorXR; trigger
/// and squeeze are single boolean buttons, so they are fully digital.
fn resolve_analog(name: Option<String>, fallback: Vec2) -> Option<XrActionValue> {
    let key = name?.to_ascii_lowercase();
    if (key.contains("thumbstick")
        || key.contains("trackpad")
        || key.contains("joystick"))
        && !key.contains("click")
        && !key.contains("touch")
    {
        Some(XrActionValue::Vector2(XrVector2f {
            x: fallback.x,
            y: fallback.y,
        }))
    } else {
        None
    }
}

/// Resolve an action name + hand against a full pose frame into a value.
fn resolve_action(name: Option<String>, hand: Option<Hand>, pose: &crate::udp_communication::WinlatorPoseData) -> XrActionValue {
    let Some(hand) = hand else {
        return XrActionValue::Unresolved;
    };
    let (buttons, thumb) = match hand {
        Hand::Left => (&pose.buttons, pose.left_hand_thumb),
        Hand::Right => (&pose.buttons, pose.right_hand_thumb),
    };
    if let Some(v) = resolve_analog(name.clone(), thumb) {
        return v;
    }
    match resolve_digital(name, hand, buttons) {
        Some(b) => XrActionValue::Bool(b),
        None => XrActionValue::Unresolved,
    }
}

enum ButtonKind {
    Grip,
    TriggerClick,
    ThumbstickClick,
    ButtonA,
    ButtonB,
}

const fn button_index(hand: Hand, kind: ButtonKind) -> Option<usize> {
    match (hand, kind) {
        (Hand::Left, ButtonKind::Grip) => Some(0),
        (Hand::Left, ButtonKind::TriggerClick) => Some(7),
        (Hand::Left, ButtonKind::ThumbstickClick) => Some(2),
        (Hand::Left, ButtonKind::ButtonA) => Some(8), // X
        (Hand::Left, ButtonKind::ButtonB) => Some(9), // Y
        (Hand::Right, ButtonKind::Grip) => Some(12),
        (Hand::Right, ButtonKind::TriggerClick) => Some(18),
        (Hand::Right, ButtonKind::ThumbstickClick) => Some(13),
        (Hand::Right, ButtonKind::ButtonA) => Some(10), // A
        (Hand::Right, ButtonKind::ButtonB) => Some(11), // B
    }
}

/// Maps the WinlatorXR button layout to per-finger curl targets in
/// `[thumb, index, middle, ring, pinky]` order. WinlatorXR only reports
/// buttons, so each curl is derived from the nearest physical input:
/// trigger -> index finger, grip -> the remaining fingers, thumbstick click
/// -> thumb.
fn hand_curl_target(hand: Hand, pose: Option<&crate::udp_communication::WinlatorPoseData>) -> [f32; 5] {
    let Some(pose) = pose else {
        return [0.0; 5];
    };
    let index: f32 = if pose.buttons[button_index(hand, ButtonKind::TriggerClick).unwrap()] {
        1.0
    } else {
        0.0
    };
    let grip: f32 = if pose.buttons[button_index(hand, ButtonKind::Grip).unwrap()] {
        1.0
    } else {
        0.0
    };
    let thumb: f32 = if pose.buttons[button_index(hand, ButtonKind::ThumbstickClick).unwrap()] {
        1.0
    } else {
        0.0
    };

    [
        thumb,
        index,
        // The rest of the hand follows the grip, with the index's curl
        // bleeding into the middle/ring/pinky fingers like a real hand.
        grip.max(index / 2.0),
        grip.max(index / 4.0),
        grip.max(index / 6.0),
    ]
}

impl<T> Clone for Action<T> {
    fn clone(&self) -> Self {
        Self {
            action_type: self.action_type,
            name: self.name.clone(),
            path: self.path,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[derive(Clone)]
pub struct ActionState<T> {
    pub current_state: T,
    pub changed_since_last_sync: bool,
    pub is_active: bool,
    pub last_change_time: XrTime,
}

impl<T> Default for ActionState<T> {
    fn default() -> Self {
        Self {
            current_state: unsafe { std::mem::zeroed() },
            changed_since_last_sync: false,
            is_active: false,
            last_change_time: XrTime(0),
        }
    }
}

impl<T: Copy> ActionState<T> {
    pub fn current_state(&self) -> T {
        self.current_state
    }
}

impl<T> ActionState<T> {
    pub fn is_active(&self) -> bool {
        self.is_active
    }

    pub fn changed_since_last_sync(&self) -> bool {
        self.changed_since_last_sync
    }

    pub fn last_change_time(&self) -> XrTime {
        self.last_change_time
    }
}

pub struct ActiveActionSet {
    pub action_set: ActionSet,
    pub subaction_path: Option<Path>,
}

impl ActiveActionSet {
    pub fn new(action_set: &ActionSet) -> Self {
        Self {
            action_set: action_set.clone(),
            subaction_path: None,
        }
    }
}

impl From<&ActionSet> for ActiveActionSet {
    fn from(action_set: &ActionSet) -> Self {
        Self::new(action_set)
    }
}

pub struct HapticVibration {
    pub amplitude: f32,
    pub duration: std::time::Duration,
    pub frequency: f32,
}

impl HapticVibration {
    pub fn new() -> Self {
        Self {
            amplitude: 0.0,
            duration: Duration::ZERO,
            frequency: FREQUENCY_UNSPECIFIED,
        }
    }

    pub fn amplitude(mut self, amplitude: f32) -> Self {
        self.amplitude = amplitude;
        self
    }

    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    pub fn frequency(mut self, frequency: f32) -> Self {
        self.frequency = frequency;
        self
    }
}

pub const FREQUENCY_UNSPECIFIED: f32 = 0.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Hand {
    Left = 1,
    Right = 2,
}

impl From<vr::ETrackedControllerRole> for Hand {
    fn from(role: vr::ETrackedControllerRole) -> Self {
        match role {
            vr::ETrackedControllerRole::LeftHand => Hand::Left,
            _ => Hand::Right,
        }
    }
}

impl From<Hand> for vr::ETrackedControllerRole {
    fn from(hand: Hand) -> Self {
        match hand {
            Hand::Left => vr::ETrackedControllerRole::LeftHand,
            Hand::Right => vr::ETrackedControllerRole::RightHand,
        }
    }
}

pub struct Binding {
    pub action: Path,
    pub path: Path,
}

impl Binding {
    pub fn new(action: &Path, path: &Path) -> Self {
        Self {
            action: *action,
            path: *path,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SpaceVelocity {
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub velocity_valid: bool,
    pub angular_velocity_valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandJoint {
    Palm,
    Wrist,
    THUMB_METACARPAL,
    THUMB_PROXIMAL,
    THUMB_DISTAL,
    THUMB_TIP,
    INDEX_METACARPAL,
    INDEX_PROXIMAL,
    INDEX_INTERMEDIATE,
    INDEX_DISTAL,
    INDEX_TIP,
    MIDDLE_METACARPAL,
    MIDDLE_PROXIMAL,
    MIDDLE_INTERMEDIATE,
    MIDDLE_DISTAL,
    MIDDLE_TIP,
    RING_METACARPAL,
    RING_PROXIMAL,
    RING_INTERMEDIATE,
    RING_DISTAL,
    RING_TIP,
    LITTLE_METACARPAL,
    LITTLE_PROXIMAL,
    LITTLE_INTERMEDIATE,
    LITTLE_DISTAL,
    LITTLE_TIP,
}

pub struct HandTracker {
    pub hand: Hand,
    pose_data: std::sync::Arc<RwLock<Option<crate::udp_communication::WinlatorPoseData>>>,
    curl_state: std::sync::Mutex<HandCurlState>,
}

/// Smoothing state for button-derived finger curls.
struct HandCurlState {
    value: [f32; 5],
    last: std::time::Instant,
}

impl HandCurlState {
    fn new() -> Self {
        Self {
            value: [0.0; 5],
            last: std::time::Instant::now(),
        }
    }
}

impl HandTracker {
    pub fn new<G: Graphics>(session: &Session<G>, hand: Hand) -> Result<Self, SessionCreationError> {
        Ok(Self {
            hand,
            pose_data: session.latest_pose_data.clone(),
            curl_state: std::sync::Mutex::new(HandCurlState::new()),
        })
    }

    pub fn locate_hand_joints(
        &self,
        _base_space: &Space,
        _time: XrTime,
    ) -> Result<HandJointLocations, SessionCreationError> {
        let pose = self.pose_data.read().unwrap().clone();
        let target = hand_curl_target(self.hand, pose.as_ref());

        // WinlatorXR buttons are binary, so smooth the curls over time to avoid
        // the fingers snapping between open and closed poses.
        let mut state = self.curl_state.lock().unwrap();
        let elapsed = state.last.elapsed().as_secs_f32();
        state.last = std::time::Instant::now();
        const FINGER_SMOOTHING_SPEED: f32 = 24.0;
        let t = (elapsed * FINGER_SMOOTHING_SPEED).min(1.0);
        for (value, target) in state.value.iter_mut().zip(target) {
            *value += (target - *value) * t;
        }

        Ok(crate::input::skeletal::synthesize_hand_joints(
            self.hand,
            state.value,
        ))
    }
}

#[derive(Debug, Clone, Default)]
pub struct HandJointLocations {
    pub joint_count: u32,
    pub joint_locations: Vec<HandJointLocation>,
    pub joint_radii: Vec<f32>,
}

impl IntoIterator for HandJointLocations {
    type Item = HandJointLocation;
    type IntoIter = std::vec::IntoIter<HandJointLocation>;

    fn into_iter(self) -> Self::IntoIter {
        self.joint_locations.into_iter()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct HandJointLocation {
    pub pose: XrPosef,
    pub radius: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositionLayerFlags {
    pub bits: u64,
}

impl CompositionLayerFlags {
    pub const EMPTY: Self = Self { bits: 0 };
    pub const CORRECT_ORIENTATION: Self = Self { bits: 1 };
    pub const CORRECT_ARRAY_SIZE: Self = Self { bits: 2 };
    pub const UNPREMULTIPLIED_ALPHA: Self = Self { bits: 4 };
    pub const BLEND_TEXTURE_SOURCE_ALPHA: Self = Self { bits: 8 };
}

impl std::ops::BitOr for CompositionLayerFlags {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits | rhs.bits,
        }
    }
}

impl std::ops::BitOrAssign for CompositionLayerFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.bits |= rhs.bits;
    }
}

impl Default for CompositionLayerFlags {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum StructureType {
    CompositionLayerProjection = 1000012000,
    CompositionLayerProjectionView = 1000012001,
    CompositionLayerQuad = 1000012002,
    CompositionLayerCylinderKHR = 1000015000,
    CompositionLayerEquirectKHR = 1000015001,
    CompositionLayerColorScaleBiasKHR = 1000016000,
}

pub mod sys {
    pub type Action = super::RawAction;
    pub type Session = super::RawSession;
    pub type Path = super::Path;
    pub type Haptic = super::HapticTy;
    pub type ActionState<T> = super::ActionState<T>;
    pub type ActionSet = super::ActionSet;
    pub type SessionCreateInfo = super::SessionCreateInfo;
    pub type ExtensionSet = super::ExtensionSet;
    pub type ViewConfigurationType = super::ViewConfigurationType;
    pub type EnvironmentBlendMode = super::EnvironmentBlendMode;
    pub type ViewConfigurationView = super::ViewConfigurationView;
    pub type ViewStateFlags = super::ViewStateFlags;
    pub type View = super::View;
    pub type XrFovf = super::XrFovf;
    pub type SwapchainCreateInfo<G> = super::SwapchainCreateInfo<G>;
    pub type Swapchain<G> = super::Swapchain<G>;
    pub type SwapchainSubImage<G> = super::SwapchainSubImage<G>;
    pub type CompositionLayerBase<G> = super::CompositionLayerBase<G>;
    pub type CompositionLayerProjection<G> = super::CompositionLayerProjection<G>;
    pub type CompositionLayerProjectionView<G> = super::CompositionLayerProjectionView<G>;
    pub type CompositionLayerQuad<G> = super::CompositionLayerQuad<G>;
    pub type CompositionLayerCylinder<G> = super::CompositionLayerCylinder<G>;
    pub type CompositionLayerEquirect<G> = super::CompositionLayerEquirect<G>;
    pub type ReferenceSpaceType = super::ReferenceSpaceType;
    pub type Space = super::Space;
    pub type SpaceLocation = super::SpaceLocation;
    pub type SpaceRelation = super::SpaceRelation;
    pub type SpaceVelocity = super::SpaceVelocity;
    pub type HandTracker = super::HandTracker;
    pub type HandJoint = super::HandJoint;
    pub type HandJointLocation = super::HandJointLocation;
    pub type HandJointLocations = super::HandJointLocations;
    pub type CompositionLayerColorScaleBiasKHR = super::CompositionLayerColorScaleBiasKHR;
    pub type BaseInStructure = super::BaseInStructure;
    pub type CompositionLayerFlags = super::CompositionLayerFlags;
    pub type StructureType = super::StructureType;
    pub type EyeVisibility = super::EyeVisibility;
    pub type SwapchainCreateFlags = super::SwapchainCreateFlags;
    pub type SwapchainUsageFlags = super::SwapchainUsageFlags;
    pub type SessionCreationError = super::SessionCreationError;
    pub type XrTime = super::XrTime;
    pub type Duration = super::Duration;
    pub type SystemId = super::SystemId;
    pub type SessionState = super::SessionState;
    pub type Instance = super::Instance;
    pub type InitError = super::InitError;
    pub type HapticVibration = super::HapticVibration;
    pub type Hand = super::Hand;
    pub type Binding = super::Binding;
    pub type ActiveActionSet = super::ActiveActionSet;
    pub type ViewState = ViewStateFlags;
    pub type FrameWaiter = super::FrameWaiter;
    pub type FrameStream<G> = super::FrameStream<G>;
    pub type SwapchainImageVulkanKHR = super::SwapchainImageVulkanKHR;
    pub type SwapchainImageOpenGLKHR = super::SwapchainImageOpenGLKHR;
    pub type GraphicalSession<G> = super::GraphicalSession<G>;
    pub type GenericSession<G> = super::GenericSession<G>;
    pub type CreateInfo = super::CreateInfo;
    pub type GraphicsBinding = super::GraphicsBinding;
    pub type VulkanGraphicsBinding = super::VulkanGraphicsBinding;
    pub type OpenGLGraphicsBinding = super::OpenGLGraphicsBinding;
    pub type VulkanSessionCreateInfo = super::VulkanSessionCreateInfo;
    pub type OpenGLSessionCreateInfo = super::OpenGLSessionCreateInfo;
    pub type VulkanSwapchainCreateInfoKHR = super::VulkanSwapchainCreateInfoKHR;
    pub type OpenGLSwapchainCreateInfoKHR = super::OpenGLSwapchainCreateInfoKHR;
    pub type VulkanSwapchainImageKHR = SwapchainImageVulkanKHR;
    pub type OpenGLSwapchainImageKHR = SwapchainImageOpenGLKHR;
    pub type VisibilityMaskType = super::VisibilityMaskType;
    pub type VisibilityMaskKHR = super::VisibilityMaskKHR;

    pub enum Result {
        Success,
        Error,
        ERROR_EXTENSION_NOT_PRESENT,
        ERROR_FEATURE_UNSUPPORTED,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EyeVisibility {
    Both,
    Left,
    Right,
}

impl EyeVisibility {
    pub const BOTH: Self = Self::Both;
}

#[repr(C)]
pub struct CompositionLayerColorScaleBiasKHR {
    pub ty: StructureType,
    pub next: *const BaseInStructure,
    pub color_scale: XrColor4f,
    pub color_bias: XrColor4f,
}

#[repr(C)]
pub struct BaseInStructure {
    pub type_id: u32,
    pub next: *const BaseInStructure,
}

impl BaseInStructure {
    pub unsafe fn from_raw(raw: *const std::ffi::c_void) -> Self {
        Self {
            type_id: 0,
            next: raw as *const BaseInStructure,
        }
    }
}

pub struct AtomicXrTime(AtomicI64);

impl AtomicXrTime {
    pub fn new(time: XrTime) -> Self {
        Self(AtomicI64::new(time.0))
    }

    pub fn get(&self) -> XrTime {
        XrTime(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, time: XrTime) {
        self.0.store(time.0, Ordering::Relaxed);
    }
}

pub struct SessionReadGuard(pub RwLock<ManuallyDrop<SessionData<DirectX11>>>);

impl SessionReadGuard {
    pub fn new(data: SessionData<DirectX11>) -> Self {
        Self(RwLock::new(ManuallyDrop::new(data)))
    }

    pub fn get(&self) -> std::sync::RwLockReadGuard<'_, ManuallyDrop<SessionData<DirectX11>>> {
        self.0.read().unwrap()
    }

    pub fn get_mut(&self) -> std::sync::RwLockWriteGuard<ManuallyDrop<SessionData<DirectX11>>> {
        self.0.write().unwrap()
    }
}

pub fn get_app_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .and_then(|name| name.to_str())
        .map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::udp_communication::WinlatorPoseData;
    use glam::f32::Vec2;

    fn pose(buttons: [bool; 19], left_thumb: Vec2, right_thumb: Vec2) -> WinlatorPoseData {
        WinlatorPoseData {
            left_hand_quat: Quat::IDENTITY,
            left_hand_thumb: left_thumb,
            left_hand_pos: Vec3::ZERO,
            right_hand_quat: Quat::IDENTITY,
            right_hand_thumb: right_thumb,
            right_hand_pos: Vec3::ZERO,
            hmd_quat: Quat::IDENTITY,
            hmd_pos: Vec3::ZERO,
            ipd: 0.06,
            fov_h: 99.0,
            fov_v: 103.0,
            frame_id: 0,
            buttons,
            immersive_mode: false,
            sbs_mode: false,
        }
    }

    #[test]
    fn resolves_left_grip_and_trigger() {
        let mut b = [false; 19];
        b[0] = true; // left grip
        let p = pose(b, Vec2::ZERO, Vec2::ZERO);

        assert_eq!(
            resolve_action(
                "/actions/set/in/grip".to_string().into_action_name(),
                Some(Hand::Left),
                &p,
            ),
            XrActionValue::Bool(true)
        );
    }

    // Helper: extract an action name from a path string (what `action_name_from_path` does)
    trait IntoActionName {
        fn into_action_name(self) -> Option<String>;
    }
    impl IntoActionName for String {
        fn into_action_name(self) -> Option<String> {
            action_name_from_path(string_to_path_global(&self).0)
        }
    }

    #[test]
    fn resolves_thumbstick_analog_and_click() {
        let buttons = [false; 19];
        let p = pose(buttons, Vec2::new(0.4, -0.6), Vec2::ZERO);

        let analog = resolve_action(
            "/actions/set/in/main-joystick".to_string().into_action_name(),
            Some(Hand::Left),
            &p,
        );
        assert_eq!(
            analog,
            XrActionValue::Vector2(XrVector2f { x: 0.4, y: -0.6 })
        );

        let click = resolve_action(
            "/actions/set/in/thumbstick-click".to_string().into_action_name(),
            Some(Hand::Left),
            &p,
        );
        assert_eq!(click, XrActionValue::Bool(false));
    }

    #[test]
    fn trigger_analog_reflects_button() {
        let mut b = [false; 19];
        b[18] = true; // right trigger
        let p = pose(b, Vec2::ZERO, Vec2::ZERO);
        let v = resolve_action(
            "/actions/set/in/trigger".to_string().into_action_name(),
            Some(Hand::Right),
            &p,
        );
        assert_eq!(v, XrActionValue::Bool(true));
        let f: f32 = convert_value_to_action_state::<f32>(&v);
        assert_eq!(f, 1.0);
    }

#[test]
    fn menu_maps_only_to_left_hand() {
        let mut b = [false; 19];
        b[1] = true; // left menu
        let p = pose(b, Vec2::ZERO, Vec2::ZERO);
        assert_eq!(
            resolve_action(
                "/actions/set/in/app-menu".to_string().into_action_name(),
                Some(Hand::Left),
                &p,
            ),
            XrActionValue::Bool(true)
        );
        assert_eq!(
            resolve_action(
                "/actions/set/in/app-menu".to_string().into_action_name(),
                Some(Hand::Right),
                &p,
            ),
            XrActionValue::Unresolved
        );
    }

    #[test]
    fn curl_from_buttons_follows_grip_trigger_and_thumbstick() {
        let mut b = [false; 19];
        b[7] = true; // left trigger
        let p = pose(b, Vec2::ZERO, Vec2::ZERO);
        let curls = hand_curl_target(Hand::Left, Some(&p));
        assert_eq!(curls[1], 1.0); // index follows trigger
        assert!(curls[2] >= 0.5 && curls[2] < 1.0); // partial bleed into middle

        let mut b = [false; 19];
        b[0] = true; // left grip
        let p = pose(b, Vec2::ZERO, Vec2::ZERO);
        let curls = hand_curl_target(Hand::Left, Some(&p));
        assert_eq!(curls, [0.0, 0.0, 1.0, 1.0, 1.0]);

        let mut b = [false; 19];
        b[13] = true; // right thumbstick click
        let p = pose(b, Vec2::ZERO, Vec2::ZERO);
        let curls = hand_curl_target(Hand::Right, Some(&p));
        assert_eq!(curls[0], 1.0); // thumb
        assert_eq!(curls[1..], [0.0, 0.0, 0.0, 0.0]);

        assert_eq!(hand_curl_target(Hand::Left, None), [0.0; 5]);
    }

    #[test]
    fn real_session_creation_sends_startup_packet() {
        let instance = Instance::new().unwrap();
        let system_id = instance.system_id;
        let create_info = SessionCreateInfo {
            system_id,
            graphics_binding: GraphicsBinding::Vulkan(VulkanGraphicsBinding {
                instance: ash::vk::Instance::null(),
                physical_device: ash::vk::PhysicalDevice::null(),
                device: ash::vk::Device::null(),
                queue_family_index: 0,
                queue_index: 0,
            }),
        };
        let data = SessionData::<crate::graphics_backends::DirectX11>::new(
            &instance,
            system_id,
            vr::ETrackingUniverseOrigin::Standing,
            Some(create_info),
        )
        .unwrap();
        assert!(data.is_real_session());
        // Drop joins the pose receiver thread spawned by the session.
    }
}
