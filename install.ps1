# install.ps1 — Instalação do dictation-rs no Windows nativo
[CmdletBinding()]
param(
    [string]$InstallDir = "$env:LOCALAPPDATA\dictation\bin",
    [switch]$SkipBuild,
    [switch]$NoStartup
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

$ScriptDir = if ($PSScriptRoot) { $PSScriptRoot } else { (Get-Location).Path }
$releaseDir = Join-Path $ScriptDir "target\release"
$cliExe = Join-Path $releaseDir "dictation.exe"
$daemonExe = Join-Path $releaseDir "dictationd.exe"

if (-not (Test-Path $cliExe) -or -not (Test-Path $daemonExe)) {
    Write-Error "Binários não encontrados em $releaseDir. Execute sem -SkipBuild primeiro para compilar."
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

# 4. Criar diretórios de configuração e arquivos de modelo (schema idêntico ao config.ini.example)
$configDir = Join-Path $env:APPDATA "dictation"
if (-not (Test-Path $configDir)) {
    New-Item -ItemType Directory -Path $configDir -Force | Out-Null
}

$configFile = Join-Path $configDir "config.ini"
$needsConfigUpdate = (-not (Test-Path $configFile))
if (Test-Path $configFile) {
    $existing = Get-Content -Path $configFile -Raw -ErrorAction SilentlyContinue
    if ($existing -like "*[general]*") {
        # Corrige configuração anterior com seções inválidas
        $needsConfigUpdate = $true
    }
}

if ($needsConfigUpdate) {
    Write-Host "==> Gerando $configFile com o schema oficial [dictation]/[paths]/[local]..." -ForegroundColor Yellow
    @'
; ==============================================================================
; config.ini — ditado por voz (dictation-rs)
; ==============================================================================
[dictation]
provider = groq
language = pt
model = whisper-large-v3
device =
capture = cpal
max_seconds = 180
inject = type
type_delay_ms = 6
trailing_space = true
notify = true
indicator = true
indicator_anchor = bottom-center
preview = false
preview_interval_ms = 1500
preview_anchor = bottom-center
segment_seconds = 10.0
overlap_seconds = 0.5
min_segment_seconds = 0.3

[paths]
vault = %APPDATA%\anubis\groq.env
vocab = %APPDATA%\dictation\vocab.txt
corrections = %APPDATA%\dictation\corrections.tsv
state_dir = %LOCALAPPDATA%\dictation

[local]
whisper_bin =
model =
threads = 4
'@ | Set-Content -Path $configFile -Encoding utf8
    Write-Host "    [OK] Configurado $configFile" -ForegroundColor Green
}

# Copiar vocabulário e correções de exemplo se não existirem
$vocabDst = Join-Path $configDir "vocab.txt"
$vocabSrc = Join-Path $ScriptDir "vocab.txt.example"
if (-not (Test-Path $vocabDst) -and (Test-Path $vocabSrc)) {
    Copy-Item -Path $vocabSrc -Destination $vocabDst
    Write-Host "    [OK] Criado $vocabDst" -ForegroundColor Green
}

$correctionsDst = Join-Path $configDir "corrections.tsv"
$correctionsSrc = Join-Path $ScriptDir "corrections.tsv.example"
if (-not (Test-Path $correctionsDst) -and (Test-Path $correctionsSrc)) {
    Copy-Item -Path $correctionsSrc -Destination $correctionsDst
    Write-Host "    [OK] Criado $correctionsDst" -ForegroundColor Green
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

# 6. Configurar inicialização automática com o Windows (Startup)
if (-not $NoStartup) {
    $startupFolder = [Environment]::GetFolderPath("Startup")
    if (-not $startupFolder -or -not (Test-Path $startupFolder)) {
        $startupFolder = Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\Startup"
    }
    if (Test-Path $startupFolder) {
        $shortcutPath = Join-Path $startupFolder "dictationd.lnk"
        Write-Host "==> Configurando inicialização automática com o Windows em $shortcutPath..." -ForegroundColor Yellow
        $wsh = New-Object -ComObject WScript.Shell
        $shortcut = $wsh.CreateShortcut($shortcutPath)
        $shortcut.TargetPath = (Join-Path $InstallDir "dictationd.exe")
        $shortcut.WorkingDirectory = $InstallDir
        $shortcut.Description = "Dictation daemon (push-to-talk voice typing)"
        $shortcut.Save()
        Write-Host "    [OK] Atalho de inicialização automática criado em: $shortcutPath" -ForegroundColor Green
    }
}

Write-Host ""
Write-Host "=== dictation-rs instalado com sucesso! ===" -ForegroundColor Green
Write-Host "Como usar:"
Write-Host "  1. A chave Groq está configurada no cofre '$vaultFile' ou via GROQ_API_KEY."
Write-Host "  2. O daemon (dictationd) iniciará automaticamente com o Windows (ou execute 'dictationd' agora)."
Write-Host "     - O daemon registra os atalhos globais F8 (Gravar/Parar) e F9 (Ensinar correção)."
Write-Host "     - Exibe ícone na bandeja do sistema (System Tray) e overlay REC flutuante."
Write-Host "  3. Ou use o cliente CLI: dictation toggle | dictation status | dictation quit"
