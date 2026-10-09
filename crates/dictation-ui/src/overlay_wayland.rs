//! Overlay Wayland via **layer-shell** (`zwlr_layer_surface_v1`) + **egui**/`wgpu`.
//!
//! Substitui o `eframe`/`winit` no caminho Wayland (que não faz layer-shell): cria
//! uma superfície de camada `Overlay`, ancorada, **sem foco de teclado** e
//! **click-through** (input region vazia), e renderiza a **mesma UI** do
//! [`crate::overlay::content`] com `egui-wgpu`.
//!
//! Posicionamento: o layer-shell ancora a **margens de um `wl_output`**, não a
//! coordenadas globais. Por isso o overlay é (re)criado no `wl_output` que contém a
//! janela em foco e as margens são convertidas para o espaço **local do output**.

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

use crate::anchor;
use crate::geometry::GeometryProvider;
use crate::state::SharedUi;

/// Reancoragem na janela em foco (o ritmo de render é o vsync do compositor).
const REPOSITION: Duration = Duration::from_millis(500);

/// Roda o overlay Wayland. **Bloqueia** (chamar na thread principal).
pub fn run_overlay(state: SharedUi, indicator_anchor: String, preview_anchor: String) {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "overlay Wayland: falha ao conectar no compositor");
            return;
        }
    };
    let display_ptr = conn.display().id().as_ptr() as *mut c_void;
    let (globals, mut event_queue) = match registry_queue_init(&conn) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "overlay Wayland: registry");
            return;
        }
    };
    let qh = event_queue.handle();

    let compositor = match CompositorState::bind(&globals, &qh) {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "overlay Wayland: wl_compositor indisponível");
            return;
        }
    };
    let layer_shell = match LayerShell::bind(&globals, &qh) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!(error = %e, "overlay Wayland: wlr-layer-shell indisponível");
            return;
        }
    };
    let shm = match Shm::bind(&globals, &qh) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "overlay Wayland: wl_shm indisponível");
            return;
        }
    };

    let output_state = OutputState::new(&globals, &qh);
    let size = crate::overlay::overlay_size(false, "");
    let (layer, surface_ptr) = create_layer(
        &compositor,
        &layer_shell,
        &qh,
        None,
        (size.x as u32, size.y as u32),
    );

    let mut app = WaylandOverlay {
        registry_state: RegistryState::new(&globals),
        output_state,
        shm,
        compositor,
        layer_shell,
        state,
        indicator_anchor,
        preview_anchor,
        phase: 0.0,
        geom: crate::geometry::provider(),
        last_win: None,
        last_size: (size.x as u32, size.y as u32),
        last_pos: (i32::MIN, i32::MIN),
        last_tick: Instant::now(),
        last_reposition: Instant::now(),
        display_ptr,
        surface_ptr,
        layer,
        output: None,
        output_origin: (0, 0),
        gfx: None,
        exit: false,
    };

    tracing::info!("overlay Wayland (layer-shell) iniciado");
    while !app.exit {
        if let Err(e) = event_queue.blocking_dispatch(&mut app) {
            tracing::error!(error = %e, "overlay Wayland: dispatch");
            break;
        }
    }
}

