# Evidence Finder - prepara e gera o INSTALADOR do Windows (NSIS).
#
# Variantes:
#   COMPLETO  : motor semantico + modelo multilingual-e5-small + Tesseract (OCR) incluidos.
#               Funciona offline logo apos instalar. Instalador grande (centenas de MB).
#   ENXUTO    : motor semantico + Tesseract, SEM o modelo. A busca exata funciona de imediato;
#               a semantica pede o modelo (scripts\instalar-modelo.ps1, uma unica vez, com internet).
#   SEM MOTOR : compilado com --no-default-features (sem busca semantica). Menor e mais simples.
#
# Uso (PowerShell, na raiz do projeto):
#   .\scripts\preparar-distribuicao.ps1 -TesseractDir "C:\Program Files\Tesseract-OCR" -Completo
#   .\scripts\preparar-distribuicao.ps1 -TesseractDir "C:\Program Files\Tesseract-OCR"
#   .\scripts\preparar-distribuicao.ps1 -TesseractDir "C:\Program Files\Tesseract-OCR" -SemMotorSemantico
#   .\scripts\preparar-distribuicao.ps1 -TesseractDir "..." -Completo -SoPreparar   (so monta dist-staging)
#
# Requer: Node.js, Rust (MSVC) e dependencias ja instaladas (npm ci). O download do modelo, quando
# -Completo, acontece AQUI, no computador de quem gera o instalador - nunca no do usuario final.
# O Tesseract precisa conter tesseract.exe, as DLLs, tessdata\por.traineddata e tessdata\configs\tsv.
# PDFium (opcional): renderiza paginas de PDF com texto digital para a evidencia visual. Baixe
# pdfium-win-x64.tgz de https://github.com/bblanchon/pdfium-binaries/releases, extraia, e informe a
# pasta em -PdfiumDir (deve conter bin\pdfium.dll e LICENSE). Sem ele, esses PDFs saem so com texto.

param(
  [string]$TesseractDir,
  [string]$PdfiumDir,
  [switch]$Completo,
  [switch]$SemMotorSemantico,
  [switch]$SoPreparar
)

$ErrorActionPreference = "Stop"
$raiz = Split-Path -Parent $PSScriptRoot
Set-Location $raiz

function Falha($msg) { Write-Host "ERRO: $msg" -ForegroundColor Red; exit 1 }
function Titulo($t) { Write-Host ""; Write-Host "=== $t ===" -ForegroundColor Cyan }
function Tamanho-Pasta($p) {
  $s = (Get-ChildItem -LiteralPath $p -Recurse -File -ErrorAction SilentlyContinue | Measure-Object -Property Length -Sum).Sum
  if (-not $s) { return 0 }
  return [int64]$s
}
function MB($bytes) { return ("{0:N0} MB" -f ($bytes / 1MB)) }

if ($Completo -and $SemMotorSemantico) { Falha "-Completo inclui o modelo semantico e nao combina com -SemMotorSemantico." }
if (-not $TesseractDir) { Falha "Informe -TesseractDir (pasta do Tesseract instalado, com tesseract.exe e tessdata)." }

$conf = Get-Content -Raw -Encoding UTF8 (Join-Path $raiz "src-tauri\tauri.conf.json") | ConvertFrom-Json
$versao = $conf.version
$variante = if ($SemMotorSemantico) { "sem-motor" } elseif ($Completo) { "completo" } else { "enxuto" }
$staging = Join-Path $raiz "dist-staging"

# ---------------------------------------------------------------- 1. Tesseract
Titulo "Conferindo o Tesseract em $TesseractDir"
$exe = Join-Path $TesseractDir "tesseract.exe"
$tessdata = Join-Path $TesseractDir "tessdata"
if (-not (Test-Path -LiteralPath $exe)) { Falha "tesseract.exe nao encontrado em $TesseractDir." }
foreach ($obrigatorio in @("por.traineddata", "configs\tsv")) {
  # configs\tsv e essencial: o aplicativo le o resultado do OCR no formato TSV.
  if (-not (Test-Path -LiteralPath (Join-Path $tessdata $obrigatorio))) { Falha "Falta tessdata\$obrigatorio no Tesseract informado." }
}
$versaoTess = (& $exe --version 2>&1 | Select-Object -First 1)
Write-Host "Tesseract: $versaoTess"

