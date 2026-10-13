# Evidence Finder - instalador e inicializador para Windows.
# Instala o que faltar (Node.js, Rust, Ferramentas C++, WebView2, Tesseract OCR), compila o app
# na primeira vez e o abre. Nas proximas vezes, apenas abre o app.
param([switch]$Recompilar)

$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
Set-Location $raiz
$log = Join-Path $raiz "instalacao-log.txt"
try { Start-Transcript -Path $log -Force | Out-Null } catch {}

function Titulo($t) { Write-Host ""; Write-Host "=== $t ===" -ForegroundColor Cyan }
function Atualizar-Path {
    $m = [Environment]::GetEnvironmentVariable("Path", "Machine")
    $u = [Environment]::GetEnvironmentVariable("Path", "User")
    $env:Path = "$m;$u;$env:USERPROFILE\.cargo\bin"
}
function Tem($cmd) { return [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }
function Winget-Instalar($id, $extra) {
    $args = @("install", "--id", $id, "-e", "--accept-package-agreements", "--accept-source-agreements")
    if ($extra) { $args += $extra }
    & winget @args
}
function Tem-BuildTools {
    $vw = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path $vw)) { return $false }
    $r = & $vw -products * -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    return [bool]$r
}
function Tem-WebView2 {
    $chave = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
    foreach ($p in @("HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$chave",
                     "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$chave",
                     "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$chave")) {
        if (Test-Path $p) { return $true }
    }
    return $false
}

function Achar-Tesseract {
    $c = @()
    if ($env:EVIDENCE_FINDER_TESSERACT) { $c += $env:EVIDENCE_FINDER_TESSERACT }
    $c += (Join-Path $env:ProgramFiles "Tesseract-OCR\tesseract.exe")
    $cmd = Get-Command tesseract -ErrorAction SilentlyContinue
    if ($cmd) { $c += $cmd.Source }
    foreach ($p in $c) { if ($p -and (Test-Path $p)) { return $p } }
    return $null
}
# OCR e opcional: qualquer falha aqui vira aviso e nunca impede de abrir o app.
function Garantir-Tesseract {
    try {
        $t = Achar-Tesseract
        if (-not $t) {
            if (-not (Tem "winget")) { Write-Host "AVISO: sem winget, o Tesseract (OCR) nao foi instalado. Imagens e PDFs digitalizados nao poderao ser lidos." -ForegroundColor Yellow; return }
            Titulo "Instalando Tesseract OCR (leitura de imagens e PDFs digitalizados)"
            Write-Host "Se o Windows pedir permissao de administrador, clique em Sim."
            Winget-Instalar "UB-Mannheim.TesseractOCR" @("--silent")
            Atualizar-Path
            $t = Achar-Tesseract
        }
        if (-not $t) { Write-Host "AVISO: o Tesseract nao foi encontrado depois da instalacao. O OCR ficara indisponivel." -ForegroundColor Yellow; return }
        $dados = Join-Path (Split-Path -Parent $t) "tessdata"
        $por = Join-Path $dados "por.traineddata"
        if (Test-Path $por) { Write-Host "Tesseract (OCR em portugues): ok"; return }
        Titulo "Baixando o idioma portugues do OCR"
        $tmp = Join-Path $env:TEMP "por.traineddata"
        Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/tesseract-ocr/tessdata_fast/raw/main/por.traineddata" -OutFile $tmp
        if ((Get-Item $tmp).Length -lt 500000) { throw "arquivo de idioma incompleto" }
        try { Copy-Item $tmp $por -Force -ErrorAction Stop }
        catch {
            Write-Host "Copiando com permissao de administrador (clique em Sim)..."
            Start-Process powershell -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList @("-NoProfile", "-Command", "Copy-Item -LiteralPath '$tmp' -Destination '$por' -Force")
        }
        if (Test-Path $por) { Write-Host "Idioma portugues do OCR instalado." } else { throw "nao foi possivel copiar o idioma para $dados" }
    } catch {
        Write-Host "AVISO: OCR nao configurado ($($_.Exception.Message)). O app abre normalmente; imagens e PDFs digitalizados ficam indisponiveis ate instalar o Tesseract com o idioma Portuguese." -ForegroundColor Yellow
    }
}

