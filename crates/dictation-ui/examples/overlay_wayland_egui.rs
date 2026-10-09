//! **F4 (integração)** — overlay Wayland com **egui** renderizado via `wgpu` numa
//! superfície **layer-shell**. Substitui o `eframe`/`winit` (que não faz layer-shell)
//! no caminho Wayland.
//!
//! Prova de conceito: cria a `zwlr_layer_surface_v1` (camada Overlay, ancorada,
//! `keyboard_interactivity=none`, input region vazia = click-through), cria a surface
//! `wgpu` a partir dos *raw handles* da `wl_surface` e desenha o badge REC com egui.
//!
//! Rodar numa sessão Wayland:
//! ```sh
//! cargo run -p dictation-ui --features wayland --example overlay_wayland_egui
//! ```
//! Sai sozinho após ~8 s.

use std::ffi::c_void;
use std::num::NonZeroU32;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use raw_window_handle::{
    RawDisplayHandle, RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle,
};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState, FrameCallbackData, Region},
    delegate_registry,
    output::{OutputHandler, OutputState},
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_surface},
    Connection, Proxy, QueueHandle,
};

const W: u32 = 360;
const H: u32 = 90;
const RUN: Duration = Duration::from_secs(8);

fn main() {
    let conn = Connection::connect_to_env().expect("conectar ao compositor Wayland");
    let display_ptr = conn.display().id().as_ptr() as *mut c_void;
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor indisponível");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell indisponível");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm indisponível");

    let surface = compositor.create_surface(&qh);
    let surface_ptr = surface.id().as_ptr() as *mut c_void;
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("dictation-rec"), None);
    layer.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(W, H);

    let region = Region::new(&compositor).expect("wl_region");
    layer
        .wl_surface()
        .set_input_region(Some(region.wl_region()));
    layer.commit();
    drop(region);

    let mut app = OverlayEgui {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        exit: false,
        start: Instant::now(),
        width: W,
        height: H,
        layer,
        display_ptr,
        surface_ptr,
        gfx: None,
    };

    while !app.exit {
        event_queue.blocking_dispatch(&mut app).expect("dispatch");
    }
    println!("overlay_wayland_egui: encerrado");
}

struct OverlayEgui {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    exit: bool,
    start: Instant,
    width: u32,
    height: u32,
    layer: LayerSurface,
    display_ptr: *mut c_void,
    surface_ptr: *mut c_void,
    gfx: Option<Gfx>,
}

struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
}

impl OverlayEgui {
    fn ensure_gfx(&mut self) {
        if self.gfx.is_some() {
            return;
        }
        self.gfx = Some(Gfx::new(
            self.display_ptr,
            self.surface_ptr,
            self.width,
            self.height,
        ));
    }

    fn draw(&mut self, qh: &QueueHandle<Self>) {
        // Pede o próximo frame (mantém o loop acordado) e desenha.
        self.layer
            .wl_surface()
            .frame(qh, FrameCallbackData(self.layer.wl_surface().clone()));
        if let Some(gfx) = self.gfx.as_mut() {
            gfx.render();
        }
    }
}

impl Gfx {
    fn new(display_ptr: *mut c_void, surface_ptr: *mut c_void, width: u32, height: u32) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });

        let raw_display = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(
            NonNull::new(display_ptr).expect("wl_display não nulo"),
        ));
        let raw_window = RawWindowHandle::Wayland(WaylandWindowHandle::new(
            NonNull::new(surface_ptr).expect("wl_surface não nula"),
        ));
        let surface: wgpu::Surface<'static> = unsafe {
            instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display),
                raw_window_handle: raw_window,
            })
        }
        .expect("criar surface wgpu a partir da wl_surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: Some(&surface),
            ..Default::default()
        }))
        .expect("adaptador wgpu");
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("dictation-overlay"),
            ..Default::default()
        }))
        .expect("device wgpu");

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(wgpu::TextureFormat::is_srgb)
            .unwrap_or(caps.formats[0]);
        // Overlay é translúcido: preferir alpha pré-multiplicado.
        let alpha_mode = caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::CompositeAlphaMode::PreMultiplied)
            .unwrap_or(caps.alpha_modes[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode,
            view_formats: vec![],
        };
        surface.configure(&device, &config);

        let ctx = egui::Context::default();
        let renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        println!(
            "overlay_wayland_egui: wgpu pronto ({}x{}, {:?}, alpha={:?})",
            width, height, format, config.alpha_mode
        );
        Gfx {
            surface,
            device,
            queue,
            config,
            ctx,
            renderer,
        }
    }

    fn render(&mut self) {
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(self.config.width as f32, self.config.height as f32),
            )),
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(raw_input, |ui| {
            egui::Frame::NONE
                .fill(egui::Color32::from_rgba_unmultiplied(18, 18, 18, 205))
                .corner_radius(12.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("\u{25CF}")
                                .color(egui::Color32::from_rgb(235, 45, 45))
                                .size(16.0),
                        );
                        ui.label(egui::RichText::new("REC").strong().size(12.0));
                    });
                });
        });
        // Aplica texturas do egui (fonte) antes de desenhar; sem isto, o epaint
        // entra em pânico ao dropar `TexturesDelta` com deltas não aplicados.
        for (id, deltas) in &output.textures_delta.set {
            for d in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, d);
            }
        }
        let paint_jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: output.pixels_per_point,
        };

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            _ => return,
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        let cmds = self.renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
            let mut pass = pass.forget_lifetime();
            self.renderer.render(&mut pass, &paint_jobs, &screen);
        }
        self.queue
            .submit(cmds.into_iter().chain(std::iter::once(encoder.finish())));
        self.queue.present(frame);

        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        output.textures_delta.clear();
    }
}

impl CompositorHandler for OverlayEgui {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }
    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }
    fn frame(&mut self, _: &Connection, qh: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {
        if self.start.elapsed() > RUN {
            self.exit = true;
            return;
        }
        self.draw(qh);
    }
    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for OverlayEgui {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        self.exit = true;
    }
    fn configure(
        &mut self,
        _: &Connection,
        qh: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        self.width = NonZeroU32::new(configure.new_size.0).map_or(W, NonZeroU32::get);
        self.height = NonZeroU32::new(configure.new_size.1).map_or(H, NonZeroU32::get);
        println!(
            "overlay_wayland_egui: configure ({}x{})",
            self.width, self.height
        );
        self.ensure_gfx();
        self.draw(qh);
    }
}

impl OutputHandler for OverlayEgui {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for OverlayEgui {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(OverlayEgui);

impl ProvidesRegistryState for OverlayEgui {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(OverlayEgui);