# ---------------------------------------------------------------- 2. Staging limpo
Titulo "Montando $staging"
if (Test-Path -LiteralPath $staging) { Remove-Item -LiteralPath $staging -Recurse -Force }
New-Item -ItemType Directory -Force -Path (Join-Path $staging "tesseract\tessdata\configs") | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $staging "licencas") | Out-Null
# Esta pasta precisa existir (mesmo vazia) para o mapeamento de recursos do Tauri.
New-Item -ItemType Directory -Force -Path (Join-Path $staging "models") | Out-Null
Set-Content -LiteralPath (Join-Path $staging "models\LEIA-ME.txt") -Encoding ASCII -Value "Esta pasta recebe o modelo de busca semantica (multilingual-e5-small), quando incluido na variante completa."

Copy-Item -LiteralPath $exe -Destination (Join-Path $staging "tesseract")
Get-ChildItem -LiteralPath $TesseractDir -Filter "*.dll" -File | Copy-Item -Destination (Join-Path $staging "tesseract")
foreach ($idioma in @("por.traineddata", "eng.traineddata")) {   # por: obrigatorio; eng: opcional (melhora palavras em ingles)
  $p = Join-Path $tessdata $idioma
  if (Test-Path -LiteralPath $p) { Copy-Item -LiteralPath $p -Destination (Join-Path $staging "tesseract\tessdata") }
}
Copy-Item -LiteralPath (Join-Path $tessdata "configs\tsv") -Destination (Join-Path $staging "tesseract\tessdata\configs")
Get-ChildItem -LiteralPath $TesseractDir -File | Where-Object { $_.Name -match '^(LICENSE|NOTICE|COPYING)' } |
  Copy-Item -Destination (Join-Path $staging "licencas")

# ---------------------------------------------------------------- 2b. PDFium (opcional)
New-Item -ItemType Directory -Force -Path (Join-Path $staging "pdfium") | Out-Null
if ($PdfiumDir) {
  $dll = Join-Path $PdfiumDir "bin\pdfium.dll"
  if (-not (Test-Path -LiteralPath $dll)) { Falha "bin\pdfium.dll nao encontrado em $PdfiumDir." }
  Copy-Item -LiteralPath $dll -Destination (Join-Path $staging "pdfium")
  foreach ($lic in @("LICENSE")) {
    $p = Join-Path $PdfiumDir $lic
    if (Test-Path -LiteralPath $p) { Copy-Item -LiteralPath $p -Destination (Join-Path $staging "licencas\PDFium-$lic.txt") }
  }
  if (Test-Path -LiteralPath (Join-Path $PdfiumDir "licenses")) {
    Copy-Item -LiteralPath (Join-Path $PdfiumDir "licenses") -Destination (Join-Path $staging "licencas\pdfium-licenses") -Recurse
  }
  $hashPdfium = (Get-FileHash -LiteralPath $dll -Algorithm SHA256).Hash.ToLower()
  Write-Host "PDFium incluido. SHA-256 de pdfium.dll: $hashPdfium (guarde para conferencia)"
} else {
  Set-Content -LiteralPath (Join-Path $staging "pdfium\LEIA-ME.txt") -Encoding ASCII -Value "PDFium nao incluido nesta variante: PDFs com texto digital nao geram evidencia visual (so texto)."
  Write-Host "AVISO: sem -PdfiumDir; PDFs com texto digital sairao sem evidencia visual." -ForegroundColor Yellow
}

# ---------------------------------------------------------------- 3. Modelo (somente -Completo)
$modeloDir = Join-Path $staging "models\multilingual-e5-small"
if ($Completo) {
  Titulo "Baixando e conferindo o modelo (somente neste computador, para empacotar)"
  & (Join-Path $PSScriptRoot "instalar-modelo.ps1") -Destino $modeloDir -Sim
  if ($LASTEXITCODE -ne 0) { Falha "O modelo nao foi obtido/conferido. Nada foi empacotado." }

  # Manifesto lido pelo aplicativo (embeddings::verify_manifest): SHA-256 e tamanho de cada arquivo.
  $arquivos = [ordered]@{}
  foreach ($nome in @("config.json", "tokenizer.json", "model.safetensors")) {
    $caminho = Join-Path $modeloDir $nome
    $arquivos[$nome] = [ordered]@{
      sha256 = (Get-FileHash -LiteralPath $caminho -Algorithm SHA256).Hash.ToLower()
      size   = (Get-Item -LiteralPath $caminho).Length
    }
  }
  $json = [ordered]@{ files = $arquivos } | ConvertTo-Json -Depth 4
  [System.IO.File]::WriteAllText((Join-Path $modeloDir "MODEL-MANIFEST.json"), $json, (New-Object System.Text.UTF8Encoding($false)))
}

# ---------------------------------------------------------------- 4. Avisos de licenca
$avisos = @"
Evidence Finder - componentes de terceiros incluidos neste instalador (variante: $variante)

