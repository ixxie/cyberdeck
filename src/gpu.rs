use std::sync::Arc;

use wgpu::*;
use wgpu::util::DeviceExt;

/// Shared GPU state: device, queue, and vello renderer.
pub struct GpuState {
    pub instance: Instance,
    pub adapter: Adapter,
    pub device: Arc<Device>,
    pub queue: Arc<Queue>,
    pub vello: vello::Renderer,
}

impl GpuState {
    pub fn new() -> Self {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::VULKAN | Backends::GL,
            ..Default::default()
        });

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("no suitable GPU adapter found");

        log::info!("gpu: adapter = {:?}", adapter.get_info().name);

        let (device, queue) = pollster::block_on(adapter.request_device(
            &DeviceDescriptor {
                label: Some("cyberdeck"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                memory_hints: MemoryHints::Performance,
                trace: Default::default(),
                experimental_features: Default::default(),
            },
        ))
        .expect("failed to create GPU device");

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let vello = vello::Renderer::new(
            &device,
            vello::RendererOptions {
                use_cpu: false,
                antialiasing_support: vello::AaSupport::area_only(),
                num_init_threads: Some(std::num::NonZeroUsize::new(1).unwrap()),
                pipeline_cache: None,
            },
        )
        .expect("failed to create vello renderer");

        Self { instance, adapter, device, queue, vello }
    }
}

/// GPU render target: renders via vello to a texture, reads back to CPU for SHM display.
pub struct GpuTarget {
    pub target_tex: Option<Texture>,
    pub readback_buf: Option<Buffer>,
    pub width: u32,
    pub height: u32,
}

impl GpuTarget {
    pub fn new() -> Self {
        Self { target_tex: None, readback_buf: None, width: 0, height: 0 }
    }

    /// Resize the render target.
    pub fn resize(&mut self, gpu: &GpuState, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;

        self.target_tex = Some(gpu.device.create_texture(&TextureDescriptor {
            label: Some("vello_target"),
            size: Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::STORAGE_BINDING
                | TextureUsages::COPY_SRC,
            view_formats: &[],
        }));

        // Buffer for reading back pixels to CPU
        let padded_row = Self::padded_row_size(width);
        self.readback_buf = Some(gpu.device.create_buffer(&BufferDescriptor {
            label: Some("readback"),
            size: (padded_row * height) as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        }));

        log::info!("gpu: resize {}x{}", width, height);
    }

    fn padded_row_size(width: u32) -> u32 {
        let bytes_per_row = width * 4;
        // wgpu requires rows aligned to 256 bytes
        (bytes_per_row + 255) & !255
    }

    /// Render scene and read back pixels as premultiplied sRGB BGRA for Wayland.
    pub fn render_to_pixels(
        &mut self, gpu: &mut GpuState, scene: &vello::Scene,
        base_color: vello::peniko::color::AlphaColor<vello::peniko::color::Srgb>,
    ) -> Option<Vec<u8>> {
        let Some(ref target_tex) = self.target_tex else { return None };
        let Some(ref readback_buf) = self.readback_buf else { return None };
        let w = self.width;
        let h = self.height;

        let target_view = target_tex.create_view(&TextureViewDescriptor::default());

        let params = vello::RenderParams {
            base_color,
            width: w,
            height: h,
            antialiasing_method: vello::AaConfig::Area,
        };

        gpu.vello
            .render_to_texture(&gpu.device, &gpu.queue, scene, &target_view, &params)
            .expect("vello render failed");

        // Copy texture to readback buffer
        let padded_row = Self::padded_row_size(w);
        let mut encoder = gpu.device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("readback"),
        });
        encoder.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: target_tex,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: readback_buf,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_row),
                    rows_per_image: Some(h),
                },
            },
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        gpu.queue.submit(std::iter::once(encoder.finish()));

        // Map buffer and read pixels
        let slice = readback_buf.slice(..);
        slice.map_async(MapMode::Read, |_| {});
        let _ = gpu.device.poll(PollType::Wait { submission_index: None, timeout: None });

        let data = slice.get_mapped_range();
        let row_bytes = (w * 4) as usize;
        let mut pixels = vec![0u8; row_bytes * h as usize];

        // Convert RGBA to premultiplied ARGB8888 (Wayland: B, G, R, A in memory)
        for y in 0..h as usize {
            let src_offset = y * padded_row as usize;
            let dst_offset = y * row_bytes;
            for x in 0..w as usize {
                let si = src_offset + x * 4;
                let di = dst_offset + x * 4;
                let a = data[si + 3];
                if a > 0 {
                    let af = a as f32 / 255.0;
                    // Premultiply for Wayland compositor
                    pixels[di] = (data[si + 2] as f32 * af + 0.5) as u8; // B
                    pixels[di + 1] = (data[si + 1] as f32 * af + 0.5) as u8; // G
                    pixels[di + 2] = (data[si] as f32 * af + 0.5) as u8; // R
                    pixels[di + 3] = a;
                }
            }
        }

        drop(data);
        readback_buf.unmap();

        Some(pixels)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.0031308 { return c * 12.92; }
    1.055 * c.powf(1.0 / 2.4) - 0.055
}
