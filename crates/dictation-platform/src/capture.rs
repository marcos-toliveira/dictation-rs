//! Captura de áudio via `cpal` — portável (Linux: ALSA/PulseAudio; Windows: WASAPI).
//!
//! Entrega blocos **mono `i16` a 16 kHz** (downmix de canais + reamostragem linear),
//! que é o formato que o Whisper/Groq espera. Não depende do display server (funciona
//! também no Wayland).

use crate::{AudioCapture, CaptureError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};

/// Taxa de amostragem alvo do pipeline.
pub const TARGET_RATE: u32 = 16_000;

/// Captura via `cpal`.
pub struct CpalCapture {
    stream: Option<cpal::Stream>,
    rx: Option<Receiver<Vec<i16>>>,
    device_name: Option<String>,
}

impl CpalCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            rx: None,
            device_name: None,
        }
    }

    /// Seleciona um dispositivo de entrada por nome (substring). Sem isso, usa o padrão.
    pub fn with_device(mut self, name: impl Into<String>) -> Self {
        self.device_name = Some(name.into());
        self
    }

    fn pick_device(&self, host: &cpal::Host) -> Result<cpal::Device, CaptureError> {
        // Sem nome explícito: prefere backends que funcionam bem com PipeWire/PulseAudio
        // (o ALSA "default" de algumas máquinas é problemático).
        let wants: Vec<String> = match &self.device_name {
            Some(n) => vec![n.clone()],
            None => default_candidates(),
        };
        if !wants.is_empty() {
            if let Ok(devices) = host.input_devices() {
                let devices: Vec<cpal::Device> = devices.collect();
                for want in &wants {
                    if let Some(d) = devices.iter().find(|d| {
                        d.name()
                            .map(|n| n == *want || n.contains(want.as_str()))
                            .unwrap_or(false)
                    }) {
                        return Ok(d.clone());
                    }
                }
            }
            if let Some(explicit) = &self.device_name {
                return Err(CaptureError::Device(format!(
                    "dispositivo '{explicit}' não encontrado"
                )));
            }
        }
        host.default_input_device()
            .ok_or_else(|| CaptureError::Device("nenhum dispositivo de entrada padrão".into()))
    }
}

