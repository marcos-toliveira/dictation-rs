# install.ps1 — Instalação do dictation-rs no Windows nativo
[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\dictation\bin",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

Write-Host "==> Instalando dictation-rs no Windows..." -ForegroundColor Cyan

# 1. Compilar os binários em modo release, a menos que --SkipBuild seja passado
if (-not $SkipBuild) {
    Write-Host "==> Compilando workspace com cargo --release..." -ForegroundColor Yellow
    cargo build --release --workspace
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Falha na compilação do Cargo."
        exit 1
    }
}

$releaseDir = Join-Path $PSScriptRoot "target\release"
$cliExe = Join-Path $releaseDir "dictation.exe"
$daemonExe = Join-Path $releaseDir "dictationd.exe"

if (-not (Test-Path $cliExe) -or -not (Test-Path $daemonExe)) {
    Write-Error "Binários não encontrados em $releaseDir. Execute sem -SkipBuild primeiro."
    exit 1
}

# 2. Criar diretório de destino e copiar binários
if (-not (Test-Path $InstallDir)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

Write-Host "==> Copiando binários para $InstallDir..." -ForegroundColor Yellow
Copy-Item -Path $cliExe -Destination $InstallDir -Force
Copy-Item -Path $daemonExe -Destination $InstallDir -Force

# 3. Adicionar InstallDir ao PATH do Usuário se ainda não estiver
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
if ($userPath -notlike "*$InstallDir*") {
    Write-Host "==> Adicionando $InstallDir ao PATH do Usuário..." -ForegroundColor Yellow
    [Environment]::SetEnvironmentVariable("Path", "$userPath;$InstallDir", "User")
    $env:PATH = "$InstallDir;$env:PATH"
    Write-Host "    [OK] Adicionado ao PATH do usuário." -ForegroundColor Green
} else {
    Write-Host "    [OK] Diretório já está no PATH." -ForegroundColor Green
}

# 4. Criar diretórios de configuração e arquivos de modelo se não existirem
$configDir = Join-Path $env:APPDATA "dictation"
if (-not (Test-Path $configDir)) {
    New-Item -ItemType Directory -Path $configDir -Force | Out-Null
}

$configFile = Join-Path $configDir "config.ini"
if (-not (Test-Path $configFile)) {
    Write-Host "==> Criando modelo de config em $configFile..." -ForegroundColor Yellow
    @'
[general]
provider = groq
language = pt
model = whisper-large-v3
device = default
capture = cpal
max_seconds = 600
inject = type

[ui]
indicator = true
indicator_anchor = top-center
preview = false
preview_interval_ms = 500

[paths]
history_dir = %LOCALAPPDATA%\dictation\history
corrections_file = %APPDATA%\dictation\corrections.tsv
'@ | Set-Content -Path $configFile -Encoding utf8
    Write-Host "    [OK] Criado $configFile" -ForegroundColor Green
}

# 5. Criar diretório do cofre da chave Groq
$vaultDir = Join-Path $env:APPDATA "anubis"
if (-not (Test-Path $vaultDir)) {
    New-Item -ItemType Directory -Path $vaultDir -Force | Out-Null
}
$vaultFile = Join-Path $vaultDir "groq.env"
if (-not (Test-Path $vaultFile)) {
    Write-Host "==> Criando modelo de cofre em $vaultFile..." -ForegroundColor Yellow
    @'
# Insira sua chave de API do Groq abaixo:
GROQ_API_KEY=gsk_sua_chave_aqui
'@ | Set-Content -Path $vaultFile -Encoding utf8
    Write-Host "    [OK] Criado $vaultFile (adicione sua chave Groq aqui ou na variável de ambiente GROQ_API_KEY)" -ForegroundColor Green
}

Write-Host ""
Write-Host "=== dictation-rs instalado com sucesso! ===" -ForegroundColor Green
Write-Host "Como usar:"
Write-Host "  1. Configure sua chave Groq em '$vaultFile' ou defina a variável de ambiente GROQ_API_KEY."
Write-Host "  2. Inicie o daemon em segundo plano executando: dictationd"
Write-Host "     - O daemon registra os atalhos globais F8 (Gravar/Parar) e F9 (Ensinar correção)."
Write-Host "     - Exibe ícone na bandeja do sistema (System Tray) e overlay REC flutuante."
Write-Host "  3. Ou use o cliente CLI: dictation toggle | dictation status | dictation quit"
