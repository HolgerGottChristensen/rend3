use crate::{
    instruction::InstructionStreamPair,
    renderer::{
        limits::{check_features, check_limits},
        material::MaterialManager,
        mesh::MeshManager,
        object::ObjectManager,
        resources::RendererGlobalResources,
        shaders::ShaderManager,
        texture::TextureManager,
        Renderer, SWAPCHAIN_FORMAT,
    },
    RendererInitializationError, RendererOptions, TLS,
};
use parking_lot::{Mutex, RwLock};
use raw_window_handle::HasRawWindowHandle;
use std::{cell::RefCell, sync::Arc};
use switchyard::Switchyard;
use wgpu::{BackendBit, DeviceDescriptor, Instance, PowerPreference, RequestAdapterOptions};
use wgpu_conveyor::{AutomatedBufferManager, UploadStyle};

pub async fn create_renderer<W: HasRawWindowHandle, TLD>(
    window: &W,
    yard: Arc<Switchyard<RefCell<TLD>>>,
    imgui: &mut imgui::Context,
    options: RendererOptions,
) -> Result<Arc<Renderer<TLD>>, RendererInitializationError>
where
    TLD: AsMut<TLS> + 'static,
{
    let instance = Instance::new(BackendBit::PRIMARY);

    let surface = unsafe { instance.create_surface(window) };

    let adapter = instance
        .request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
        })
        .await
        .ok_or(RendererInitializationError::MissingAdapter)?;

    let adapter_info = adapter.get_info();
    let features = check_features(adapter.features())?;
    let limits = check_limits(adapter.limits())?;

    let (device, queue) = adapter
        .request_device(
            &DeviceDescriptor {
                features,
                limits,
                shader_validation: true,
            },
            None,
        )
        .await
        .map_err(|_| RendererInitializationError::RequestDeviceFailed)?;

    let mut buffer_manager = Mutex::new(AutomatedBufferManager::new(UploadStyle::from_device_type(
        &adapter_info.device_type,
    )));
    let global_resources = RwLock::new(RendererGlobalResources::new(&device, &surface, &options));
    let shader_manager = ShaderManager::new();
    let mesh_manager = RwLock::new(MeshManager::new(&device));
    let texture_manager = RwLock::new(TextureManager::new(&device));
    let material_manager = RwLock::new(MaterialManager::new(&device, buffer_manager.get_mut()));
    let object_manager = RwLock::new(ObjectManager::new(&device, buffer_manager.get_mut()));

    let imgui_renderer = imgui_wgpu::Renderer::new(imgui, &device, &queue, SWAPCHAIN_FORMAT);

    Ok(Arc::new(Renderer {
        yard,
        instructions: InstructionStreamPair::new(),

        adapter_info,
        queue,
        device,
        surface,

        buffer_manager,
        global_resources,
        shader_manager,
        mesh_manager,
        texture_manager,
        material_manager,
        object_manager,

        imgui_renderer,

        options,
    }))
}