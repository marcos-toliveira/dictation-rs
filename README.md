# dictation-rs

Push-to-talk voice typing for Linux (X11 today; Wayland/Windows planned), rewritten
in **Rust** from [`dictation`](https://github.com/marcos-toliveira/dictation) (the
original Python/PySide6 implementation).

Same behaviour as the original — **Groq** transcription (`whisper-large-v3-turbo`) with a
local `whisper.cpp` fallback — but with a redesigned core that fixes the bugs seen with
long dictation and leaves clean extension points for other platforms.

> **Status:** core + Groq + platform + daemon + CLI implemented and tested. The
> headless path works end-to-end on Linux/X11 (cpal capture → Groq → inject). The GUI
> (floating REC/preview overlay, tray) and the global hotkeys are the remaining work.

## Why a rewrite

The Python version had three structural problems on long speech:

1. the live preview **re-transcribed the whole accumulated audio** every tick (quadratic
   cost → "heavy");
2. **sessions were not isolated** (shared `rec.raw`/`rec.wav`) — starting a new recording
   before the previous transcription finished clobbered it and injected **old text into the
   new one**;
3. final injection was **asynchronous and unordered**.

Rust alone does not fix these; the **architecture** does. See
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).

## Design in one picture

```
crates/
  dictation-core      pure logic: state machine, sessions, append-only segmentation,
                      ASR trait, config, corrections, WAV  (no platform I/O)
  dictation-groq      Groq AsrEngine (OpenAI-compatible /audio/transcriptions)
  dictation-platform  traits + impls: AudioCapture, TextInjector, HotkeyProvider,
                      OverlayHost, TrayHost  (feature-gated: x11 | wayland | windows)
  dictation-daemon    wiring + UI (egui/winit) + socket server
  dictation-cli       thin client (talks to the daemon socket)
```

Invariants enforced by construction and by tests:

- one capture at a time (state machine);
- per-session buffers (no shared files);
- **append-only** transcript (text never rewrites itself → no "old text in the middle");
- preview never injects; final injection happens **exactly once**, serialized;
- segmentation is **lossless** (every sample belongs to a segment) and linear-cost.

## Build & test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Roadmap

- [x] `dictation-core` (state machine, sessions, segmentation, ASR contract, config, corrections)
- [ ] `dictation-groq`
- [ ] `dictation-platform` (X11: cpal audio, XTest inject, XGrabKey hotkey, click-through overlay, tray)
- [ ] `dictation-daemon` + `dictation-cli`
- [ ] Wayland backend (feature-gated, later — on a Wayland machine)
- [ ] Windows backend (feature-gated, later — on a Windows machine)

## License

MIT — see [LICENSE](LICENSE).