impl Default for CpalCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioCapture for CpalCapture {
    fn start(&mut self) -> Result<(), CaptureError> {
        let host = cpal::default_host();
        let device = self.pick_device(&host)?;
        if let Ok(name) = device.name() {
            tracing::info!(device = %name, "dispositivo de captura");
        }

        let (tx, rx) = mpsc::channel::<Vec<i16>>();

        // Tenta primeiro o default_input_config().
        // No Windows/WASAPI, alguns dispositivos reportam F32 como padrão mas falham com AUDCLNT_E_UNSUPPORTED_FORMAT (0x88890008),
        // funcionando perfeitamente em I16 ou I32.
        let mut last_err = None;
        if let Ok(def_cfg) = device.default_input_config() {
            match try_build_stream(&device, &def_cfg, tx.clone()) {
                Ok(stream) => {
                    stream
                        .play()
                        .map_err(|e| CaptureError::Stream(e.to_string()))?;
                    self.stream = Some(stream);
                    self.rx = Some(rx);
                    return Ok(());
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }

        // Fallback: busca em supported_input_configs() (priorizando I16 e F32)
        if let Ok(supported) = device.supported_input_configs() {
            let mut configs: Vec<_> = supported.map(|c| c.with_max_sample_rate()).collect();
            // Prioriza I16 no fallback por ser o formato mais universal em interfaces de áudio USB
            configs.sort_by_key(|c| match c.sample_format() {
                cpal::SampleFormat::I16 => 0,
                cpal::SampleFormat::F32 => 1,
                cpal::SampleFormat::I32 => 2,
                _ => 3,
            });

            for cfg in configs {
                match try_build_stream(&device, &cfg, tx.clone()) {
                    Ok(stream) => {
                        stream
                            .play()
                            .map_err(|e| CaptureError::Stream(e.to_string()))?;
                        self.stream = Some(stream);
                        self.rx = Some(rx);
                        return Ok(());
                    }
                    Err(e) => {
                        last_err = Some(e);
                    }
                }
            }
        }

        Err(last_err.unwrap_or_else(|| {
            CaptureError::Device(
                "não foi possível iniciar stream em nenhuma configuração suportada".into(),
            )
        }))
    }

    fn recv(&mut self) -> Option<Vec<i16>> {
        // Espera **limitada**: se o stream parar de entregar (troca/erro de dispositivo,
        // callback morto), não bloqueia para sempre — devolve um bloco vazio para o laço
        // do daemon rechecar a flag de `stop` e encerrar.
        let rx = self.rx.as_ref()?;
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(block) => Some(block),
            Err(RecvTimeoutError::Timeout) => Some(Vec::new()),
            Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.stream = None; // drop encerra a captura
        self.rx = None;
        Ok(())
    }
}

fn try_build_stream(
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    tx: mpsc::Sender<Vec<i16>>,
) -> Result<cpal::Stream, CaptureError> {
    let sample_format = config.sample_format();
    let in_rate = config.sample_rate().0;
    let channels = config.channels() as usize;
    let stream_config: cpal::StreamConfig = config.clone().into();
    let err_fn = |e| tracing::error!(error = %e, "cpal stream error");

    match sample_format {
        cpal::SampleFormat::F32 => {
            build::<f32>(device, &stream_config, channels, in_rate, tx, err_fn)
        }
        cpal::SampleFormat::I16 => {
            build::<i16>(device, &stream_config, channels, in_rate, tx, err_fn)
        }
        cpal::SampleFormat::U16 => {
            build::<u16>(device, &stream_config, channels, in_rate, tx, err_fn)
        }
        cpal::SampleFormat::I32 => {
            build::<i32>(device, &stream_config, channels, in_rate, tx, err_fn)
        }
        other => Err(CaptureError::Stream(format!(
            "formato de amostra não suportado: {other:?}"
        ))),
    }
}

/// Converte amostras de um tipo para `f32` em [-1, 1].
trait ToF32 {
    fn to_f32(self) -> f32;
}
impl ToF32 for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}
impl ToF32 for i16 {
    fn to_f32(self) -> f32 {
        self as f32 / 32768.0
    }
}
impl ToF32 for u16 {
    fn to_f32(self) -> f32 {
        (self as f32 - 32768.0) / 32768.0
    }
}
impl ToF32 for i32 {
    fn to_f32(self) -> f32 {
        self as f32 / 2147483648.0
    }
}

/// Reamostrador linear simples para 16 kHz.
struct Resampler {
    step: f64,
    pos: f64,
    buf: Vec<f32>,
}

impl Resampler {
    fn new(in_rate: u32) -> Self {
        Self {
            step: in_rate as f64 / TARGET_RATE as f64,
            pos: 0.0,
            buf: Vec::new(),
        }
    }

