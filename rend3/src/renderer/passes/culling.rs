use crate::{
    renderer::shaders::{ShaderArguments, ShaderManager},
    TLS,
};
use shaderc::ShaderKind;
use std::{cell::RefCell, future::Future, sync::Arc};
use switchyard::Switchyard;
use tracing_futures::Instrument;
use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindingResource, Buffer, BufferAddress,
    BufferDescriptor, BufferUsage, ComputePass, ComputePipeline, ComputePipelineDescriptor, Device,
    PipelineLayoutDescriptor, ProgrammableStageDescriptor, PushConstantRange, ShaderModule, ShaderStage,
};

const SIZE_OF_OUTPUT_DATA: BufferAddress = 7 * 16;
const SIZE_OF_INDIRECT_CALL: BufferAddress = 5 * 4;
const SIZE_OF_INDIRECT_COUNT: BufferAddress = 4;

pub struct CullingPassData {
    name: String,
    bind_group: BindGroup,
    object_count: u32,
}

pub struct CullingPass {
    pipeline: ComputePipeline,
    shader: Arc<ShaderModule>,
    subgroup_size: u32,
}
impl CullingPass {
    pub fn new<'a, TLD>(
        device: &'a Arc<Device>,
        yard: &Switchyard<RefCell<TLD>>,
        shader_manager: &Arc<ShaderManager>,
        input_bgl: &BindGroupLayout,
        output_bgl: &BindGroupLayout,
        uniform_bgl: &BindGroupLayout,
        subgroup_size: u32,
    ) -> impl Future<Output = Self> + 'a
    where
        TLD: AsMut<TLS> + 'static,
    {
        let new_span = tracing::warn_span!("Creating CullingPass");
        let new_span_guard = new_span.enter();

        let shader = shader_manager.compile_shader(
            &yard,
            Arc::clone(device),
            ShaderArguments {
                file: String::from("rend3/shaders/cull.comp"),
                defines: vec![(String::from("WARP_SIZE"), Some(subgroup_size.to_string()))],
                kind: ShaderKind::Compute,
                debug: true,
            },
        );

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("culling pipeline layout"),
            bind_group_layouts: &[input_bgl, output_bgl, uniform_bgl],
            push_constant_ranges: &[PushConstantRange {
                range: 0..4,
                stages: ShaderStage::COMPUTE,
            }],
        });

        drop(new_span_guard);

        async move {
            let shader = shader.await.unwrap();

            let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
                label: Some("culling pipeline"),
                layout: Some(&pipeline_layout),
                compute_stage: ProgrammableStageDescriptor {
                    module: &shader,
                    entry_point: "main",
                },
            });

            Self {
                pipeline,
                shader,
                subgroup_size,
            }
        }
        .instrument(new_span)
    }

    pub fn prepare(
        &self,
        device: &Device,
        output_bgl: &BindGroupLayout,
        object_count: u32,
        name: String,
    ) -> CullingPassData {
        span_transfer!(_ -> prepare_span, WARN, "Preparing CullingPass");

        let output_buffer = device.create_buffer(&BufferDescriptor {
            label: Some(&*format!("object output buffer for {}", &name)),
            size: SIZE_OF_OUTPUT_DATA * object_count as BufferAddress,
            usage: BufferUsage::STORAGE,
            mapped_at_creation: false,
        });

        let indirect_buffer = device.create_buffer(&BufferDescriptor {
            label: Some(&*format!("indirect buffer for {}", &name)),
            size: SIZE_OF_INDIRECT_CALL * object_count as BufferAddress,
            usage: BufferUsage::STORAGE | BufferUsage::INDIRECT,
            mapped_at_creation: false,
        });

        let count_buffer = device.create_buffer(&BufferDescriptor {
            label: Some(&*format!("count buffer for {}", &name)),
            size: SIZE_OF_INDIRECT_COUNT,
            usage: BufferUsage::STORAGE | BufferUsage::INDIRECT,
            mapped_at_creation: true,
        });

        count_buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck::bytes_of(&0));
        count_buffer.unmap();

        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some(&*format!("output bind group for {}", &name)),
            layout: output_bgl,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(output_buffer.slice(..)),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Buffer(indirect_buffer.slice(..)),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Buffer(count_buffer.slice(..)),
                },
            ],
        });

        CullingPassData {
            name,
            bind_group,
            object_count,
        }
    }

    pub fn run<'a>(
        &'a self,
        compute_pass: &mut ComputePass<'a>,
        input_bg: &'a BindGroup,
        uniform_bg: &'a BindGroup,
        data: &'a CullingPassData,
    ) {
        span_transfer!(_ -> run_span, WARN, "Running CullingPass");
        compute_pass.set_pipeline(&self.pipeline);
        compute_pass.set_push_constants(0, &[data.object_count]);
        compute_pass.set_bind_group(0, input_bg, &[]);
        compute_pass.set_bind_group(1, &data.bind_group, &[]);
        compute_pass.set_bind_group(2, uniform_bg, &[]);
        compute_pass.dispatch((data.object_count + self.subgroup_size - 1) / self.subgroup_size, 1, 1);
    }
}