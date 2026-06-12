use super::GraphicsBackend;
use crate::winlatorxr::*;
use openvr as vr;
use std::collections::HashMap;
use std::result::Result as StdResult;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::core::*;

pub type D3D_DRIVER_TYPE = u32;
pub const D3D_DRIVER_TYPE_HARDWARE: D3D_DRIVER_TYPE = 1;
pub const D3D_DRIVER_TYPE_NULL: D3D_DRIVER_TYPE = 0;
pub const D3D_DRIVER_TYPE_WARP: D3D_DRIVER_TYPE = 2;
pub const D3D_DRIVER_TYPE_REFERENCE: D3D_DRIVER_TYPE = 3;
pub const D3D_DRIVER_TYPE_SOFTWARE: D3D_DRIVER_TYPE = 4;

#[derive(Debug, Clone, Copy, Default)]
pub struct Extent2D {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Extent2Di {
    pub width: i32,
    pub height: i32,
}

#[derive(Debug)]
pub enum GraphicsError {
    InitializationFailed(String),
}

pub struct DirectX11Data {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchains: HashMap<u32, SwapchainData>,
}

struct SwapchainData {
    texture: ID3D11Texture2D,
    shader_resource_view: ID3D11ShaderResourceView,
    render_target_view: ID3D11RenderTargetView,
    width: u32,
    height: u32,
}

pub struct DirectX11;

impl Graphics for DirectX11 {
    type SwapchainImage = ID3D11Texture2D;
    type Format = u64;
    type SessionCreateInfo = VulkanSessionCreateInfo;
    type SwapchainCreateInfo = crate::winlatorxr::SwapchainCreateInfo;
}

impl DirectX11Data {
    pub fn new() -> StdResult<Self, GraphicsError> {
        unsafe {
            let mut device = None;
            let mut context = None;

            let result = D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_DEBUG,
                None,
                D3D11_SDK_VERSION,
                &mut device,
                None,
                &mut context,
            );

            if let Err(e) = result {
                return Err(GraphicsError::InitializationFailed(e.to_string()));
            }

            Ok(Self {
                device: device.unwrap(),
                context: context.unwrap(),
                swapchains: HashMap::new(),
            })
        }
    }

    unsafe fn get_texture_extent(&self, texture: &vr::Texture_t) -> Extent2D {
        let d3d_texture = self.get_d3d11_texture(texture);
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        d3d_texture.GetDesc(&mut desc);

        Extent2D {
            width: desc.Width,
            height: desc.Height,
        }
    }

    unsafe fn get_d3d11_texture(&self, texture: &vr::Texture_t) -> &ID3D11Texture2D {
        &*(texture.handle as *const ID3D11Texture2D)
    }
}

impl GraphicsBackend for DirectX11Data {
    type Api = DirectX11;
    type OpenVrTexture = vr::Texture_t;
    type NiceFormat = u64;

    fn to_nice_format(format: <Self::Api as Graphics>::Format) -> Self::NiceFormat {
        format
    }

    fn session_create_info(&self) -> <Self::Api as Graphics>::SessionCreateInfo {
        todo!("DirectX11 session creation info")
    }

    fn get_texture(texture: &vr::Texture_t) -> Option<Self::OpenVrTexture> {
        if !texture.handle.is_null() {
            Some(*texture)
        } else {
            None
        }
    }

    fn swapchain_info_for_texture(
        &self,
        texture: Self::OpenVrTexture,
        _bounds: vr::VRTextureBounds_t,
        _color_space: vr::EColorSpace,
    ) -> <Self::Api as Graphics>::SwapchainCreateInfo {
        let extent = unsafe { self.get_texture_extent(&texture) };

        <Self::Api as Graphics>::SwapchainCreateInfo {
            width: extent.width,
            height: extent.height,
            format: 28, // DXGI_FORMAT_R8G8B8A8_UNORM as u64
            sample_count: 1,
            create_flags: SwapchainCreateFlags(0),
            usage_flags: SwapchainUsageFlags { bits: 3 },
            face_count: 1,
            array_size: 2,
            mip_count: 1,
        }
    }

