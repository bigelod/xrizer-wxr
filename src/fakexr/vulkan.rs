//! Test doubles for the Vulkan loader used in test builds.
//!
//! This mirrors the functions the real `ash` loader would dispatch to, so that
//! swapchain and device setup can run without a physical GPU present.

use ash::vk::{self, Handle};
use paste::paste;
use std::ffi::{CStr, c_char};

macro_rules! get_fn {
    ($name:expr => $($fn:ident),+) => {
        match $name {
            $(
                x if x == unsafe {
                    const {
                        CStr::from_bytes_with_nul_unchecked(concat!("vk", stringify!($fn), "\0").as_bytes())
                    }
                } => paste! {
                    Some(unsafe { std::mem::transmute([<$fn:snake>] as vk::[<PFN_vk $fn>]) })
                },
            )+
            _ => None
        }
    }
}

#[allow(clippy::missing_transmute_annotations, clippy::missing_safety_doc)]
pub unsafe extern "system" fn get_instance_proc_addr(
    instance: vk::Instance,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    let name = unsafe { CStr::from_ptr(name) };

    if instance.is_null() {
        get_fn![name => CreateInstance]
    } else {
        get_fn![name =>
            GetPhysicalDeviceQueueFamilyProperties,
            CreateDevice,
            GetDeviceProcAddr,
            GetDeviceQueue,
            DestroyInstance
        ]
    }
}

#[allow(clippy::missing_transmute_annotations)]
extern "system" fn get_device_proc_addr(
    device: vk::Device,
    name: *const c_char,
) -> vk::PFN_vkVoidFunction {
    if device.is_null() {
        None
    } else {
        let name = unsafe { CStr::from_ptr(name) };
        get_fn![name => GetDeviceQueue, DeviceWaitIdle, DestroyDevice]
    }
}

struct Instance;

#[repr(C)]
pub(crate) struct Device {
    debug: u64,
}

impl Device {
    const DEBUG_VAL: u64 = 946;
    pub fn validate(ptr: u64) -> bool {
        if ptr == 0 {
            return false;
        }
        let debug = ptr as *const u64;
        let val = unsafe { *debug };
        val == Device::DEBUG_VAL
    }
}

extern "system" fn create_instance(
    _: *const vk::InstanceCreateInfo<'_>,
    _: *const vk::AllocationCallbacks<'_>,
    instance: *mut vk::Instance,
) -> vk::Result {
    let inst = Box::new(Instance);
    unsafe {
        *instance = vk::Instance::from_raw(Box::into_raw(inst) as u64);
    }
    vk::Result::SUCCESS
}

extern "system" fn destroy_instance(instance: vk::Instance, _: *const vk::AllocationCallbacks<'_>) {
    drop(unsafe { Box::from_raw(instance.as_raw() as *mut Instance) });
}

extern "system" fn create_device(
    _: vk::PhysicalDevice,
    _: *const vk::DeviceCreateInfo,
    _: *const vk::AllocationCallbacks<'_>,
    device: *mut vk::Device,
) -> vk::Result {
    let d = Box::new(Device {
        debug: Device::DEBUG_VAL,
    });
    unsafe {
        *device = vk::Device::from_raw(Box::into_raw(d) as u64);
    }
    vk::Result::SUCCESS
}

extern "system" fn destroy_device(device: vk::Device, _: *const vk::AllocationCallbacks<'_>) {
    drop(unsafe { Box::from_raw(device.as_raw() as *mut Device) });
}

extern "system" fn get_device_queue(
    _: vk::Device,
    queue_family_index: u32,
    queue_index: u32,
    queue: *mut vk::Queue,
) {
    if queue_index == 0 && queue_family_index == 0 {
        unsafe {
            *queue = vk::Queue::from_raw(1);
        }
    }
}

extern "system" fn device_wait_idle(_: vk::Device) -> vk::Result {
    vk::Result::SUCCESS
}

extern "system" fn get_physical_device_queue_family_properties(
    _physical_device: vk::PhysicalDevice,
    queue_family_property_count: *mut u32,
    queue_family_properties: *mut vk::QueueFamilyProperties,
) {
    unsafe { *queue_family_property_count = 1 };
    if let Some(props) = unsafe { queue_family_properties.as_mut() } {
        *props = vk::QueueFamilyProperties {
            queue_count: 1,
            queue_flags: vk::QueueFlags::GRAPHICS,
            ..Default::default()
        };
    }
}
