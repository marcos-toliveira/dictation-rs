# ==============================================================================
# set-groq-key.ps1 — Configura a chave de API do Groq no cofre local (Windows)
# ==============================================================================
[CmdletBinding()]
param(
    [string]$Key
)

$ErrorActionPreference = "Stop"

if (-not $Key) {
    $sec = Read-Host "Cole sua chave do Groq (a digitacao ficara oculta)" -AsSecureString
    $bstr = [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($sec)
    $Key = [System.Runtime.InteropServices.Marshal]::PtrToStringAuto($bstr)
}

if (-not $Key -or $Key.Trim() -eq "") {
    Write-Error "Nenhuma chave informada."
    exit 1
}

$Key = $Key.Trim()

function Save-Vault($dir) {
    if (-not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    $file = Join-Path $dir "groq.env"
    "GROQ_API_KEY=$Key" | Set-Content -Path $file -Encoding utf8
    Write-Host "[OK] Chave gravada com sucesso em: $file" -ForegroundColor Green
}

# 1. Cofre padrao Windows (%APPDATA%\anubis\groq.env)
Save-Vault (Join-Path $env:APPDATA "anubis")

# 2. Cofre compativel Unix (%USERPROFILE%\.config\anubis\groq.env)
Save-Vault (Join-Path $env:USERPROFILE ".config\anubis")

Write-Host ""
Write-Host "Chave do Groq configurada! O daemon dictationd a utilizara automaticamente." -ForegroundColor Cyan
