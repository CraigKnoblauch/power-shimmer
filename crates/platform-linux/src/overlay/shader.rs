//! wgpu pipeline and uniform packing for the shimmer shader.

use std::path::Path;

use bytemuck::{Pod, Zeroable};
use power_shimmer_core::ShimmerConfig;
use wgpu::util::DeviceExt;
use wgpu::{
    Device, Queue, RenderPipeline, ShaderModule, SurfaceConfiguration, TextureFormat,
};

/// CPU mirror of [`super::SHADER_SOURCE`] `ShimmerParams` uniform block.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ShimmerParams {
    /// Seconds since animation start.
    pub elapsed_s: f32,
    /// Total duration in seconds (`duration_ms / 1000`).
    pub duration_s: f32,
    /// Peak opacity from config.
    pub opacity: f32,
    /// Speed multiplier from config.
    pub speed: f32,
}

impl ShimmerParams {
    /// Builds uniforms from domain config and elapsed time.
    #[must_use]
    pub fn from_config(config: &ShimmerConfig, elapsed_s: f32) -> Self {
        Self {
            elapsed_s,
            #[allow(clippy::cast_precision_loss)]
            duration_s: {
                config.duration_ms as f32 / 1000.0
            },
            opacity: config.opacity,
            speed: config.speed,
        }
    }
}

/// Loaded WGSL and render pipeline resources.
pub struct ShimmerPipeline {
    pipeline: RenderPipeline,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,
    _shader: ShaderModule,
}

impl ShimmerPipeline {
    /// Compiles the embedded shader and builds the render pipeline.
    pub fn new(device: &Device, format: TextureFormat, source: &str) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shimmer_shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shimmer_bind_group_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("shimmer_uniform_buffer"),
            contents: bytemuck::bytes_of(&ShimmerParams::from_config(
                &ShimmerConfig::default(),
                0.0,
            )),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shimmer_bind_group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shimmer_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("shimmer_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState {
                        color: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::SrcAlpha,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                        alpha: wgpu::BlendComponent {
                            src_factor: wgpu::BlendFactor::One,
                            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                            operation: wgpu::BlendOperation::Add,
                        },
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            pipeline,
            uniform_buffer,
            bind_group,
            _shader: shader,
        }
    }

    /// Writes uniform values for the current frame.
    pub fn write_uniforms(&self, queue: &Queue, params: &ShimmerParams) {
        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(params));
    }

    /// Records a full-screen draw into `encoder`.
    pub fn render(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shimmer_pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind_group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Path to the workspace shader asset (for tests/docs).
#[must_use]
#[cfg_attr(not(test), allow(dead_code))]
pub fn shader_asset_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/shaders/shimmer.wgsl")
}

/// WGSL source embedded at compile time.
pub const SHADER_SOURCE: &str =
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/shaders/shimmer.wgsl"));

/// Preferred surface format for premultiplied alpha overlays.
#[must_use]
pub fn surface_format() -> TextureFormat {
    TextureFormat::Bgra8Unorm
}

/// Builds a surface configuration for the given size.
#[must_use]
pub fn surface_config(width: u32, height: u32, format: TextureFormat) -> SurfaceConfiguration {
    SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format,
        width: width.max(1),
        height: height.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: wgpu::CompositeAlphaMode::PreMultiplied,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shimmer_params_layout_is_16_bytes() {
        assert_eq!(std::mem::size_of::<ShimmerParams>(), 16);
    }

    #[test]
    fn shader_asset_exists_on_disk() {
        assert!(
            shader_asset_path().exists(),
            "missing {}",
            shader_asset_path().display()
        );
    }

    #[test]
    fn embedded_shader_matches_asset_file() {
        let disk = std::fs::read_to_string(shader_asset_path()).expect("read shader");
        assert_eq!(disk, SHADER_SOURCE);
    }

    #[test]
    fn from_config_maps_duration_ms_to_seconds() {
        let mut config = ShimmerConfig::default();
        config.duration_ms = 2500;
        config.opacity = 0.5;
        config.speed = 2.0;
        let params = ShimmerParams::from_config(&config, 1.25);
        assert!((params.duration_s - 2.5).abs() < f32::EPSILON);
        assert!((params.opacity - 0.5).abs() < f32::EPSILON);
        assert!((params.speed - 2.0).abs() < f32::EPSILON);
        assert!((params.elapsed_s - 1.25).abs() < f32::EPSILON);
    }
}