/// Cria uma `wl_surface` + `zwlr_layer_surface_v1` (camada Overlay, ancorada em
/// `TOP|LEFT`, sem foco, click-through) no `output` dado. Devolve a `LayerSurface`
/// e o ponteiro da `wl_surface` (para a surface `wgpu`).
fn create_layer(
    compositor: &CompositorState,
    layer_shell: &LayerShell,
    qh: &QueueHandle<WaylandOverlay>,
    output: Option<&wl_output::WlOutput>,
    (w, h): (u32, u32),
) -> (LayerSurface, *mut c_void) {
    let surface = compositor.create_surface(qh);
    let surface_ptr = surface.id().as_ptr() as *mut c_void;
    let layer = layer_shell.create_layer_surface(
        qh,
        surface,
        Layer::Overlay,
        Some("dictation-rec"),
        output,
    );
    layer.set_anchor(Anchor::TOP | Anchor::LEFT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(w, h);
    if let Ok(region) = Region::new(compositor) {
        layer
            .wl_surface()
            .set_input_region(Some(region.wl_region()));
    }
    layer.commit();
    (layer, surface_ptr)
}

struct WaylandOverlay {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    state: SharedUi,
    indicator_anchor: String,
    preview_anchor: String,
    phase: f32,
    geom: Box<dyn GeometryProvider>,
    last_win: Option<anchor::Rect>,
    last_size: (u32, u32),
    last_pos: (i32, i32),
    last_tick: Instant,
    last_reposition: Instant,
    display_ptr: *mut c_void,
    surface_ptr: *mut c_void,
    layer: LayerSurface,
    /// `wl_output` em que a superfície está ancorada (e a origem lógica dele).
    output: Option<wl_output::WlOutput>,
    output_origin: (i32, i32),
    gfx: Option<Gfx>,
    exit: bool,
}

impl WaylandOverlay {
    /// `wl_output` que contém o ponto global `(cx, cy)`, se conhecido.
    fn output_for_point(&self, cx: f32, cy: f32) -> Option<wl_output::WlOutput> {
        for out in self.output_state.outputs() {
            if let Some(info) = self.output_state.info(&out) {
                if let (Some(p), Some(s)) = (info.logical_position, info.logical_size) {
                    if cx >= p.0 as f32
                        && cx < (p.0 + s.0) as f32
                        && cy >= p.1 as f32
                        && cy < (p.1 + s.1) as f32
                    {
                        return Some(out);
                    }
                }
            }
        }
        None
    }

    /// Recria a superfície no `output` alvo (o layer-shell não permite trocar de output
    /// numa superfície existente). A surface `wgpu` também é recriada.
    fn recreate_layer(&mut self, qh: &QueueHandle<Self>, output: Option<wl_output::WlOutput>) {
        self.gfx = None; // libera a surface wgpu (antes de destruir a wl_surface antiga)
        self.output_origin = output
            .as_ref()
            .and_then(|o| self.output_state.info(o))
            .and_then(|i| i.logical_position)
            .unwrap_or((0, 0));
        let (layer, surface_ptr) = create_layer(
            &self.compositor,
            &self.layer_shell,
            qh,
            output.as_ref(),
            self.last_size,
        );
        self.layer = layer;
        self.surface_ptr = surface_ptr;
        self.output = output;
        self.last_pos = (i32::MIN, i32::MIN);
    }

    /// Um passo do overlay: lê o estado, ajusta output/tamanho/posição e desenha.
    fn tick(&mut self, qh: &QueueHandle<Self>) {
        // O callback de frame só é armado quando a superfície é **commitada** (o
        // `present()` da wgpu faz isso). Por isso este caminho SEMPRE renderiza — não
        // pode retornar antes do commit, senão o loop de eventos congela.
        self.layer
            .wl_surface()
            .frame(qh, FrameCallbackData(self.layer.wl_surface().clone()));

        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;

        let (recording, transcribing, preview) = {
            let s = self.state.lock().unwrap();
            (s.recording, s.transcribing, s.preview.trim().to_string())
        };
        let visible = recording || transcribing || !preview.is_empty();
        self.phase += 2.3 * dt; // ~0.14 por 60 ms (independente do fps)
        let pulse = 0.5 + 0.5 * self.phase.sin();

        let size = crate::overlay::overlay_size(recording, &preview);
        let (w, h) = (size.x as u32, size.y as u32);
        if (w, h) != self.last_size {
            self.last_size = (w, h);
            self.layer.set_size(w, h);
        }

        // Reancora na janela em foco (com guarda anti-loop) e escolhe o output.
        if visible && now.duration_since(self.last_reposition) >= REPOSITION {
            self.last_reposition = now;
            if let Some(win) = self.geom.active_window_rect() {
                self.last_win = Some(win);
            }
        }
        if visible {
            if let Some(win) = self.last_win {
                let (cx, cy) = win.center();
                let monitor = self.geom.monitor_rect_for(cx, cy);
                let anchor = if preview.is_empty() {
                    &self.indicator_anchor
                } else {
                    &self.preview_anchor
                };
                let (px, py) = anchor::anchor_pos(anchor, win, monitor, (size.x, size.y));

                // Margens são relativas ao output: garante a superfície no output certo
                // e converte as coordenadas globais para locais do output.
                let target = self.output_for_point(cx, cy);
                if target != self.output {
                    self.recreate_layer(qh, target);
                }
                let (ox, oy) = self.output_origin;
                let pos = (
                    (px - ox as f32).max(0.0) as i32,
                    (py - oy as f32).max(0.0) as i32,
                );
                if pos != self.last_pos {
                    self.last_pos = pos;
                    self.layer.set_margin(pos.1, 0, 0, pos.0);
                }
            }
        }
        self.layer.commit();

        // Inicializa a GPU na primeira vez (precisa da superfície já criada).
        if self.gfx.is_none() {
            self.gfx = Some(Gfx::new(self.display_ptr, self.surface_ptr, self.last_size));
        }
        if let Some(gfx) = self.gfx.as_mut() {
            if (gfx.width, gfx.height) != self.last_size {
                gfx.resize(self.last_size);
            }
            gfx.render(visible, recording, &preview, pulse);
        }
    }
}

struct Gfx {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    width: u32,
    height: u32,
}

impl Gfx {
    fn new(
        display_ptr: *mut c_void,
        surface_ptr: *mut c_void,
        (width, height): (u32, u32),
    ) -> Self {
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
        .expect("surface wgpu");
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
        // egui-wgpu faz o gamma no shader: prefira um framebuffer NÃO-sRGB.
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| {
                matches!(
                    f,
                    wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Rgba8Unorm
                )
            })
            .or_else(|| caps.formats.iter().copied().find(|f| !f.is_srgb()))
            .unwrap_or(caps.formats[0]);
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
        tracing::info!(%width, %height, "overlay Wayland: wgpu pronto");
        Gfx {
            surface,
            device,
            queue,
            config,
            ctx,
            renderer,
            width,
            height,
        }
    }

    fn resize(&mut self, (width, height): (u32, u32)) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self, visible: bool, recording: bool, preview: &str, pulse: f32) {
        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(self.width as f32, self.height as f32),
            )),
            ..Default::default()
        };
        let mut output = self.ctx.run_ui(raw_input, |ui| {
            if visible {
                crate::overlay::content(ui, recording, preview, pulse);
            }
        });
        for (id, deltas) in &output.textures_delta.set {
            for d in deltas {
                self.renderer
                    .update_texture(&self.device, &self.queue, *id, d);
            }
        }
        let paint_jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.width, self.height],
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

impl CompositorHandler for WaylandOverlay {
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
        self.tick(qh);
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

impl LayerShellHandler for WaylandOverlay {
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
        let w = NonZeroU32::new(configure.new_size.0).map_or(self.last_size.0, NonZeroU32::get);
        let h = NonZeroU32::new(configure.new_size.1).map_or(self.last_size.1, NonZeroU32::get);
        self.last_size = (w, h);
        self.tick(qh);
    }
}

impl OutputHandler for WaylandOverlay {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for WaylandOverlay {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(WaylandOverlay);

impl ProvidesRegistryState for WaylandOverlay {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(WaylandOverlay);
