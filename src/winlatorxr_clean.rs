use derive_more::Deref;
use glam::f32::{Quat, Vec2, Vec3};
use std::mem::ManuallyDrop;
use std::sync::{
    RwLock,
    atomic::{AtomicI64, AtomicBool, Ordering},
};
use std::thread;

#[cfg(windows)]
use windows::Win32::Graphics::Direct3D11::ID3D11Texture2D;

use crate::clientcore::Injected;
use crate::graphics_backends::Extent2Di;

pub trait ActionTy {}

pub struct HapticTy;

impl ActionTy for bool {}
impl ActionTy for f32 {}
impl ActionTy for XrVector2f {}
impl ActionTy for HapticTy {}

pub type Posef = XrPosef;
pub type Vector2f = XrVector2f;

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

pub struct Vulkan;

impl Graphics for Vulkan {
    type SessionCreateInfo = VulkanSessionCreateInfo;
    type Format = u64;
    type SwapchainImage = SwapchainImageVulkanKHR;
    type SwapchainCreateInfo = VulkanSwapchainCreateInfoKHR;
}

pub use crate::graphics_backends::DirectX11;

impl Graphics for DirectX11 {
    type SwapchainImage = ID3D11Texture2D;
    type Format = u64;
    type SessionCreateInfo = VulkanSessionCreateInfo;
    type SwapchainCreateInfo = SwapchainCreateInfo<DirectX11>;
}

pub trait Compositor: openvr::InterfaceImpl {
    fn post_session_restart(
        &self,
        session: &SessionData<DirectX11>,
        waiter: FrameWaiter<DirectX11>,
        stream: FrameStream<DirectX11>,
    );

