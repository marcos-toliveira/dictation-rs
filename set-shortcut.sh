#!/usr/bin/env bash
# ==============================================================================
# set-shortcut.sh — registra/atualiza os atalhos globais conforme o desktop:
#   • KDE Plasma 6 (KGlobalAccel): Plasma 6 removeu o "Comando personalizado"
#     (KHotKeys não foi portado), então usamos .desktop + kglobalshortcutsrc no
#     componente "services".
#   • GNOME (X11 ou Wayland): atalhos personalizados do gsettings
#     (org.gnome.settings-daemon.plugins.media-keys), tratados pelo próprio Mutter.
#
# Uso: set-shortcut.sh [tecla_ditado] [tecla_ensinar]
#   default: F8 (ditar) e F9 (ensinar correção)
# ==============================================================================
set -euo pipefail
KEY="${1:-F8}"
TEACH_KEY="${2:-F9}"
BIN="$HOME/.local/bin/dictation"
APP_DIR="$HOME/.local/share/applications"

[ -x "$BIN" ] || { echo "ERRO: $BIN não existe (rode install.sh primeiro)." >&2; exit 1; }

set_kde() {
    mkdir -p "$APP_DIR"
    write_desktop() { # $1=arquivo $2=nome $3=exec
        cat > "$APP_DIR/$1" <<EOF
[Desktop Entry]
Type=Application
Name=$2
Name[pt_BR]=$2
Comment=Ditado por voz (push-to-talk)
Comment[pt_BR]=Ditado por voz (push-to-talk)
Exec=$3
Icon=audio-input-microphone
Terminal=false
NoDisplay=true
StartupNotify=false
Categories=Utility;
EOF
    }
    write_desktop "dictation.desktop" "Ditado" "$BIN toggle"
    write_desktop "dictation-teach.desktop" "Ditado: ensinar correção" "$BIN teach"

    cp -p "$HOME/.config/kglobalshortcutsrc" "$HOME/.config/kglobalshortcutsrc.bak-$(date +%Y%m%d-%H%M%S)"
    kwriteconfig6 --file kglobalshortcutsrc --group services --group dictation.desktop       --key _launch "$KEY"
    kwriteconfig6 --file kglobalshortcutsrc --group services --group dictation.desktop       --key _k_friendly_name "Ditado"
    kwriteconfig6 --file kglobalshortcutsrc --group services --group dictation-teach.desktop --key _launch "$TEACH_KEY"
    kwriteconfig6 --file kglobalshortcutsrc --group services --group dictation-teach.desktop --key _k_friendly_name "Ensinar correção"
    kbuildsycoca6 --noincremental >/dev/null 2>&1 || true
    systemctl --user restart plasma-kglobalaccel.service

    echo "✔ ditado   = $KEY"
    echo "✔ ensinar  = $TEACH_KEY  (selecione o texto errado e aperte)"
    echo "  (backup do kglobalshortcutsrc salvo em ~/.config/)"
}

# GNOME: acrescenta (sem apagar os existentes) dois atalhos personalizados.
# Idempotente: reexecutar só atualiza nome/comando/tecla dos nossos dois caminhos.
set_gnome() {
    command -v gsettings >/dev/null 2>&1 || { echo "ERRO: gsettings não encontrado." >&2; exit 1; }
    command -v python3 >/dev/null 2>&1 || { echo "ERRO: python3 não encontrado." >&2; exit 1; }
    local schema="org.gnome.settings-daemon.plugins.media-keys"
    local base="/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings"
    local p_toggle="$base/dictation/" p_teach="$base/dictation-teach/"

    local current merged
    current="$(gsettings get "$schema" custom-keybindings)"
    merged="$(python3 - "$current" "$p_toggle" "$p_teach" <<'PY'
import ast, sys
raw = sys.argv[1]
if raw.startswith("@as"):
    raw = raw.split(None, 1)[1]
lst = ast.literal_eval(raw)
for p in sys.argv[2:]:
    if p not in lst:
        lst.append(p)
print(repr(lst))
PY
)"
    echo "  (lista anterior de atalhos personalizados: $current)"
    gsettings set "$schema" custom-keybindings "$merged"

    set_binding() { # $1=caminho $2=nome $3=comando $4=tecla
        local k="$schema.custom-keybinding:$1"
        gsettings set "$k" name "$2"
        gsettings set "$k" command "$3"
        gsettings set "$k" binding "$4"
    }
    set_binding "$p_toggle" "Ditado" "$BIN toggle" "$KEY"
    set_binding "$p_teach" "Ditado: ensinar correção" "$BIN teach" "$TEACH_KEY"

    echo "✔ ditado   = $KEY  ($BIN toggle)"
    echo "✔ ensinar  = $TEACH_KEY  ($BIN teach)"
    echo "  Confira em Configurações > Teclado > Atalhos personalizados (o GNOME"
    echo "  aceita a mesma tecla em dois atalhos)."
}

case "${XDG_CURRENT_DESKTOP:-}" in
    *KDE*) set_kde ;;
    *GNOME*|*Unity*|*ubuntu*) set_gnome ;;
    *)
        echo "ERRO: desktop '${XDG_CURRENT_DESKTOP:-?}' sem suporte neste script (KDE ou GNOME)." >&2
        echo "      Registre manualmente: '$BIN toggle' (ditar) e '$BIN teach' (ensinar)." >&2
        exit 1
        ;;
esac
