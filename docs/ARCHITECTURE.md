# Arquitetura do dictation-rs

## Objetivo
Reescrever o `dictation` (Python/PySide6) em Rust com **paridade de funcionalidades** e,
principalmente, **eliminar por construção** os bugs de fala longa:

- preview "pesada" (re-transcrição do áudio acumulado a cada tick — custo quadrático);
- **texto antigo injetado no meio do novo** (arquivos de gravação compartilhados entre sessões);
- injeção final assíncrona e fora de ordem.

## Separação em crates

| Crate | Responsabilidade | Depende de plataforma? |
|---|---|---|
| `dictation-core` | máquina de estados, sessões, segmentação append-only, contrato ASR, config, correções, WAV | **não** (100% testável) |
| `dictation-groq` | implementação do `AsrEngine` para o Groq | não (só rede) |
| `dictation-platform` | traits + implementações por SO: `AudioCapture`, `TextInjector`, `HotkeyProvider`, `OverlayHost`, `TrayHost` | sim (feature-gated) |
| `dictation-daemon` | liga tudo + UI (egui/winit) + servidor de socket | sim |
| `dictation-cli` | cliente fino que fala com o daemon | mínimo |

O **core não conhece** X11/Wayland/Windows — por isso é portável e testável sem display.

## Invariantes (garantidos por testes)

1. **Uma captura por vez** — `Machine` só aceita `Start` em `Idle`.
2. **Sessões isoladas** — cada gravação tem um `SessionId`; buffers/arquivos são por sessão
   (nunca reaproveitados), então uma sessão antiga não contamina a nova.
3. **Transcript append-only** — `Transcript` só cresce; o texto final é a concatenação dos
   segmentos na ordem de produção. Impossível "voltar" texto antigo.
4. **Prévia nunca injeta** — só a finalização injeta, e **exatamente uma vez**, serializada.
5. **Segmentação sem perda** — a união dos intervalos dos segmentos cobre todo o áudio
   (propriedade testada com `proptest`), com custo **linear** mesmo em falas longas.
6. **Cancelamento determinístico** — requisições em voo são canceladas/superadas no `stop`.

## Segmentação append-only
Em vez de reenviar o áudio acumulado, dividimos em segmentos de ~`segment_seconds` (com
`overlap_seconds` de sobreposição) e transcrevemos **cada segmento uma vez**, concatenando.
Isso torna o custo linear e o texto monotônico.

> v1 usa janelas fixas com pequena sobreposição. VAD (corte por silêncio) é uma melhoria
> futura para evitar corte no meio de palavra; a interface já está isolada para permitir isso.

## Suporte a Plataformas: Linux e Windows Nativo
Cada capacidade é isolada em traits ou abstrações condicionais (`#[cfg(target_os = ...)]`):

- **Áudio** (`AudioCapture`): `cpal` cobre Linux (ALSA/PulseAudio/JACK) e Windows (WASAPI nativo). No Windows, o padrão é `capture = "cpal"`.
- **Injeção de texto** (`TextInjector`):
  - Linux/X11: `xdotool` com `key Uxxxx` ou `xclip` + Ctrl+V.
  - Windows: `SendInput` com `KEYEVENTF_UNICODE` (`WindowsTypeInjector` — sem dependência de layout de teclado ou dead keys) ou `arboard` + Ctrl+V (`WindowsClipboardInjector`).
- **Atalho global**:
  - Linux: KGlobalAccel via scripts de integração.
  - Windows: thread dedicada rodando loop de mensagens Win32 com `RegisterHotKey` (F8 para toggle de gravação e F9 para ensinar correção).
- **Overlay**:
  - Linux/X11: `override_redirect` + `X11WindowType::Tooltip` + `mouse_passthrough`.
  - Windows: janela Win32 sem foco e click-through via `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE`, posicionada sobre a janela em foco detectada via `GetForegroundWindow` e `GetWindowRect`.
- **Bandeja**:
  - Linux: `ksni` (D-Bus StatusNotifierItem).
  - Windows: `Shell_NotifyIconW` nativo com menu popup Win32 e suporte a `WM_COMMAND`.
- **IPC CLI ↔ Daemon**:
  - Linux: Unix domain socket (`/tmp/dictation.sock` ou `~/.local/state/dictation/socket`).
  - Windows: Windows Named Pipe (`\\.\pipe\dictation`).
- **Caminhos de Configuração**:
  - Linux: `~/.config/dictation/config.ini`, cofre `~/.config/anubis/groq.env`.
  - Windows: `%APPDATA%\dictation\config.ini`, `%APPDATA%\anubis\groq.env` (ou variável `GROQ_API_KEY`).

## Testes e qualidade
- unit + **property-based** (`proptest`) para invariantes (máquina de estados, segmentação);
- integração com **ASR mock** (sem rede);
- gates de CI: `fmt`, `clippy -D warnings`, `test`; **mutation testing** (`cargo-mutants`) e
  cobertura mínima.
