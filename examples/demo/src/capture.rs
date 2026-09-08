//! Headless render for the demo: `SHOT=<w>:<h>:<out.rgba>` rasterizes the app to raw
//! RGBA through vello_hybrid (no window, no display) so the editor can be screenshotted
//! in CI or over SSH. Convert the dump to PNG with any tool (e.g. PIL).

use pebbles::core::Ui;
use pebbles::prelude::*;
use pebbles::render::paint::kurbo;
use pebbles::render::{Scene, TextEnv};
use vello::util::RenderContext;
use vello_hybrid::{RenderSize, RenderTargetConfig, Renderer as HybRenderer, Resources, Scene as HybScene, TextureBindings};
use wgpu::{Extent3d, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: HybRenderer,
    resources: Resources,
}

impl Gpu {
    fn new(w: u32, h: u32) -> Self {
        let mut ctx = RenderContext::new();
        let dev_id = pollster::block_on(ctx.device(None)).expect("no compatible GPU device");
        let handle = ctx.devices.remove(dev_id);
        let (renderer, resources) =
            HybRenderer::new(&handle.device, &RenderTargetConfig { format: TextureFormat::Rgba8Unorm, width: w, height: h });
        Gpu { device: handle.device, queue: handle.queue, renderer, resources }
    }

    fn rasterize(&mut self, scene: &Scene, w: u32, h: u32, base: Color) -> Vec<u8> {
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("shot"),
            size: Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut hyb = HybScene::new(w.min(u32::from(u16::MAX)) as u16, h.min(u32::from(u16::MAX)) as u16);
        let [r, g, b, _] = base.components;
        hyb.set_paint(Color::new([r, g, b, 1.0]));
        hyb.fill_rect(&kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        scene.flush(&mut hyb, &mut self.resources);
        let bindings = TextureBindings::new();

        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.renderer
            .render(&hyb, &mut self.resources, &self.device, &self.queue, &mut enc, &RenderSize { width: w, height: h }, &view, &bindings)
            .expect("render");
        self.queue.submit([enc.finish()]);

        let unpadded = w * 4;
        let padded = unpadded.div_ceil(256) * 256;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: u64::from(padded) * u64::from(h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            TexelCopyBufferInfo { buffer: &buffer, layout: TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(padded), rows_per_image: Some(h) } },
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        self.queue.submit([enc.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).expect("poll");
        rx.recv().expect("map").expect("mapped");
        let mapped = slice.get_mapped_range();
        let mut out = Vec::with_capacity((unpadded * h) as usize);
        for row in 0..h {
            let s = (row * padded) as usize;
            out.extend_from_slice(&mapped[s..s + unpadded as usize]);
        }
        drop(mapped);
        buffer.unmap();
        out
    }
}

fn scene_for(ui: &mut Ui, env: &mut TextEnv, w: u32, h: u32) -> Scene {
    let size = Size::new(f64::from(w), f64::from(h));
    ui.make_current();
    let mut scene = Scene::new();
    for _ in 0..4 {
        ui.rebuild_if_dirty();
        ui.layout(env, size);
        scene = Scene::new();
        if !ui.paint(env, &mut scene) {
            break;
        }
    }
    scene
}

/// `SHOT=<w>:<h>:<out.rgba>` — render the demo app once and dump tight RGBA.
pub fn shot(spec: &str, root: fn() -> AnyWidget, base: Color) -> Result<(), Box<dyn std::error::Error>> {
    let p: Vec<&str> = spec.split(':').collect();
    let (w, h, out) = (p[0].parse::<u32>()?, p[1].parse::<u32>()?, p[2]);
    pebbles::widgets::overlay::init();
    pebbles::core::focus::init();
    let mut gpu = Gpu::new(w, h);
    let mut env = TextEnv::new();
    let mut ui = Ui::new();
    ui.make_current();
    ui.mount_root(View::new(base, OverlayHost::wrap(root().into_widget())).into_widget());
    pebbles::widgets::overlay::set_window_size(f64::from(w), f64::from(h));
    let scene = scene_for(&mut ui, &mut env, w, h);
    let px = gpu.rasterize(&scene, w, h, base);
    std::fs::write(out, &px)?;
    println!("{w} {h} {out}");
    Ok(())
}
