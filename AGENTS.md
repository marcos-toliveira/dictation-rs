# AGENTS.md — dictation-rs

Guia para agentes que trabalham **neste** repositório. Vale junto com o `generic-dev/AGENTS.md` do workspace.

## O que é
Reescrita em **Rust** do `dictation` (Python/PySide6): ditado por voz **push-to-talk** —
atalho → grava → transcreve no **Groq** → digita no app em foco. Original (referência, **não editar**):
<https://github.com/marcos-toliveira/dictation>.

- **Alvo atual:** Linux/**X11** (funcionando).
- **Planejado:** Windows nativo (branch `feat/windows`).

## Arquitetura (workspace Cargo)
| Crate | Papel | Depende de SO? |
|---|---|---|
| `dictation-core` | puro/testável: máquina de estados, sessões, config, correções, WAV, contrato de ASR | não |
| `dictation-groq` | `AsrEngine` do Groq (`/audio/transcriptions`, multipart) | não |
| `dictation-platform` | traits + impls por SO (`AudioCapture`, `TextInjector`); features `audio-cpal`, `x11` | sim |
| `dictation-ui` | overlay (eframe/egui) + bandeja (`ksni`, Linux/SNI) | sim |
| `dictation-daemon` | liga tudo (overlay na thread principal; threads: socket, captura, bandeja). Binário `dictationd` | sim |
| `dictation-cli` | cliente fino via socket. Binário `dictation` | mínimo |

## Comandos
```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo build --release --workspace
```

## Invariantes (não quebrar)
1. **Uma captura por vez** (máquina de estados proíbe `Start` durante gravação).
2. **Sessão isolada** (`SessionId`) — nenhum estado/arquivo compartilhado entre gravações.
3. **Injeção única** e serializada (só na finalização).
4. **Transcrição do áudio INTEIRO numa única chamada** ao Groq no `stop` (não segmentar) — blocos de ~10 min por causa do teto de 25 MB.
5. Prévia ao vivo é **opcional e desligada** por padrão (estoura o rate limit do Groq).

## Groq (free tier)
- Limites **por organização**: **20 req/min**, 2.000 req/dia, 7.200 s de áudio/hora, 28.800 s/dia; cada requisição conta **mínimo 10 s**.
- Hoje: **1 requisição por ditado** (áudio inteiro). Modelo padrão: **`whisper-large-v3`**.
- Chave no cofre `~/.config/anubis/groq.env` (`chmod 600`). **Nunca** em argv/commit/chat.

## Injeção de texto (X11)
- **`clipboard`** (padrão): `xclip` + `Ctrl+V` — **atômico e exato** (acentos ok); exige campo de texto focado.
- **`type`**: `xdotool` ASCII + `key Uxxxx` para acentos — universal, mas **frágil em Chromium/Electron** (perde caractere/acento). Sempre há um delay inicial (~150 ms).
- Config: `inject = type | clipboard | stdout`.

## Overlay / bandeja / atalhos
- Overlay: eframe/egui **frameless, translúcido, click-through, sem roubar foco** (X11: `override_redirect` + `X11WindowType::Tooltip` + `mouse_passthrough`). Estados: **REC** → **"transcrevendo…"**.
- Bandeja: `ksni` (StatusNotifierItem do KDE, sem GTK). Menu + estado. No Windows: `Shell_NotifyIconW`, com ícone **microfone/record desenhado via GDI** (sem assets) e auto-promovido.
- Atalhos: **KGlobalAccel** (Plasma 6) via `set-shortcut.sh` (F8 ditar / F9 ensinar).

## Config
`~/.config/dictation/config.ini` (mesmo formato do Python — troca drop-in). Chaves principais:
`provider`, `language`, `model`, `device`, `capture` (`ffmpeg`|`cpal`|auto), `max_seconds`,
`inject`, `indicator`/`indicator_anchor`, `preview`/`preview_interval_ms` (desligado), `[paths]`, `[local]`.

## Gotchas (aprendidos no Linux)
- **Acento digitado** é frágil (teclas mortas). Preferir `clipboard` em app web.
- **cpal/ALSA** desta máquina falha ~10% → no Linux o padrão é **ffmpeg**; no Windows usar **cpal** (WASAPI).
- Overlay **não pode roubar foco** (senão a injeção erra o alvo).
- O daemon **não pode bloquear** a thread de captura em HTTP (evita travar a captura).
- **Captura nunca bloqueia indefinidamente**: `recv`/`read` usam espera limitada (`recv_timeout`); a thread de captura é `join()`ada sob o mutex do daemon — bloqueio sem timeout congela o pipe/watchdog e prende o badge REC. Bloco vazio = "sem dados" (recheca `stop`); `None` = fim.
- **Bandeja no Windows**: o Windows 11 joga ícones novos no *overflow* (seta `^`). O app grava `IsPromoted=1` em `HKCU\Control Panel\NotifyIconSettings` (só se o usuário nunca escolheu) e re-adiciona o ícone; também trata `TaskbarCreated` para re-adicioná-lo se o Explorer reiniciar.
- **Overlay no Windows**: `with_mouse_passthrough(true)` força `WS_EX_LAYERED` e o **glow/OpenGL não apresenta** (a janela aparece vazia). Não usar no Windows; o click-through/sem-foco vem de `WS_EX_TRANSPARENT | WS_EX_NOACTIVATE` aplicado por `SetWindowLongPtr` a cada quadro (o winit reaplica estilos ao processar comandos de viewport).

## Regras
- **NUNCA** adicionar tags/assinaturas de coautoria de IA nos commits.
- **Não editar** o legado Python (é o oráculo).
- **Não quebrar o Linux** ao adicionar Windows: tudo atrás de `cfg(windows)`/feature.
- Segredos **nunca** em argv/commit.
- Ao mexer no daemon em uso: **avisar antes** de reiniciar (o operador pode estar gravando).
- Ao abrir um PR: **solicitar a review do Copilot** (`gh pr create --reviewer @copilot` / `gh pr edit <PR> --add-reviewer @copilot`); re-solicitar após push (ele não re-revisa sozinho) e tratar achados **High/Medium** antes do merge.

## Distribuição
- **AUR**: `packaging/PKGBUILD` (+ `.SRCINFO`, `.install`). Publicação: ver `packaging/README.md`.
- Tag de release → atualizar `pkgver`/`sha256sums` → `makepkg --printsrcinfo > .SRCINFO`.