    fn process(&mut self, input: &[f32]) -> Vec<i16> {
        self.buf.extend_from_slice(input);
        let mut out = Vec::new();
        while self.pos + 1.0 < self.buf.len() as f64 {
            let i = self.pos as usize;
            let frac = self.pos - i as f64;
            let s = self.buf[i] as f64 * (1.0 - frac) + self.buf[i + 1] as f64 * frac;
            out.push((s.clamp(-1.0, 1.0) * i16::MAX as f64) as i16);
            self.pos += self.step;
        }
        let drop = self.pos as usize;
        if drop > 0 {
            self.buf.drain(0..drop);
            self.pos -= drop as f64;
        }
        out
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: usize,
    in_rate: u32,
    tx: Sender<Vec<i16>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, CaptureError>
where
    T: cpal::SizedSample + ToF32 + Send + 'static,
{
    let mut resampler = Resampler::new(in_rate);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| {
                let frames = data.len() / channels.max(1);
                let mut mono = Vec::with_capacity(frames);
                for f in 0..frames {
                    let mut acc = 0.0f32;
                    for c in 0..channels {
                        acc += data[f * channels + c].to_f32();
                    }
                    mono.push(acc / channels as f32);
                }
                let out = resampler.process(&mono);
                if !out.is_empty() {
                    let _ = tx.send(out);
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| CaptureError::Stream(e.to_string()))
}

/// Candidatos de dispositivo quando nenhum é informado (Linux: PipeWire/Pulse).
#[cfg(target_os = "linux")]
fn default_candidates() -> Vec<String> {
    vec!["pipewire".into(), "pulse".into()]
}

#[cfg(not(target_os = "linux"))]
fn default_candidates() -> Vec<String> {
    Vec::new()
}

/// Thread leitora do pipe do ffmpeg: lê blocos de `s16le` e os entrega por canal.
///
/// O `read` pode bloquear; por isso fica **fora** da thread de captura (que consome
/// com `recv_timeout`). Ao receber EOF (fonte encerrada) ou quando o receptor some, a
/// thread termina. Extraída para permitir teste com uma fonte controlada.
#[cfg(target_os = "linux")]
fn spawn_reader<R: std::io::Read + Send + 'static>(
    out: R,
    tx: mpsc::Sender<Vec<i16>>,
) -> std::thread::JoinHandle<()> {
    use std::io::Read;
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(out);
        let mut buf = vec![0u8; 3200]; // 100 ms @ 16 kHz mono s16le
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break, // EOF: fonte encerrou
                Ok(n) => {
                    let mut block = buf[..n].to_vec();
                    if block.len() % 2 == 1 {
                        block.pop(); // descarta byte ímpar
                    }
                    let (pairs, _) = block.as_chunks::<2>();
                    let samples: Vec<i16> = pairs.iter().map(|p| i16::from_le_bytes(*p)).collect();
                    if tx.send(samples).is_err() {
                        break; // receptor sumiu
                    }
                }
                Err(_) => break,
            }
        }
    })
}

/// Captura via `ffmpeg` (Linux: PulseAudio/PipeWire) — **robusto** onde o cpal/ALSA
/// desta máquina falha intermitentemente (panic de buffer no backend ALSA).
#[cfg(target_os = "linux")]
pub struct FfmpegCapture {
    source: Option<String>,
    child: Option<std::process::Child>,
    rx: Option<Receiver<Vec<i16>>>,
    reader: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl FfmpegCapture {
    pub fn new() -> Self {
        Self {
            source: None,
            child: None,
            rx: None,
            reader: None,
        }
    }

    /// Fonte PulseAudio/PipeWire (`None` = fonte padrão via `pactl`).
    pub fn with_device(mut self, name: impl Into<String>) -> Self {
        self.source = Some(name.into());
        self
    }
}

#[cfg(target_os = "linux")]
impl Default for FfmpegCapture {
    fn default() -> Self {
        Self::new()
    }
}

/// Fonte padrão do PulseAudio/PipeWire (`pactl get-default-source`), como no Python.
#[cfg(target_os = "linux")]
pub fn pulse_default_source() -> String {
    if let Ok(o) = std::process::Command::new("pactl")
        .args(["get-default-source"])
        .output()
    {
        if o.status.success() {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if !s.is_empty() {
                return s;
            }
        }
    }
    "default".into()
}

#[cfg(target_os = "linux")]
impl AudioCapture for FfmpegCapture {
    fn start(&mut self) -> Result<(), CaptureError> {
        use std::process::{Command, Stdio};
        let source = self.source.clone().unwrap_or_else(pulse_default_source);
        let mut child = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-f",
                "pulse",
                "-i",
                &source,
                "-ar",
                "16000",
                "-ac",
                "1",
                "-f",
                "s16le",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| CaptureError::Device(format!("ffmpeg: {e}")))?;

        // Se o ffmpeg morrer logo no início, a fonte é inválida.
        std::thread::sleep(std::time::Duration::from_millis(300));
        if let Ok(Some(status)) = child.try_wait() {
            return Err(CaptureError::Device(format!(
                "ffmpeg saiu ({status}) — fonte '{source}' inválida?"
            )));
        }

        let out = child
            .stdout
            .take()
            .ok_or_else(|| CaptureError::Stream("ffmpeg sem stdout".into()))?;