Tesseract OCR e dados de idioma (tessdata: por, eng)
  Licenca: Apache License 2.0. https://github.com/tesseract-ocr/tesseract
  Distribuido junto com as DLLs que o acompanham; veja os arquivos de licenca desta pasta, quando houver.

"@
if ($Completo) {
  $avisos += @"
Modelo de embeddings multilingual-e5-small (intfloat)
  Licenca: MIT. https://huggingface.co/intfloat/multilingual-e5-small
  Citacao: Wang et al., 2024, "Multilingual E5 Text Embeddings: A Technical Report", arXiv:2402.05672.

"@
}
if ($PdfiumDir) {
  $avisos += @"
PDFium (renderizador de paginas de PDF)
  Licenca: BSD-3-Clause / Apache-2.0 (veja licencas\PDFium-LICENSE.txt e a pasta pdfium-licenses). Binarios: https://github.com/bblanchon/pdfium-binaries

"@
}
$avisos += "As dependencias Rust e JavaScript do aplicativo tem licencas proprias (veja Cargo.lock e package-lock.json do codigo-fonte)."
Set-Content -LiteralPath (Join-Path $staging "licencas\AVISOS-DE-TERCEIROS.txt") -Value $avisos -Encoding UTF8

# ---------------------------------------------------------------- 5. Resumo de tamanho
Titulo "Conteudo a ser incluido"
Write-Host ("Tesseract + idiomas : " + (MB (Tamanho-Pasta (Join-Path $staging "tesseract"))))
Write-Host ("PDFium             : " + (MB (Tamanho-Pasta (Join-Path $staging "pdfium"))))
if ($Completo) { Write-Host ("Modelo semantico    : " + (MB (Tamanho-Pasta $modeloDir))) }
if ($SoPreparar) { Write-Host "-SoPreparar: dist-staging montado. Nada foi compilado."; exit 0 }

# ---------------------------------------------------------------- 6. Configuracao de recursos e build
Titulo "Compilando o instalador ($variante)"
$overlay = Join-Path $raiz "src-tauri\tauri.distribuicao.conf.json"
$recursos = [ordered]@{
  "../dist-staging/tesseract" = "tesseract"
  "../dist-staging/pdfium"    = "pdfium"
  "../dist-staging/models"    = "models"
  "../dist-staging/licencas"  = "licencas"
}
$bundle = [ordered]@{ resources = $recursos }
if ($Completo) {
  # Windows 10 sem WebView2: o instalador padrao o BAIXARIA da internet na instalacao. Na variante
  # completa (offline) o runtime vai dentro do instalador (aumenta o tamanho em ~130 MB).
  $bundle["windows"] = [ordered]@{ webviewInstallMode = [ordered]@{ type = "offlineInstaller" } }
}
$cfg = [ordered]@{ bundle = $bundle }
[System.IO.File]::WriteAllText($overlay, ($cfg | ConvertTo-Json -Depth 5), (New-Object System.Text.UTF8Encoding($false)))

try {
  if ($SemMotorSemantico) {
    & npx tauri build --config $overlay -- --no-default-features
  } else {
    & npx tauri build --config $overlay
  }
  if ($LASTEXITCODE -ne 0) { Falha "A compilacao do instalador falhou." }
}
finally {
  Remove-Item -LiteralPath $overlay -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------- 7. Saida
$gerado = Get-ChildItem -Path (Join-Path $raiz "src-tauri\target\release\bundle\nsis") -Filter "*.exe" -ErrorAction SilentlyContinue |
  Sort-Object LastWriteTime -Descending | Select-Object -First 1
if (-not $gerado) { Falha "Instalador nao encontrado em src-tauri\target\release\bundle\nsis." }
$saidaDir = Join-Path $raiz "dist"
New-Item -ItemType Directory -Force -Path $saidaDir | Out-Null
$destino = Join-Path $saidaDir ("EvidenceFinder-$versao-$variante-setup.exe")
Copy-Item -LiteralPath $gerado.FullName -Destination $destino -Force
$hash = (Get-FileHash -LiteralPath $destino -Algorithm SHA256).Hash.ToLower()
Set-Content -LiteralPath "$destino.sha256" -Value "$hash  $(Split-Path -Leaf $destino)" -Encoding ASCII

Titulo "Pronto"
Write-Host "Instalador : $destino"
Write-Host ("Tamanho    : " + (MB (Get-Item -LiteralPath $destino).Length))
Write-Host "SHA-256    : $hash"
Write-Host "Teste o instalador em um Windows limpo com a lista de docs\DISTRIBUICAO.md antes de distribuir."
