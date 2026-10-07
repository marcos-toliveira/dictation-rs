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

## Portas abertas: Wayland e Windows
Cada capacidade é um trait, com `feature = "x11" | "wayland" | "windows"`:

- **Áudio** (`AudioCapture`): `cpal` cobre Linux (ALSA/PulseAudio — funciona também no
  Wayland) e Windows (WASAPI). É **independente do display server**.
- **Injeção de texto** (`TextInjector`): X11 = XTest (`x11rb`); Wayland = `wtype`/libei
  (futuro); Windows = `SendInput` (futuro).
- **Atalho global** (`HotkeyProvider`): X11 = `XGrabKey`; Wayland = KGlobalAccel/portal
  (futuro); Windows = `RegisterHotKey` (futuro).
- **Overlay** (`OverlayHost`): X11 = janela click-through via input shape; Wayland =
  `wl_surface.set_input_region` (futuro); Windows = janela em camadas `WS_EX_TRANSPARENT` (futuro).
- **Bandeja** (`TrayHost`): crate `tray-icon` (SNI no Linux, `Shell_NotifyIcon` no Windows).

O primeiro alvo é **Linux/X11**; Wayland e Windows ficam como backends a implementar nas
máquinas correspondentes, sem tocar no core.

## Testes e qualidade
- unit + **property-based** (`proptest`) para invariantes (máquina de estados, segmentação);
- integração com **ASR mock** (sem rede);
- gates de CI: `fmt`, `clippy -D warnings`, `test`; **mutation testing** (`cargo-mutants`) e
  cobertura mínima.
