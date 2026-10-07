//! Captura de áudio via `cpal` — portável (Linux: ALSA/PulseAudio; Windows: WASAPI).
//!
//! Entrega blocos **mono `i16` a 16 kHz** (downmix de canais + reamostragem linear),
//! que é o formato que o Whisper/Groq espera. Não depende do display server (funciona
//! também no Wayland).

use crate::{AudioCapture, CaptureError};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{self, Receiver, Sender};

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
        let config = device
            .default_input_config()
            .map_err(|e| CaptureError::Device(e.to_string()))?;
        let sample_format = config.sample_format();
        let in_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let stream_config: cpal::StreamConfig = config.into();

        let (tx, rx) = mpsc::channel::<Vec<i16>>();
        let err_fn = |e| tracing::error!(error = %e, "cpal stream error");

        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                build::<f32>(&device, &stream_config, channels, in_rate, tx, err_fn)
            }
            cpal::SampleFormat::I16 => {
                build::<i16>(&device, &stream_config, channels, in_rate, tx, err_fn)
            }
            cpal::SampleFormat::U16 => {
                build::<u16>(&device, &stream_config, channels, in_rate, tx, err_fn)
            }
            other => Err(CaptureError::Stream(format!(
                "formato de amostra não suportado: {other:?}"
            ))),
        }?;

        stream
            .play()
            .map_err(|e| CaptureError::Stream(e.to_string()))?;
        self.stream = Some(stream);
        self.rx = Some(rx);
        Ok(())
    }

    fn recv(&mut self) -> Option<Vec<i16>> {
        self.rx.as_ref()?.recv().ok()
    }

    fn stop(&mut self) -> Result<(), CaptureError> {
        self.stream = None; // drop encerra a captura
        self.rx = None;
        Ok(())
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
}
