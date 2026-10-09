//! **Spike F4** — overlay em `wlr-layer-shell` (Wayland/KWin).
//!
//! Prova de conceito (NÃO integrado ao daemon ainda) do que o `eframe`/`winit` não
//! oferece no Wayland: uma superfície de **camada `Overlay`**, **ancorada**,
//! **sem foco de teclado** e **click-through** (input region vazia).
//!
//! Renderiza um "badge REC" simples via `wl_shm` (software). A integração do egui
//! de verdade vem depois; aqui validamos o protocolo/âncora/click-through.
//!
//! Rodar numa sessão Wayland:
//! ```sh
//! cargo run -p dictation-ui --features wayland --example overlay_wayland_spike
//! ```
//! Sai sozinho após ~8 s (ou quando o compositor fechar a superfície).

use std::num::NonZeroU32;
use std::time::{Duration, Instant};

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
    shm::{slot::SlotPool, Shm, ShmHandler},
};
use wayland_client::{
    globals::registry_queue_init,
    protocol::{wl_output, wl_shm, wl_surface},
    Connection, QueueHandle,
};

/// Tamanho do badge (só-badge), como no overlay X11.
const BADGE_W: u32 = 160;
const BADGE_H: u32 = 48;
/// Duração do spike antes de sair.
const RUN: Duration = Duration::from_secs(8);

fn main() {
    let conn = Connection::connect_to_env().expect("conectar ao compositor Wayland");
    let (globals, mut event_queue) = registry_queue_init(&conn).expect("registry");
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh).expect("wl_compositor indisponível");
    let layer_shell = LayerShell::bind(&globals, &qh).expect("wlr-layer-shell indisponível");
    let shm = Shm::bind(&globals, &qh).expect("wl_shm indisponível");

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Overlay, Some("dictation-rec"), None);

    // Âncora: canto inferior direito (badge). Sem reservar espaço (exclusive_zone=0).
    layer.set_anchor(Anchor::BOTTOM | Anchor::RIGHT);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(BADGE_W, BADGE_H);

    // **Click-through**: input region VAZIA — o compositor entrega os cliques ao app de baixo.
    let region = Region::new(&compositor).expect("wl_region");

    // Commit inicial SEM buffer: o compositor responde com o primeiro `configure`.
    layer
        .wl_surface()
        .set_input_region(Some(region.wl_region()));
    layer.commit();
    drop(region);

    let pool = SlotPool::new((BADGE_W * BADGE_H * 4) as usize, &shm).expect("pool shm");

    let mut app = OverlaySpike {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        exit: false,
        first_configure: true,
        start: Instant::now(),
        pool,
        width: BADGE_W,
        height: BADGE_H,
        layer,
    };

    while !app.exit {
        event_queue
            .blocking_dispatch(&mut app)
            .expect("dispatch Wayland");
    }
    println!("spike: encerrado");
}

struct OverlaySpike {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    exit: bool,
    first_configure: bool,
    start: Instant,
    pool: SlotPool,
    width: u32,
    height: u32,
    layer: LayerSurface,
}

impl OverlaySpike {
    fn draw(&mut self, qh: &QueueHandle<Self>) {
        let w = self.width;
        let h = self.height;
        let stride = (w * 4) as i32;
        let (buffer, canvas) = self
            .pool
            .create_buffer(w as i32, h as i32, stride, wl_shm::Format::Argb8888)
            .expect("buffer shm");

        // Fundo escuro translúcido + "dot" REC vermelho à esquerda (premultiplied alpha).
        let bg = (205u32, 18u32, 18u32, 18u32); // a, r, g, b
        let dot = (255u32, 235u32, 45u32, 45u32);
        let (cx, cy, r) = (24.0f32, h as f32 / 2.0, 7.0f32);
        for y in 0..h {
            for x in 0..w {
                let in_dot = {
                    let dx = x as f32 + 0.5 - cx;
                    let dy = y as f32 + 0.5 - cy;
                    dx * dx + dy * dy <= r * r
                };
                let (a, rr, gg, bb) = if in_dot { dot } else { bg };
                // premultiply
                let pr = rr * a / 255;
                let pg = gg * a / 255;
                let pb = bb * a / 255;
                let px = (a << 24) | (pr << 16) | (pg << 8) | pb;
                let idx = ((y * w + x) * 4) as usize;
                canvas[idx..idx + 4].copy_from_slice(&px.to_le_bytes());
            }
        }

        self.layer
            .wl_surface()
            .damage_buffer(0, 0, w as i32, h as i32);
        self.layer
            .wl_surface()
            .frame(qh, FrameCallbackData(self.layer.wl_surface().clone()));
        buffer
            .attach_to(self.layer.wl_surface())
            .expect("attach buffer");
        self.layer.commit();
    }
}

impl CompositorHandler for OverlaySpike {
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

impl LayerShellHandler for OverlaySpike {
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
        self.width = NonZeroU32::new(configure.new_size.0).map_or(BADGE_W, NonZeroU32::get);
        self.height = NonZeroU32::new(configure.new_size.1).map_or(BADGE_H, NonZeroU32::get);
        println!(
            "spike: configure recebido ({}x{}) — layer-shell OK",
            self.width, self.height
        );
        if self.first_configure {
            self.first_configure = false;
            self.draw(qh);
        }
    }
}

impl OutputHandler for OverlaySpike {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }
    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

impl ShmHandler for OverlaySpike {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(OverlaySpike);

impl ProvidesRegistryState for OverlaySpike {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(OverlaySpike);
