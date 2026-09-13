//! Headless render for the demo: `SHOT=<w>:<h>:<out.rgba>` rasterizes the app to raw
//! RGBA through vello_hybrid (no window, no display) so the editor can be screenshotted
//! in CI or over SSH. Convert the dump to PNG with any tool (e.g. PIL).

use pebbles::core::Ui;
use pebbles::prelude::*;
use pebbles::render::paint::kurbo;
use pebbles::render::{Scene, TextEnv};
use vello::util::RenderContext;
use vello_hybrid::{
    RenderSize, RenderTargetConfig, Renderer as HybRenderer, Resources, Scene as HybScene,
    TextureBindings,
};
use wgpu::{
    Extent3d, TexelCopyBufferInfo, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsages,
};

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
        let (renderer, resources) = HybRenderer::new(
            &handle.device,
            &RenderTargetConfig {
                format: TextureFormat::Rgba8Unorm,
                width: w,
                height: h,
            },
        );
        Gpu {
            device: handle.device,
            queue: handle.queue,
            renderer,
            resources,
        }
    }

    fn rasterize(&mut self, scene: &Scene, w: u32, h: u32, base: Color) -> Vec<u8> {
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("shot"),
            size: Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        let mut hyb = HybScene::new(
            w.min(u32::from(u16::MAX)) as u16,
            h.min(u32::from(u16::MAX)) as u16,
        );
        let [r, g, b, _] = base.components;
        hyb.set_paint(Color::new([r, g, b, 1.0]));
        hyb.fill_rect(&kurbo::Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        scene.flush(&mut hyb, &mut self.resources);
        let bindings = TextureBindings::new();

        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        self.renderer
            .render(
                &hyb,
                &mut self.resources,
                &self.device,
                &self.queue,
                &mut enc,
                &RenderSize {
                    width: w,
                    height: h,
                },
                &view,
                &bindings,
            )
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
        let mut enc = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        enc.copy_texture_to_buffer(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            TexelCopyBufferInfo {
                buffer: &buffer,
                layout: TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([enc.finish()]);

        let slice = buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("poll");
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

/// Mirror the shell's post-layout step so `use_bounds()` works headlessly (needed for soft
/// wrap, which measures the content width). Without this, bounds stay `Rect::ZERO`.
fn publish_bounds(ui: &Ui) {
    let tree = ui.render_tree();
    for (w, src) in pebbles::core::bounds::wanted_bounds() {
        if let Some(rid) = tree.find_by_source(src) {
            let o = tree.absolute_offset(rid);
            let s = tree.size_of(rid);
            pebbles::core::bounds::publish_bounds(
                w,
                src,
                Rect::new(o.x, o.y, o.x + s.width, o.y + s.height),
            );
        }
    }
}

fn scene_for(ui: &mut Ui, env: &mut TextEnv, w: u32, h: u32) -> Scene {
    let size = Size::new(f64::from(w), f64::from(h));
    ui.make_current();
    let mut scene = Scene::new();
    for _ in 0..6 {
        ui.rebuild_if_dirty();
        ui.layout(env, size);
        scene = Scene::new();
        ui.paint(env, &mut scene);
        publish_bounds(ui);
    }
    scene
}

/// `SHOT=<w>:<h>:<out.rgba>` — render the demo app once and dump tight RGBA.
pub fn shot(
    spec: &str,
    root: fn() -> AnyWidget,
    base: Color,
) -> Result<(), Box<dyn std::error::Error>> {
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
    // Optional: drive keys before the shot (e.g. SHOT_KEYS=find to open the find bar).
    if let Ok(keys) = std::env::var("SHOT_KEYS") {
        for _ in 0..3 {
            ui.rebuild_if_dirty();
            ui.layout(&mut env, Size::new(f64::from(w), f64::from(h)));
        }
        let dispatch = |ui: &mut Ui, env: &mut TextEnv, k: KeyInput| {
            ui.dispatch_key(k);
            ui.rebuild_if_dirty();
            ui.layout(env, Size::new(f64::from(w), f64::from(h)));
        };
        match keys.as_str() {
            "find" => {
                dispatch(&mut ui, &mut env, KeyInput::Find);
                for ch in "counts".chars() {
                    dispatch(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
                }
            }
            "replace" => {
                dispatch(&mut ui, &mut env, KeyInput::Replace);
                for ch in "counts".chars() {
                    dispatch(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
                }
            }
            "palette" => {
                dispatch(&mut ui, &mut env, KeyInput::CommandPalette);
            }
            // Insert a right-to-left (Arabic) comment to check bidi display.
            "rtl" => {
                dispatch(
                    &mut ui,
                    &mut env,
                    KeyInput::Move {
                        motion: Motion::DocEnd,
                        extend: false,
                    },
                );
                dispatch(&mut ui, &mut env, KeyInput::Enter);
                for ch in "// مرحبا بالعالم hello".chars() {
                    dispatch(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
                }
            }
            // Type a fresh line so the inline diff (vs last-saved) shows a green added band.
            "diff" => {
                dispatch(
                    &mut ui,
                    &mut env,
                    KeyInput::Move {
                        motion: Motion::DocEnd,
                        extend: false,
                    },
                );
                dispatch(&mut ui, &mut env, KeyInput::Enter);
                for ch in "let added = true;".chars() {
                    dispatch(&mut ui, &mut env, KeyInput::Insert(ch.to_string()));
                }
            }
            // An exhaustive interaction soak: TAP the activity bar / explorer rows / tabs /
            // menus, drive find/replace/palette, type, double/right-click, scroll and hover —
            // to surface any crash and confirm the live-signal count stays bounded.
            "soak" => {
                let size = Size::new(f64::from(w), f64::from(h));
                let step = |ui: &mut Ui, env: &mut TextEnv| {
                    ui.rebuild_if_dirty();
                    ui.layout(env, size);
                };
                let tap = |ui: &mut Ui, env: &mut TextEnv, x: f64, y: f64| {
                    ui.dispatch_tap(Offset::new(x, y));
                    step(ui, env);
                };
                let key = |ui: &mut Ui, env: &mut TextEnv, k: KeyInput| {
                    ui.dispatch_key(k);
                    step(ui, env);
                };
                let typ = |ui: &mut Ui, env: &mut TextEnv, s: &str| {
                    for ch in s.chars() {
                        ui.dispatch_key(KeyInput::Insert(ch.to_string()));
                    }
                    step(ui, env);
                };
                #[cfg(debug_assertions)]
                let baseline = pebbles::core::census_signals();
                for i in 0..60 {
                    // Activity bar (on_tap): Search, Settings, Explorer, Problems toggle.
                    tap(&mut ui, &mut env, 24.0, 103.0);
                    tap(&mut ui, &mut env, 24.0, 149.0);
                    // In Settings: font +/- and a couple of switches.
                    tap(&mut ui, &mut env, 285.0, 88.0);
                    tap(&mut ui, &mut env, 210.0, 88.0);
                    tap(&mut ui, &mut env, 270.0, 150.0);
                    tap(&mut ui, &mut env, 24.0, 57.0); // back to Explorer
                    // Open several files by tapping rows.
                    for y in [
                        94.0, 116.0, 182.0, 205.0, 227.0, 271.0, 316.0, 360.0, 382.0, 427.0,
                    ] {
                        tap(&mut ui, &mut env, 140.0, y);
                    }
                    // Double-click + right-click an explorer row (rename / context menu), cancel.
                    ui.dispatch_double_tap(Offset::new(140.0, 205.0));
                    step(&mut ui, &mut env);
                    key(&mut ui, &mut env, KeyInput::Escape);
                    ui.dispatch_secondary_tap(Offset::new(140.0, 205.0));
                    step(&mut ui, &mut env);
                    key(&mut ui, &mut env, KeyInput::Escape);
                    // Tabs: switch + close.
                    tap(&mut ui, &mut env, 380.0, 52.0);
                    tap(&mut ui, &mut env, 520.0, 52.0);
                    tap(&mut ui, &mut env, 560.0, 52.0); // a close (×)
                    // Editor: click, type, edit.
                    ui.dispatch_pointer_down(Offset::new(600.0, 200.0));
                    step(&mut ui, &mut env);
                    typ(&mut ui, &mut env, "hello");
                    key(&mut ui, &mut env, KeyInput::Enter);
                    key(&mut ui, &mut env, KeyInput::Backspace);
                    key(&mut ui, &mut env, KeyInput::Indent);
                    // Find / replace / palette.
                    key(&mut ui, &mut env, KeyInput::Find);
                    typ(&mut ui, &mut env, "count");
                    key(&mut ui, &mut env, KeyInput::Escape);
                    key(&mut ui, &mut env, KeyInput::Replace);
                    key(&mut ui, &mut env, KeyInput::Escape);
                    key(&mut ui, &mut env, KeyInput::CommandPalette);
                    typ(&mut ui, &mut env, "up");
                    key(&mut ui, &mut env, KeyInput::Escape);
                    // Menu bar: open File/Edit/View, tap an item area, dismiss.
                    tap(&mut ui, &mut env, 118.0, 16.0);
                    tap(&mut ui, &mut env, 118.0, 60.0);
                    tap(&mut ui, &mut env, 167.0, 16.0);
                    tap(&mut ui, &mut env, 167.0, 60.0);
                    tap(&mut ui, &mut env, 223.0, 16.0);
                    tap(&mut ui, &mut env, 223.0, 60.0);
                    // Scroll + hover.
                    ui.dispatch_scroll(Offset::new(600.0, 300.0), 40.0);
                    ui.dispatch_hover(Offset::new(500.0 + (i % 30) as f64, 180.0));
                    step(&mut ui, &mut env);
                    #[cfg(debug_assertions)]
                    if i % 20 == 0 {
                        println!("soak i={i} signals={}", pebbles::core::census_signals());
                    }
                }
                #[cfg(debug_assertions)]
                {
                    let after = pebbles::core::census_signals();
                    println!("soak DONE: signals {baseline} -> {after}");
                    assert!(
                        after <= baseline + 200,
                        "signal leak: {baseline} -> {after}"
                    );
                }
            }
            _ => {}
        }
    }
    let scene = scene_for(&mut ui, &mut env, w, h);
    let px = gpu.rasterize(&scene, w, h, base);
    std::fs::write(out, &px)?;
    println!("{w} {h} {out}");
    Ok(())
}
