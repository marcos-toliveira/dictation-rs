#!/usr/bin/env bash
# ==============================================================================
# set-groq-key.sh — Configura a chave de API do Groq no cofre local de forma segura
# Suporta Linux, WSL e Git Bash no Windows.
# ==============================================================================
set -euo pipefail

KEY="${1:-}"

if [ -z "$KEY" ]; then
    echo -n "Cole sua chave do Groq (a digitação não aparecerá na tela): "
    read -rs KEY
    echo
fi

if [ -z "$KEY" ]; then
    echo "Erro: nenhuma chave informada." >&2
    exit 1
fi

save_vault() {
    local target_dir="$1"
    local target_file="$target_dir/groq.env"
    mkdir -p "$target_dir"
    printf 'GROQ_API_KEY=%s\n' "$KEY" > "$target_file"
    chmod 600 "$target_file" 2>/dev/null || true
    echo "✔ Chave salva com sucesso em: $target_file"
}

# 1. Cofre padrão Linux / Unix (~/.config/anubis/groq.env)
save_vault "$HOME/.config/anubis"

# 2. Se estiver no Windows (Git Bash / MSYS2) ou WSL, grava também nos caminhos do Windows
if [ -n "${APPDATA:-}" ]; then
    # Git Bash
    save_vault "$APPDATA/anubis"
elif [ -d "/mnt/c/Users" ]; then
    # WSL: detecta o usuário Windows padrão
    WIN_USER=$(cmd.exe /c "echo %USERNAME%" 2>/dev/null | tr -d '\r' || true)
    if [ -n "$WIN_USER" ] && [ -d "/mnt/c/Users/$WIN_USER" ]; then
        save_vault "/mnt/c/Users/$WIN_USER/AppData/Roaming/anubis"
        save_vault "/mnt/c/Users/$WIN_USER/.config/anubis"
    fi
fi

if [ -n "${USERPROFILE:-}" ]; then
    save_vault "$USERPROFILE/.config/anubis"
fi

echo
echo "Tudo pronto! O daemon dictationd carregará a chave automaticamente do cofre."
