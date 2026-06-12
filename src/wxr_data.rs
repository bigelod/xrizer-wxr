use crate::{
    clientcore::{Injected, Injector},
    graphics_backends::{GraphicsBackend, VulkanData, supported_apis_enum},
    winlatorxr::*,
};
use derive_more::Deref;
use glam::f32::{Quat, Vec3};
use log::{info, warn};
use openvr as vr;
use std::mem::ManuallyDrop;
use std::sync::{
    RwLock,
    atomic::{AtomicI64, Ordering},
};
use std::time::Duration;

pub trait Compositor: vr::InterfaceImpl {
    fn post_session_restart(
        &self,
        session: &SessionData<crate::winlatorxr::DirectX11>,
        waiter: FrameWaiter,
        stream: FrameStream<crate::winlatorxr::DirectX11>,
    );

    fn get_session_create_info(
        &self,
        data: crate::compositor::CompositorSessionData,
    ) -> SessionCreateInfo;

    #[cfg(test)]
    fn on_restart(&self) {}
}

pub type RealWxrData = WxrData<crate::compositor::Compositor>;
pub type SessionReadGuard = RwLock<ManuallyDrop<SessionData<crate::winlatorxr::DirectX11>>>;

impl<C: Compositor> Drop for WxrData<C> {
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

fn make_version() -> u32 {
    env!("CARGO_PKG_VERSION_MAJOR").parse::<u32>().unwrap_or(0) * 1000000
        + env!("CARGO_PKG_VERSION_MINOR").parse::<u32>().unwrap_or(0) * 1000
        + env!("CARGO_PKG_VERSION_PATCH").parse::<u32>().unwrap_or(1)
}

impl<C: Compositor> WxrData<C> {
    pub fn new(injector: &Injector) -> std::result::Result<Self, InitError> {
        let instance = Instance::new().map_err(|e| InitError::InitializationFailed(e.to_string()))?;

        let system_id = instance.system_id;

        let session_data = RwLock::new(ManuallyDrop::new(
            SessionData::new(
                &instance,
                system_id,
                vr::ETrackingUniverseOrigin::Standing,
                None,
            )?
            .0,
        ));

        let display_time = instance.now().0;

        let mut enabled_extensions = ExtensionSet::default();
        enabled_extensions.visibility_mask = false;
        enabled_extensions.display_refresh_rate = true;

        Ok(Self {
            instance,
            system_id,
            session_data,
            display_time: AtomicXrTime(display_time.into()),
            display_period_nanos: 11111111.into(),
            enabled_extensions,
            input: injector.inject(),
            compositor: injector.inject(),
        })
    }

    pub fn poll_events(&self) {
        let data = self.session_data.get();
        if let Some(state) = self.poll_events_impl(&data) {
            drop(data);
            self.session_data.0.write().unwrap().state = state;
        }
    }

    fn poll_events_impl(&self, _data: &SessionData<G>) -> Option<SessionState> {
        None
    }

    pub fn restart_session(&self) {
        let create_info = self
            .compositor
            .get_session_create_info(crate::compositor::CompositorSessionData {
                session: self.session_data.get().session.clone(),
            });

        let new_session_data = SessionData::new(
            &self.instance,
            self.system_id,
            vr::ETrackingUniverseOrigin::Standing,
            Some(create_info),
        );

        match new_session_data {
            Ok(new_data) => {
                let mut guard = self.session_data.0.write().unwrap();
                *guard = new_data.0;
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

    pub fn end_session(&self, data: &mut SessionData<G>) {
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
    pub fn new(
        _instance: &Instance,
        system_id: SystemId,
        _universe_origin: vr::ETrackingUniverseOrigin,
        create_info: Option<SessionCreateInfo>,
    ) -> std::result::Result<Self, SessionCreationError> {
        let create_info = create_info.unwrap_or(SessionCreateInfo {
            system_id,
            graphics_binding: GraphicsBinding::Vulkan(VulkanGraphicsBinding {
                instance: ash::Instance::null(),
                physical_device: ash::vk::PhysicalDevice::null(),
                device: ash::Device::null(),
                queue_family_index: 0,
                queue_index: 0,
            }),
        });

        let session = Session::new(create_info)?;
        let state = session.state;

        Ok(Self {
            session: Some(session),
            state,
            frame_waiter: None,
            frame_stream: None,
            input_data: crate::input::InputSessionData::default(),
            comp_data: crate::compositor::CompositorSessionData(std::sync::Mutex::new(None)),
        })
    }

    pub fn is_real_session(&self) -> bool {
        self.session.is_some()
    }
}

pub struct AtomicXrTime(AtomicI64);

impl AtomicXrTime {
    pub fn new(time: i64) -> Self {
        Self(AtomicI64::new(time))
    }

    pub fn get(&self) -> XrTime {
        XrTime(self.0.load(Ordering::Relaxed))
    }

    pub fn set(&self, time: i64) {
        self.0.store(time, Ordering::Relaxed);
    }
}

impl From<i64> for AtomicXrTime {
    fn from(value: i64) -> Self {
        Self::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_creation() {
        let instance = Instance::new().unwrap();
        let create_info = SessionCreateInfo {
            system_id: instance.system_id,
            graphics_binding: GraphicsBinding::Vulkan(VulkanGraphicsBinding {
                instance: ash::Instance::null(),
                physical_device: ash::vk::PhysicalDevice::null(),
                device: ash::Device::null(),
                queue_family_index: 0,
                queue_index: 0,
            }),
        };

        let session = Session::new(create_info);
        assert!(session.is_ok());
    }

    #[test]
    fn test_space_location() {
        let space = Space {
            space_type: ReferenceSpaceType::Local,
        };

        let location = space.locate(&space, XrTime(0));
        assert!(location.position_valid);
        assert!(location.orientation_valid);
    }
}