    fn get_session_create_info(
        &self,
        data: crate::compositor::CompositorSessionData,
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
    pub session_data: SessionReadGuard,
    pub display_time: AtomicXrTime,
    pub display_period_nanos: AtomicI64,
    pub enabled_extensions: ExtensionSet,

    pub(crate) input: Injected<crate::input::Input<C>>,
    pub(crate) compositor: Injected<C>,
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

    pub fn string_to_path(&self, _path: &str) -> Result<Path> {
        Ok(Path(0))
    }

    pub fn vulkan_graphics_device(&self, _system_id: SystemId, _instance: ash::Instance) -> ash::vk::PhysicalDevice {
        ash::vk::PhysicalDevice::null()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SystemId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct XrTime(pub i64);

impl XrTime {
    pub const ZERO: Self = Self(0);
}

pub type Time = XrTime;

#[derive(Debug, Clone, Copy, Default)]
pub struct XrPosef {
    pub orientation: Quat,
    pub position: Vec3,
}

#[derive(Debug, Clone, Copy)]
pub struct XrVector3f {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, Copy)]
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

#[derive(Debug, Clone, Copy)]
pub struct ViewStateFlags {
    pub position_valid: bool,
    pub orientation_valid: bool,
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
pub struct SwapchainCreateFlags;

impl SwapchainCreateFlags {
    pub const EMPTY: Self = Self;
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
}

impl<G> SessionData<G> {
    pub fn is_real_session(&self) -> bool {
        self.session.is_some()
    }

    pub fn begin_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn end_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn wait_frame(&mut self) -> Result<(FrameWaiter, XrTime), SessionCreationError> {
        Ok((FrameWaiter, XrTime(0)))
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

pub struct Session<G> {
    pub system_id: SystemId,
    pub state: SessionState,
    pub udp: std::sync::Arc<crate::udp_communication::UdpComm>,
    pub latest_pose_data: std::sync::Arc<RwLock<Option<crate::udp_communication::WinlatorPoseData>>>,
    pub pose_receiver_thread: Option<thread::JoinHandle<()>>,
    pub shutdown_signal: std::sync::Arc<AtomicBool>,
    _phantom: std::marker::PhantomData<G>,
}

pub struct VulkanGraphicsBinding {
    pub instance: ash::Instance,
    pub physical_device: ash::vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue_family_index: u32,
    pub queue_index: u32,
}

pub struct OpenGLGraphicsBinding {
    pub context: *mut std::ffi::c_void,
    pub display: *mut std::ffi::c_void,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenGLSessionCreateInfo {
    pub h_dc: *mut std::ffi::c_void,
    pub h_glrc: *mut std::ffi::c_void,
}

#[derive(Debug, Clone)]
pub struct VulkanSessionCreateInfo {
    pub instance: ash::Instance,
    pub physical_device: ash::vk::PhysicalDevice,
    pub device: ash::Device,
    pub queue_family_index: u32,
    pub queue_index: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct VulkanSwapchainCreateInfoKHR {
    pub create_info: SwapchainCreateInfo<Vulkan>,
}

#[derive(Debug, Clone, Copy)]
pub struct OpenGLSwapchainCreateInfoKHR {
    pub create_info: SwapchainCreateInfo<u32>,
}

#[derive(Debug, Clone)]
pub struct ExtensionSet {
    pub hand_tracking: bool,
    pub visibility_mask: bool,
    pub display_refresh_rate: bool,
    pub composition_layer_cylinder: bool,
    pub composition_layer_equirect: bool,
    pub composition_layer_color_scale_bias: bool,
}

impl Default for ExtensionSet {
    fn default() -> Self {
        Self {
            hand_tracking: false,
            visibility_mask: false,
            display_refresh_rate: false,
            composition_layer_cylinder: false,
            composition_layer_equirect: false,
            composition_layer_color_scale_bias: false,
        }
    }
}

#[derive(Debug)]
pub enum SessionCreationError {
    GraphicsBindingRequired,
    InvalidGraphicsBinding,
    InitializationFailed(String),
}

pub struct SessionCreateInfo {
    pub system_id: SystemId,
    pub graphics_binding: GraphicsBinding,
}

pub enum GraphicsBinding {
    Vulkan(VulkanGraphicsBinding),
    OpenGL(OpenGLGraphicsBinding),
}

#[derive(Debug, Clone)]
pub struct GraphicalSession<G> {
    pub session: GenericSession<G>,
}

#[derive(Debug, Clone)]
pub struct GenericSession<G> {
    pub session: SessionData<G>,
    pub graphics: G,
}

pub type GraphicalSessionType = GraphicalSession<ash::vk::PhysicalDevice>;

#[derive(Debug, Clone)]
pub struct CreateInfo<G> {
    pub system_id: SystemId,
    pub graphics_binding: GraphicsBinding,
}

impl<G> CreateInfo<G> {
    pub fn new(system_id: SystemId, graphics_binding: GraphicsBinding) -> Self {
        Self {
            system_id,
            graphics_binding,
        }
    }
}

impl<G> From<GraphicsBinding> for SessionCreateInfo {
    fn from(binding: GraphicsBinding) -> Self {
        SessionCreateInfo {
            system_id: SystemId(0),
            graphics_binding: binding,
        }
    }
}

impl<G> Session<G> {
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
            pose_receiver_thread: Some(thread),
            shutdown_signal,
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
        self.latest_pose_data.read().unwrap().clone()
    }

    fn store_button_states(&self, buttons: [bool; 19]) {
        // Store button states for action queries
        // This will be implemented later for input handling
    }

    fn store_thumbstick_axes(&self, hand: Hand, axes: Vec2) {
        // Store thumbstick axes for action queries
        // This will be implemented later for input handling
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
        })
    }

    pub fn create_swapchain(&self, _create_info: SwapchainCreateInfo<G>) -> Result<Swapchain<G>, SessionCreationError> {
        Ok(Swapchain {
            width: _create_info.width,
            height: _create_info.height,
            format: _create_info.format,
            sample_count: _create_info.sample_count,
            create_flags: _create_info.create_flags,
            usage_flags: _create_info.usage_flags,
            _phantom: std::marker::PhantomData,
        })
    }

    pub fn wait_frame(&mut self) -> Result<(FrameWaiter, XrTime), SessionCreationError> {
        Ok((FrameWaiter, XrTime(0)))
    }

    pub fn begin_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn end_frame(&mut self, _layers: &[CompositionLayerBase<G>]) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn sync_actions(&self, _action_sets: &[ActiveActionSet]) -> Result<(), SessionCreationError> {
        if let Some(pose) = self.get_latest_pose() {
            self.store_button_states(pose.buttons);
            self.store_thumbstick_axes(Hand::Left, pose.left_hand_thumb);
            self.store_thumbstick_axes(Hand::Right, pose.right_hand_thumb);
        }
        Ok(())
    }

    pub fn send_haptic(&self, data: crate::udp_communication::WinlatorHapticData) -> Result<(), SessionCreationError> {
        self.udp.send_haptic(&data).map_err(|e| SessionCreationError::InitializationFailed(e.to_string()))
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
        })
    }

    pub fn create_swapchain(&self, _create_info: SwapchainCreateInfo<G>) -> Result<Swapchain<G>, SessionCreationError> {
        Ok(Swapchain {
            width: _create_info.width,
            height: _create_info.height,
            format: _create_info.format,
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
                    return Ok((FrameWaiter, std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_nanos() as i64).into());
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn begin_frame(&mut self) -> Result<(), SessionCreationError> {
        Ok(())
    }

    pub fn end_frame(&mut self, _layers: &[CompositionLayerBase<G>]) -> Result<(), SessionCreationError> {
        let is_sbs = _layers.iter().any(|layer| matches!(layer, CompositionLayerBase::Projection(_)));

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

pub struct Space {
    pub space_type: ReferenceSpaceType,
}

impl Space {
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

#[derive(Debug, Clone, Copy)]
pub struct SpaceLocation {
    pub pose: XrPosef,
    pub position_valid: bool,
    pub orientation_valid: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SpaceRelation {
    pub pose: XrPosef,
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub position_valid: bool,
    pub orientation_valid: bool,
    pub linear_velocity_valid: bool,
    pub angular_velocity_valid: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct SwapchainCreateInfo<G> {
    pub width: u32,
    pub height: u32,
    pub format: u64,
    pub sample_count: u32,
    pub create_flags: SwapchainCreateFlags,
    pub usage_flags: SwapchainUsageFlags,
    pub face_count: u32,
    pub array_size: u32,
    pub mip_count: u32,
    _phantom: std::marker::PhantomData<G>,
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

pub struct Swapchain<G> {
    pub width: u32,
    pub height: u32,
    pub format: u64,
    pub sample_count: u32,
    pub create_flags: SwapchainCreateFlags,
    pub usage_flags: SwapchainUsageFlags,
    _phantom: std::marker::PhantomData<G>,
}

impl Swapchain {
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

pub struct FrameWaiter;

impl FrameWaiter {
    pub fn wait(&mut self) -> Result<XrTime, SessionCreationError> {
        Ok(XrTime(0))
    }
}

pub struct FrameStream<G> {
    _phantom: std::marker::PhantomData<G>,
}

pub enum CompositionLayerBase<G> {
    Projection(CompositionLayerProjection<G>),
    Quad(CompositionLayerQuad<G>),
    Cylinder(CompositionLayerCylinder<G>),
    Equirect(CompositionLayerEquirect<G>),
}

pub struct CompositionLayerProjection<G> {
    pub views: Vec<CompositionLayerProjectionView<G>>,
}

pub struct CompositionLayerProjectionView<G> {
    pub pose: XrPosef,
    pub fov: XrFovf,
    pub sub_image: SwapchainSubImage<G>,
    _phantom: std::marker::PhantomData<G>,
}

pub struct CompositionLayerQuad<G> {
    pub pose: XrPosef,
    pub size: XrExtent2Df,
    pub sub_image: SwapchainSubImage<G>,
    _phantom: std::marker::PhantomData<G>,
}

pub struct CompositionLayerCylinder<G> {
    pub pose: XrPosef,
    pub radius: f32,
    pub central_angle: f32,
    pub aspect_ratio: f32,
    pub sub_image: SwapchainSubImage<G>,
    _phantom: std::marker::PhantomData<G>,
}

pub struct CompositionLayerEquirect<G> {
    pub pose: XrPosef,
    pub radius: f32,
    pub central_horizontal_angle: f32,
    pub upper_vertical_angle: f32,
    pub lower_vertical_angle: f32,
    pub sub_image: SwapchainSubImage<G>,
    _phantom: std::marker::PhantomData<G>,
}

pub struct SwapchainSubImage<G> {
    pub swapchain: Swapchain<G>,
    pub image_rect: Rect2D,
    _phantom: std::marker::PhantomData<G>,
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

    pub fn create_action<T>(&self, name: &str, _localized_name: &str, _subaction_path: Option<Path>) -> Action<T> {
        Action {
            action_type: std::any::TypeId::of::<T>(),
            name: name.to_string(),
            path: Path(0), // Simplified path generation
        }
    }

    pub fn suggest_interaction_profile_bindings(&self, _profile: Path, _bindings: &[Path]) -> Result<(), SessionCreationError> {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Path(pub u64);

impl Path {
    pub const NULL: Self = Self(0);
}

pub struct Action<T> {
    pub action_type: std::any::TypeId,
    pub name: String,
    pub path: Path,
}

impl<T> Action<T> {
    pub fn state(&self, _session: &Session<G>, _subaction_path: Option<Path>) -> ActionState<T> {
        ActionState::default()
    }

    pub fn unwrap(self) -> Path {
        self.path
    }

    pub fn apply_feedback(&self, session: &Session<G>, haptic: HapticVibration) -> std::result::Result<(), SessionCreationError> {
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

pub struct ActionState<T> {
    pub current_state: T,
    pub changed_since_last_sync: bool,
    pub active: bool,
    pub last_change_time: XrTime,
}

impl<T> Default for ActionState<T> {
    fn default() -> Self {
        Self {
            current_state: unsafe { std::mem::zeroed() },
            changed_since_last_sync: false,
            active: false,
            last_change_time: XrTime(0),
        }
    }
}

impl<T> ActionState<T> {
    pub fn unwrap(self) -> T {
        self.current_state
    }
}

impl ActionState<bool> {
    pub fn state(&self) -> bool {
        self.current_state
    }

    pub fn changed_since_last_sync(&self) -> bool {
        self.changed_since_last_sync
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl ActionState<f32> {
    pub fn current(&self) -> f32 {
        self.current_state
    }

    pub fn changed_since_last_sync(&self) -> bool {
        self.changed_since_last_sync
    }

    pub fn is_active(&self) -> bool {
        self.active
    }
}

impl ActionState<XrVector2f> {
    pub fn current(&self) -> XrVector2f {
        self.current_state
    }

    pub fn changed_since_last_sync(&self) -> bool {
        self.changed_since_last_sync
    }

    pub fn is_active(&self) -> bool {
        self.active
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

pub enum Hand {
    Left,
    Right,
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

pub struct SpaceVelocity {
    pub linear_velocity: Vec3,
    pub angular_velocity: Vec3,
    pub velocity_valid: bool,
    pub angular_velocity_valid: bool,
}

pub enum HandJoint {
    Palm,
    Wrist,
    ThumbMetacarpal,
    ThumbProximal,
    ThumbDistal,
    ThumbTip,
    IndexMetacarpal,
    IndexProximal,
    IndexIntermediate,
    IndexDistal,
    IndexTip,
    MiddleMetacarpal,
    MiddleProximal,
    MiddleIntermediate,
    MiddleDistal,
    MiddleTip,
    RingMetacarpal,
    RingProximal,
    RingIntermediate,
    RingDistal,
    RingTip,
    LittleMetacarpal,
    LittleProximal,
    LittleIntermediate,
    LittleDistal,
    LittleTip,
}

pub struct HandTracker {
    pub hand: Hand,
}

impl HandTracker {
    pub fn new(_session: &Session, hand: Hand) -> Result<Self, SessionCreationError> {
        Ok(Self { hand })
    }

    pub fn locate_hand_joints(&self, _base_space: &Space, _time: XrTime) -> Result<HandJointLocations, SessionCreationError> {
        Ok(HandJointLocations {
            joint_count: 0,
            joint_locations: vec![],
            joint_radii: vec![],
        })
    }
}

pub struct HandJointLocations {
    pub joint_count: u32,
    pub joint_locations: Vec<HandJointLocation>,
    pub joint_radii: Vec<f32>,
}

pub struct HandJointLocation {
    pub pose: XrPosef,
    pub radius: f32,
}

pub enum CompositionLayerFlags {
    Corrupted = 1,
    Untrimmed = 2,
    Blended = 4,
}

pub enum StructureType {
    CompositionLayerCylinderKHR = 1000015000,
    CompositionLayerEquirectKHR = 1000015001,
    CompositionLayerColorScaleBiasKHR = 1000016000,
}

pub mod sys {
    pub type Action<T> = super::Action<T>;
    pub type Path = super::Path;
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
    pub type Session = super::Session;
    pub type SystemId = super::SystemId;
    pub type SessionState = super::SessionState;
    pub type Instance = super::Instance;
    pub type InitError = super::InitError;
    pub type HapticVibration = super::HapticVibration;
    pub type Hand = super::Hand;
    pub type Binding = super::Binding;
    pub type ActiveActionSet = super::ActiveActionSet;
    pub type ViewState = super::ViewState;
    pub type FrameWaiter = super::FrameWaiter;
    pub type FrameStream<G> = super::FrameStream<G>;
    pub type SwapchainImageVulkanKHR = super::SwapchainImageVulkanKHR;
    pub type SwapchainImageOpenGLKHR = super::SwapchainImageOpenGLKHR;
    pub type GraphicalSession<G> = super::GraphicalSession<G>;
    pub type GenericSession<G> = super::GenericSession<G>;
    pub type CreateInfo<G> = super::CreateInfo<G>;
    pub type GraphicsBinding = super::GraphicsBinding;
    pub type VulkanGraphicsBinding = super::VulkanGraphicsBinding;
    pub type OpenGLGraphicsBinding = super::OpenGLGraphicsBinding;
    pub type VulkanSessionCreateInfo = super::VulkanSessionCreateInfo;
    pub type OpenGLSessionCreateInfo = super::OpenGLSessionCreateInfo;
    pub type VulkanSwapchainCreateInfoKHR = super::VulkanSwapchainCreateInfoKHR;
    pub type OpenGLSwapchainCreateInfoKHR = super::OpenGLSwapchainCreateInfoKHR;
    pub type VulkanSwapchainImageKHR = super::VulkanSwapchainImageKHR;
    pub type OpenGLSwapchainImageKHR = super::OpenGLSwapchainImageKHR;
    pub type VisibilityMaskType = super::VisibilityMaskType;
    pub type VisibilityMaskKHR = super::VisibilityMaskKHR;

    pub enum Result {
        Success,
        Error,
    }
}

pub enum EyeVisibility {
    Both,
    Left,
    Right,
}

pub struct CompositionLayerColorScaleBiasKHR {
    pub base: BaseInStructure,
    pub color_scale: XrColor4f,
    pub color_bias: XrColor4f,
}

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

impl CompositionLayerCylinder {
    pub unsafe fn from_raw(raw: *const std::ffi::c_void) -> Self {
        Self {
            pose: XrPosef::IDENTITY,
            radius: 0.0,
            central_angle: 0.0,
            aspect_ratio: 0.0,
            sub_image: SwapchainSubImage {
                swapchain: Swapchain {
                    width: 0,
                    height: 0,
                    format: 0,
                    sample_count: 0,
                    create_flags: SwapchainCreateFlags,
                    usage_flags: SwapchainUsageFlags { bits: 0 },
                },
                image_rect: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D {
                        width: 0,
                        height: 0,
                    },
                },
            },
        }
    }
}

impl CompositionLayerEquirect {
    pub unsafe fn from_raw(raw: *const std::ffi::c_void) -> Self {
        Self {
            pose: XrPosef::IDENTITY,
            radius: 0.0,
            central_horizontal_angle: 0.0,
            upper_vertical_angle: 0.0,
            lower_vertical_angle: 0.0,
            sub_image: SwapchainSubImage {
                swapchain: Swapchain {
                    width: 0,
                    height: 0,
                    format: 0,
                    sample_count: 0,
                    create_flags: SwapchainCreateFlags,
                    usage_flags: SwapchainUsageFlags { bits: 0 },
                },
                image_rect: Rect2D {
                    offset: Offset2D { x: 0, y: 0 },
                    extent: Extent2D {
                        width: 0,
                        height: 0,
                    },
                },
            },
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

pub struct SessionReadGuard(pub RwLock<SessionData<DirectX11>>);

impl SessionReadGuard {
    pub fn new(data: SessionData<DirectX11>) -> Self {
        Self(RwLock::new(data))
    }

    pub fn get(&self) -> SessionData<DirectX11> {
        self.0.read().unwrap().clone()
    }

    pub fn get_mut(&self) -> std::sync::RwLockWriteGuard<SessionData<DirectX11>> {
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