    fn store_swapchain_images(
        &mut self,
        _images: Vec<<Self::Api as Graphics>::SwapchainImage>,
        _format: <Self::Api as Graphics>::Format,
    ) {
        // Store swapchain images for DirectX 11
        todo!("Store swapchain images for DirectX 11")
    }

    fn copy_texture_to_swapchain(
        &self,
        _eye: vr::EVREye,
        _texture: Self::OpenVrTexture,
        _color_space: vr::EColorSpace,
        _bounds: vr::VRTextureBounds_t,
        _image_index: usize,
        _submit_flags: vr::EVRSubmitFlags,
    ) -> Extent2Di {
        unsafe {
            let src_texture = self.get_d3d11_texture(&_texture);
            let dst_swapchain = self.swapchains.get(&(_image_index as u32));
            
            if let Some(swapchain) = dst_swapchain {
                let src_box = D3D11_BOX {
                    left: (_bounds.uMin * swapchain.width as f32) as u32,
                    top: (_bounds.vMin * swapchain.height as f32) as u32,
                    front: 0,
                    right: (_bounds.uMax * swapchain.width as f32) as u32,
                    bottom: (_bounds.vMax * swapchain.height as f32) as u32,
                    back: 1,
                };

                self.context.CopySubresourceRegion(
                    &swapchain.texture,
                    0,
                    0, 0, 0,
                    src_texture,
                    0,
                    &src_box,
                );

                Extent2Di {
                    width: swapchain.width as i32,
                    height: swapchain.height as i32,
                }
            } else {
                Extent2Di {
                    width: 0,
                    height: 0,
                }
            }
        }
    }

    fn copy_overlay_to_swapchain(
        &mut self,
        _texture: Self::OpenVrTexture,
        _bounds: vr::VRTextureBounds_t,
        _image_index: usize,
    ) -> Extent2Di {
        todo!("Copy overlay to swapchain for DirectX 11")
    }
}

impl DirectX11Data {
    pub fn create_swapchain(&mut self, create_info: SwapchainCreateInfo<DirectX11>) -> StdResult<u64, GraphicsError> {
        unsafe {
            let desc = D3D11_TEXTURE2D_DESC {
                Width: create_info.width,
                Height: create_info.height,
                MipLevels: create_info.mip_count,
                ArraySize: 1,
                Format: unsafe { std::mem::transmute_copy(&create_info.format) },
                SampleDesc: DXGI_SAMPLE_DESC {
                    Count: create_info.sample_count,
                    Quality: 0,
                },
                Usage: D3D11_USAGE_DEFAULT,
                BindFlags: D3D11_BIND_SHADER_RESOURCE | D3D11_BIND_RENDER_TARGET,
                CPUAccessFlags: 0u32,
                MiscFlags: 0u32,
            };

            let mut texture = None;
            self.device.CreateTexture2D(&desc, None, &mut texture)
                .map_err(|e| GraphicsError::InitializationFailed(e.to_string()))?;

            let srv_desc = D3D11_SHADER_RESOURCE_VIEW_DESC {
                Format: desc.Format,
                ViewDimension: D3D11_SRV_DIMENSION(10), // D3D11_SRV_DIMENSION_TEXTURE2D
                Anonymous: Default::default(),
            };

            let mut srv = None;
            self.device.CreateShaderResourceView(&texture.as_ref().unwrap(), &srv_desc, &mut srv)
                .map_err(|e| GraphicsError::InitializationFailed(e.to_string()))?;

            let mut rtv = None;
            self.device.CreateRenderTargetView(&texture.as_ref().unwrap(), None, &mut rtv)
                .map_err(|e| GraphicsError::InitializationFailed(e.to_string()))?;

            let swapchain_id = self.swapchains.len() as u64;
            self.swapchains.insert(swapchain_id as u32, SwapchainData {
                texture: texture.unwrap(),
                shader_resource_view: srv.unwrap(),
                render_target_view: rtv.unwrap(),
                width: create_info.width,
                height: create_info.height,
            });

            Ok(swapchain_id)
        }
    }
}