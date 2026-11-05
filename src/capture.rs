//! Capture the very texture presented to Rune's window, rather than re-rendering a recording later.
use crate::ui::State;
use rune::{Compositor, Frame};

#[derive(Default)]
pub struct Capture {
    target: Option<Target>,
}
struct Target {
    device: rune::compose::DeviceKey,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    buffer: wgpu::Buffer,
    row: u32,
    bind: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}
impl Target {
    fn new(f: &Frame<'_, State>) -> Self {
        let device = f.device;
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("Tacit original pixels"),
            size: wgpu::Extent3d {
                width: f.size.0,
                height: f.size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: f.format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let row = (f.size.0 * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Tacit readback"),
            size: u64::from(row) * u64::from(f.size.1),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Exact pixel presentation"),
            source: wgpu::ShaderSource::Wgsl(
                r#"
@group(0) @binding(0) var source: texture_2d<f32>;
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let points = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(points[index], 0.0, 1.0);
}
@fragment fn fs(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(point.xy), 0);
}
"#
                .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tacit present original pixels"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: f.format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            device: f.device_key(),
            size: f.size,
            format: f.format,
            texture,
            view,
            buffer,
            row,
            bind,
            pipeline,
        }
    }
    fn read(&self, f: &mut Frame<'_, State>) -> Result<Vec<u8>, String> {
        let mut encoder = f.device.create_command_encoder(&Default::default());
        f.ui.prepare(
            f.device,
            f.queue,
            &mut f.rt.text.atlas,
            &f.rt.instances,
            f.size,
        );
        f.ui.draw(&mut encoder, &self.view, f.rt.clear_color());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.row),
                    rows_per_image: Some(f.size.1),
                },
            },
            wgpu::Extent3d {
                width: f.size.0,
                height: f.size.1,
                depth_or_array_layers: 1,
            },
        );
        f.ui.submit(f.queue, encoder);
        let (send, receive) = std::sync::mpsc::channel();
        self.buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        f.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        receive
            .recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
        let bytes = self
            .buffer
            .slice(..)
            .get_mapped_range()
            .map_err(|e| e.to_string())?;
        let mut rgba = Vec::with_capacity(f.size.0 as usize * f.size.1 as usize * 4);
        for row in bytes.chunks_exact(self.row as usize) {
            rgba.extend_from_slice(&row[..f.size.0 as usize * 4]);
        }
        drop(bytes);
        self.buffer.unmap();
        if matches!(
            f.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
        }
        Ok(rgba)
    }
    fn present(&self, f: &mut Frame<'_, State>) {
        let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Tacit recorded frame"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: f.target,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.bind, &[]);
        pass.draw(0..3, 0..1);
    }
}
impl Compositor<State> for Capture {
    fn render(&mut self, f: &mut Frame<'_, State>) {
        if !f.rt.state.capturing_pixels() {
            f.draw_ui();
            return;
        }
        if !matches!(
            f.format,
            wgpu::TextureFormat::Rgba8Unorm
                | wgpu::TextureFormat::Rgba8UnormSrgb
                | wgpu::TextureFormat::Bgra8Unorm
                | wgpu::TextureFormat::Bgra8UnormSrgb
        ) {
            f.rt.state
                .capture_failed("Unsupported pixel format; original video capture stopped".into());
            f.draw_ui();
            return;
        }
        if self
            .target
            .as_ref()
            .is_none_or(|t| t.size != f.size || t.device != f.device_key() || t.format != f.format)
        {
            self.target = Some(Target::new(f));
        }
        let target = self.target.as_ref().unwrap();
        match target.read(f) {
            Ok(pixels) => {
                if let Err(error) = f.rt.state.record_pixels(f.size, f.rt.env.scale, &pixels) {
                    f.rt.state.capture_failed(error);
                }
                target.present(f);
            }
            Err(error) => {
                f.rt.state.capture_failed(error);
                f.draw_ui();
            }
        }
    }
}
