#!/usr/bin/env bash
# ==============================================================================
# install.sh — instala o dictation-rs (Rust) para o usuário atual
#   • binários -> ~/.local/bin/{dictationd,dictation}
#   • config   -> ~/.config/dictation/{config.ini,vocab.txt,corrections.tsv}
#   • autostart -> ~/.config/autostart/dictationd.desktop
# Não sobrescreve configs existentes.
#
# ATENÇÃO: isto SOBRESCREVE os binários do dictation em Python (~/.local/bin),
# que usam os mesmos nomes. Se o daemon Python estiver rodando, pare-o antes:
#     pkill -f "python3 .*dictationd"   # (ou encerre pela bandeja)
# ==============================================================================
set -euo pipefail
SRC="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$HOME/.local/bin"
CFG_DIR="$HOME/.config/dictation"
STATE_DIR="$HOME/.local/state/dictation"
AUTO_DIR="$HOME/.config/autostart"

echo "==> compilando (release)…"
( cd "$SRC" && cargo build --release --workspace )

mkdir -p "$BIN_DIR" "$CFG_DIR" "$STATE_DIR" "$AUTO_DIR"
install -m 755 "$SRC/target/release/dictationd" "$BIN_DIR/dictationd"
install -m 755 "$SRC/target/release/dictation" "$BIN_DIR/dictation"

for pair in "config.ini.example:config.ini" "vocab.txt.example:vocab.txt" "corrections.tsv.example:corrections.tsv"; do
    src="$SRC/${pair%%:*}"; dst="$CFG_DIR/${pair##*:}"
    if [ -e "$dst" ]; then
        echo "mantido: $dst"
    else
        install -m 644 "$src" "$dst"; echo "criado:  $dst"
    fi
done

cat > "$AUTO_DIR/dictationd.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Dictation daemon
Name[pt_BR]=Daemon de ditado
Comment=Daemon quente do ditado por voz (push-to-talk)
Exec=$BIN_DIR/dictationd
Icon=audio-input-microphone
Terminal=false
NoDisplay=true
X-KDE-autostart-after=panel
X-GNOME-Autostart-enabled=true
EOF
echo "criado:  $AUTO_DIR/dictationd.desktop"

echo
echo "✔ instalado (Rust): $BIN_DIR/dictation"
echo
echo "Registre o atalho global (KDE Plasma 6 ou GNOME; detectado pelo desktop):"
echo "  bash \"$SRC/set-shortcut.sh\"            # default F8 (ditar) e F9 (ensinar)"
echo
if [ "${XDG_SESSION_TYPE:-}" = "wayland" ] || [ -n "${WAYLAND_DISPLAY:-}" ]; then
    echo "Sessão Wayland detectada (${XDG_CURRENT_DESKTOP:-?}) — dependências do backend Wayland:"
    need="ydotool wl-copy"
    case "${XDG_CURRENT_DESKTOP:-}" in
        *KDE*) need="$need kdotool kscreen-doctor" ;;  # geometria/overlay só no KWin
        *GNOME*) need="$need xdotool xrandr" ;;        # overlay via XWayland (geometria X11)
    esac
    miss=""
    for b in $need; do
        command -v "$b" >/dev/null 2>&1 || miss="$miss $b"
    done
    if [ -n "$miss" ]; then
        echo "  ⚠ faltam:$miss"
        echo "    Arch:   sudo pacman -S wl-clipboard ydotool kscreen   # kdotool-git (biglinux) ou kdotool (AUR)"
        echo "    Ubuntu: sudo apt install wl-clipboard ydotool xdotool x11-xserver-utils"
        echo "    sudo systemctl enable --now ydotoold  # serviço do ydotool (/dev/uinput)"
    else
        echo "  ✔ dependências presentes: $need"
    fi
    case "${XDG_CURRENT_DESKTOP:-}" in
        *GNOME*) echo "  Nota: o GNOME não tem wlr-layer-shell; o overlay REC roda em XWayland" \
                      "(janela não gerenciada, sem roubar foco) e o ícone da bandeja também indica a gravação." ;;
    esac
    echo
fi
echo "Teste sem atalho:  dictation start ; fale ; dictation stop"
