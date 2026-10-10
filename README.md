# dictation-rs

Push-to-talk voice typing for Linux (X11 and Wayland) and Windows, rewritten
in **Rust** from [`dictation`](https://github.com/marcos-toliveira/dictation) (the
original Python/PySide6 implementation).

Same behaviour as the original — **Groq** transcription (`whisper-large-v3-turbo`) with a
local `whisper.cpp` fallback — but with a redesigned core that fixes the bugs seen with
long dictation and leaves clean extension points for other platforms.

> **Status:** core + Groq + platform + daemon + CLI implemented and tested. Linux/X11
> works end-to-end (capture → Groq → inject) with the floating overlay, tray and
> KGlobalAccel hotkeys. **Wayland (Plasma/KWin)** is implemented — `ydotool`/`wl-copy`
> injection, KWin geometry (`kdotool`/`kscreen-doctor`) and a layer-shell overlay
> (egui/wgpu) — and **Windows** is native.

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

## Running (Linux/X11)

```bash
# 1) chave do Groq no cofre 0600
printf 'GROQ_API_KEY=%s\n' "$YOUR_KEY" > ~/.config/anubis/groq.env && chmod 600 ~/.config/anubis/groq.env
# 2) config (ver config.example.toml)
mkdir -p ~/.config/dictation-rs && cp config.example.toml ~/.config/dictation-rs/config.toml
# 3) subir o daemon e usar o cliente
cargo run -p dictation-daemon --bin dictationd &
cargo run -p dictation-cli -- toggle      # inicia/para; ao parar, digita no app em foco
```

## Running (Linux/Wayland)

O daemon detecta a sessão em runtime (`WAYLAND_DISPLAY`/`XDG_SESSION_TYPE`) e, no
Wayland, usa os backends nativos. Dependências (Arch/Plasma):

```bash
sudo pacman -S wl-clipboard ydotool kscreen kdotool-git   # kdotool-git está no repo biglinux; no Arch puro é AUR
sudo systemctl enable --now ydotoold              # serviço do ydotool (/dev/uinput)
```

- **Injeção**: `clipboard` usa `wl-copy` + `Ctrl+V` (via `ydotool`); `type` usa
  `ydotool type` (ASCII — texto com acentos cai para o clipboard).
- **Overlay**: layer-shell (`zwlr_layer_shell_v1`) + `egui`/`wgpu`, ancorado na janela
  em foco (`kdotool`), click-through e sem roubo de foco.
- **Atalhos**: `set-shortcut.sh` (KGlobalAccel) funciona no Plasma Wayland.

## Running (Windows Nativo)

### Instalação rápida via PowerShell:

```powershell
.\install.ps1
```

O script compila os binários em modo release, instala-os em `%LOCALAPPDATA%\dictation\bin` (adicionando ao `PATH`), e gera os modelos de configuração em `%APPDATA%\dictation\config.ini` e `%APPDATA%\anubis\groq.env`.

### Configuração da chave Groq:
Defina a variável de ambiente `GROQ_API_KEY` ou insira a chave no cofre `%APPDATA%\anubis\groq.env` (ou `%USERPROFILE%\.config\anubis\groq.env`).

### Execução:
1. Inicie o daemon:
   ```powershell
   dictationd
   ```
2. **Atalhos Globais (automáticos no daemon via `RegisterHotKey`):**
   - **`F8`**: Gravar / Parar (Push-to-talk ou Toggle). Ao parar, transcreve com Groq e digita diretamente na janela em foco via `SendInput` (com suporte integral a Unicode e acentuação).
   - **`F9`**: Ensinar correção (`teach`). Captura o texto selecionado e abre diálogo para registrar substituição permanente.
3. **Controle via CLI ou Bandeja:**
   - Ícone nativo na bandeja do sistema (`System Tray`) com menu de contexto e indicador de status: **microfone** quando ocioso e **círculo vermelho** durante a gravação. No Windows 11 o ícone é promovido automaticamente para aparecer ao lado do relógio.
   - CLI via Named Pipe (`\\.\pipe\dictation`):
     ```powershell
     dictation toggle    # Iniciar/parar gravação
     dictation status    # Consultar estado atual
     dictation quit      # Encerrar daemon
     ```

## Roadmap

- [x] `dictation-core` (state machine, sessions, append-only segmentation, ASR contract, config, corrections)
- [x] `dictation-groq` (multipart + mock HTTP tests)
- [x] `dictation-platform` (cpal audio, X11 inject via xdotool, traits for overlay/tray/hotkey)
- [x] `dictation-daemon` + `dictation-cli` (socket / named pipe, end-to-end validated)
- [x] GUI: floating REC/preview overlay + tray (egui/eframe + ksni no Linux; Win32 Shell_NotifyIcon no Windows)
- [x] Global hotkeys (F8/F9) via KGlobalAccel no Linux e `RegisterHotKey` no Windows
- [x] `teach` (dialog via zenity/kdialog no Linux e InputBox no Windows + CLI) e local whisper.cpp fallback
- [x] Windows backend nativo (`feat/windows`: cpal WASAPI, SendInput Unicode / arboard clipboard, Named Pipes, RegisterHotKey, overlay sem roubo de foco)
- [x] Wayland backend (`feat/wayland`: `ydotool`/`wl-copy` inject, KWin geometry via `kdotool`/`kscreen-doctor`, layer-shell overlay via egui/wgpu)

## License

MIT — see [LICENSE](LICENSE).