        // Thread leitora dedicada: o `read` do pipe pode bloquear, então fica **fora**
        // da thread de captura; esta consome com espera limitada (`recv_timeout`).
        // Quando o ffmpeg morre/é morto, o `read` retorna EOF e a thread encerra.
        let (tx, rx) = mpsc::channel::<Vec<i16>>();
        self.reader = Some(spawn_reader(out, tx));
        self.rx = Some(rx);
        self.child = Some(child);
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<i16>> {
        // Espera limitada (ver `CpalCapture::recv`): o ffmpeg pode ficar vivo sem
        // produzir stdout; um bloco vazio deixa o daemon rechecar o `stop`.
        let rx = self.rx.as_ref()?;
        match rx.recv_timeout(std::time::Duration::from_millis(100)) {
            Ok(block) => Some(block),
            Err(RecvTimeoutError::Timeout) => Some(Vec::new()),
            Err(RecvTimeoutError::Disconnected) => None,
        }
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
        self.rx = None;
        if let Some(h) = self.reader.take() {
            // O kill acima fecha o pipe → `read` retorna EOF → a thread encerra.
            let _ = h.join();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_downmixes_rate() {
        // 48k -> 16k: ~1/3 dos samples
        let mut r = Resampler::new(48_000);
        let input: Vec<f32> = (0..4800).map(|i| (i as f32 / 4800.0) - 0.5).collect();
        let out = r.process(&input);
        assert!(
            (out.len() as i64 - 1600).abs() <= 2,
            "esperava ~1600 samples, veio {}",
            out.len()
        );
    }

    #[test]
    fn resampler_is_identity_at_16k() {
        let mut r = Resampler::new(16_000);
        let input: Vec<f32> = (0..100).map(|i| (i as f32 / 100.0) - 0.5).collect();
        let out = r.process(&input);
        assert!((out.len() as i64 - 99).abs() <= 2, "veio {}", out.len());
    }

    #[test]
    #[cfg(windows)]
    fn cpal_default_input_device_exists() {
        let host = cpal::default_host();
        let dev = host.default_input_device();
        if let Some(d) = dev {
            let _ = d.default_input_config();
            let mut cap = CpalCapture::new();
            if cap.start().is_ok() {
                std::thread::sleep(std::time::Duration::from_millis(300));
                let _ = cap.recv();
                let _ = cap.stop();
            }
        }
    }

    #[test]
    fn cpal_recv_timeout_vazio_e_desconexao_none() {
        // Sender vivo, sem enviar: a espera limitada expira → bloco vazio.
        let (tx, rx) = mpsc::channel::<Vec<i16>>();
        let mut cap = CpalCapture {
            stream: None,
            rx: Some(rx),
            device_name: None,
        };
        assert_eq!(cap.recv(), Some(Vec::new()));
        // Sender dropado: canal desconectado → None (fim da fonte).
        drop(tx);
        assert_eq!(cap.recv(), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn ffmpeg_recv_expira_e_stop_faz_join() {
        use std::io::Read;

        // Leitor controlado que "trava" (como um ffmpeg vivo sem produzir stdout)
        // até ser liberado; então devolve EOF.
        struct Blocked {
            gate: mpsc::Receiver<()>,
        }
        impl Read for Blocked {
            fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
                let _ = self.gate.recv();
                Ok(0) // EOF ao liberar
            }
        }

        let (gate_tx, gate_rx) = mpsc::channel::<()>();
        let (tx, rx) = mpsc::channel::<Vec<i16>>();
        let reader = spawn_reader(Blocked { gate: gate_rx }, tx);
        let mut cap = FfmpegCapture {
            source: None,
            child: None,
            rx: Some(rx),
            reader: Some(reader),
        };

        // Leitor travado → recv expira (≤100ms) e devolve bloco vazio (o daemon
        // recheca o `stop`), em vez de bloquear para sempre.
        assert_eq!(cap.recv(), Some(Vec::new()));

        // Libera o leitor (EOF) e confirma que `stop` faz join da thread leitora.
        gate_tx.send(()).unwrap();
        assert!(cap.stop().is_ok());
    }
}