try {
    Write-Host "Evidence Finder" -ForegroundColor Green
    Write-Host "Tudo roda no seu computador. Nada e enviado para a internet pelo aplicativo."
    Write-Host "(A internet so e usada agora, para baixar as ferramentas de instalacao.)"

    Atualizar-Path
    $exe = Join-Path $raiz "src-tauri\target\release\evidence-finder.exe"

    if ((Test-Path $exe) -and -not $Recompilar) {
        if (-not (Achar-Tesseract)) { Garantir-Tesseract }
        Titulo "Abrindo o Evidence Finder"
        Start-Process $exe
        return
    }

    Titulo "Verificando o que precisa ser instalado"
    $precisaWinget = (-not (Tem "node")) -or (-not (Tem "cargo")) -or (-not (Tem-BuildTools)) -or (-not (Tem-WebView2))
    if ($precisaWinget -and -not (Tem "winget")) {
        throw "O instalador do Windows (winget) nao foi encontrado. Abra a Microsoft Store, instale ou atualize o 'Instalador de Aplicativo' (App Installer) e execute este arquivo novamente."
    }

    if (-not (Tem "node")) {
        Titulo "Instalando Node.js"
        Winget-Instalar "OpenJS.NodeJS.LTS" @("--silent")
        Atualizar-Path
    } else { Write-Host "Node.js: ok" }

    if (-not (Tem-BuildTools)) {
        Titulo "Instalando Ferramentas de Build C++ (download grande, pode levar varios minutos)"
        Write-Host "Se o Windows pedir permissao de administrador, clique em Sim."
        Winget-Instalar "Microsoft.VisualStudio.2022.BuildTools" @("--silent", "--override",
            "--wait --quiet --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended")
        if (-not (Tem-BuildTools)) {
            throw "As Ferramentas de Build C++ nao ficaram prontas. Reinicie o computador e execute este arquivo novamente. Se persistir, envie o arquivo instalacao-log.txt para quem deu suporte."
        }
    } else { Write-Host "Ferramentas C++: ok" }

    if (-not (Tem "cargo")) {
        Titulo "Instalando Rust"
        Winget-Instalar "Rustlang.Rustup" @("--silent")
        Atualizar-Path
        if (Tem "rustup") { & rustup default stable }
    } else { Write-Host "Rust: ok" }

    if (-not (Tem-WebView2)) {
        Titulo "Instalando WebView2"
        Winget-Instalar "Microsoft.EdgeWebView2Runtime" @("--silent")
    } else { Write-Host "WebView2: ok" }

    Garantir-Tesseract

    Atualizar-Path
    foreach ($c in @("node", "npm", "cargo")) {
        if (-not (Tem $c)) {
            throw "'$c' ainda nao foi encontrado depois da instalacao. Feche esta janela, reinicie o computador e execute este arquivo novamente."
        }
    }

    Titulo "Baixando as bibliotecas do aplicativo"
    & npm.cmd install
    if ($LASTEXITCODE -ne 0) { throw "Falha ao baixar as bibliotecas (npm install). Verifique sua conexao com a internet." }

    Titulo "Compilando o aplicativo (a primeira vez leva de 10 a 30 minutos; nao feche a janela)"
    & npm.cmd run tauri -- build --no-bundle
    if ($LASTEXITCODE -ne 0) { throw "Falha ao compilar o aplicativo. Envie o arquivo instalacao-log.txt para quem deu suporte." }

    if (-not (Test-Path $exe)) { throw "A compilacao terminou, mas o programa nao foi encontrado em: $exe" }

    Titulo "Pronto! Abrindo o Evidence Finder"
    Write-Host "Da proxima vez, basta abrir INICIAR.bat de novo (abre em poucos segundos)."
    Start-Process $exe
}
catch {
    Write-Host ""
    Write-Host "ERRO: $($_.Exception.Message)" -ForegroundColor Red
    $global:falhou = $true
}
finally {
    try { Stop-Transcript | Out-Null } catch {}
}
if ($global:falhou) { exit 1 }
