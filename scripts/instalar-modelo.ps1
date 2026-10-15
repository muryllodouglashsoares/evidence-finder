# Instala o modelo de busca semantica do Evidence Finder (multilingual-e5-small).
#
# E chamado automaticamente por INICIAR.bat / RECOMPILAR.bat (via instalar-e-iniciar.ps1),
# mas tambem pode ser executado a mao. O APLICATIVO em si nunca baixa nada: quem baixa
# e este script, e somente quando o modelo ainda nao esta instalado.
#
# Seguranca: HTTPS apenas; o SHA-256 e o tamanho de cada arquivo sao conferidos contra os
# metadados publicados pelo Hugging Face; o arquivo so entra na pasta final depois de
# conferido (download em .part). Se a conferencia nao for possivel ou falhar, NADA e instalado.
#
# Uso:  .\scripts\instalar-modelo.ps1            (pergunta antes de baixar)
#       .\scripts\instalar-modelo.ps1 -Sim       (sem perguntas)
#       .\scripts\instalar-modelo.ps1 -Destino "<pasta>"  (pasta personalizada)
# Sem -Destino, usa a mesma pasta que o aplicativo le (%APPDATA%\<identificador>\models\...).
# Codigo de saida: 0 = modelo disponivel; 1 = falha (o app continua funcionando sem busca semantica).

param(
  [string]$Destino,
  [switch]$Sim
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"   # a barra do Invoke-RestMethod/WebRequest deixa tudo lento no PS 5.1

$repo = "intfloat/multilingual-e5-small"
$arquivos = @("config.json", "tokenizer.json", "model.safetensors")
$minimos = @{ "config.json" = 100; "tokenizer.json" = 1MB; "model.safetensors" = 100MB }   # sanidade sem rede

function Pasta-Padrao {
  $id = "dev.evidencefinder.app"
  try {
    $conf = Join-Path (Split-Path -Parent $PSScriptRoot) "src-tauri\tauri.conf.json"
    $lido = (Get-Content -Raw -Encoding UTF8 $conf | ConvertFrom-Json).identifier
    if ($lido) { $id = $lido }
  } catch {}
  return Join-Path $env:APPDATA "$id\models\multilingual-e5-small"
}

function Arquivo-Ok($pasta, $nome) {
  $p = Join-Path $pasta $nome
  return (Test-Path -LiteralPath $p -PathType Leaf) -and ((Get-Item -LiteralPath $p).Length -ge $minimos[$nome])
}

try {
  if (-not $Destino) { $Destino = Pasta-Padrao }
  $faltando = @($arquivos | Where-Object { -not (Arquivo-Ok $Destino $_) })
  if ($faltando.Count -eq 0) {
    Write-Host "Modelo de busca semantica: ok ($Destino)"
    exit 0
  }

  Write-Host ""
  Write-Host "Modelo: $repo (embeddings multilingues, 384 dimensoes, licenca MIT)"
  Write-Host "Origem: https://huggingface.co/$repo"
  Write-Host "Faltando: $($faltando -join ', ')  (cerca de 470 MB no total)"
  Write-Host "Destino: $Destino"
  if (-not $Sim) {
    $r = Read-Host "Baixar agora? Isto usa a internet (somente esta vez). Digite S para confirmar"
    if ($r -notin @("S", "s")) { Write-Host "Cancelado. Nada foi baixado."; exit 0 }
  }

  # Metadados oficiais (tamanho e SHA-256 dos arquivos LFS). Sem eles nao ha como conferir: aborta.
  $arvore = Invoke-RestMethod -UseBasicParsing -TimeoutSec 30 -Uri "https://huggingface.co/api/models/$repo/tree/main"
  $meta = @{}
  foreach ($item in $arvore) { $meta[$item.path] = $item }
  foreach ($nome in $faltando) {
    if (-not $meta.ContainsKey($nome)) { throw "O arquivo '$nome' nao consta nos metadados do modelo. Nada foi instalado." }
  }

  New-Item -ItemType Directory -Force -Path $Destino | Out-Null
  $curl = Get-Command curl.exe -ErrorAction SilentlyContinue   # vem com o Windows 10 1803+; retoma downloads

  foreach ($nome in $faltando) {
    $saida = Join-Path $Destino $nome
    $parte = "$saida.part"
    $url = "https://huggingface.co/$repo/resolve/main/$nome"
    $info = $meta[$nome]
    $tamanhoEsperado = if ($info.lfs) { [int64]$info.lfs.size } else { [int64]$info.size }
    $hashEsperado = if ($info.lfs) { [string]$info.lfs.oid } else { $null }

    Write-Host "Baixando $nome ..."
    if ($curl) {
      & $curl.Source --fail --location --proto "=https" --proto-redir "=https" --retry 3 --continue-at - --progress-bar --output $parte $url
      if ($LASTEXITCODE -ne 0) { throw "Falha ao baixar $nome (curl codigo $LASTEXITCODE). Verifique a internet e tente de novo; o download sera retomado." }
    } else {
      if (Test-Path -LiteralPath $parte) { Remove-Item -LiteralPath $parte -Force }
      Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $parte
    }

    $tam = (Get-Item -LiteralPath $parte).Length
    if ($tam -ne $tamanhoEsperado) {
      Remove-Item -LiteralPath $parte -Force
      throw "$nome veio com tamanho diferente do esperado ($tam x $tamanhoEsperado bytes). Arquivo descartado; tente de novo."
    }
    if ($hashEsperado) {
      $h = (Get-FileHash -LiteralPath $parte -Algorithm SHA256).Hash.ToLower()
      if ($h -ne $hashEsperado.ToLower()) {
        Remove-Item -LiteralPath $parte -Force
        throw "$nome falhou na conferencia SHA-256. Arquivo descartado e nada foi instalado."
      }
    }
    Move-Item -LiteralPath $parte -Destination $saida -Force
    Write-Host "  $nome conferido e instalado."
  }

  Write-Host ""
  Write-Host "SHA-256 dos arquivos instalados (guarde para conferencia):"
  foreach ($nome in $arquivos) {
    $h = Get-FileHash -LiteralPath (Join-Path $Destino $nome) -Algorithm SHA256
    Write-Host ("  {0}  {1}" -f $h.Hash.ToLower(), $nome)
  }
  Write-Host "Modelo instalado. No aplicativo, use 'Gerar indice semantico' em Busca semantica local."
  exit 0
}
catch {
  Write-Host "AVISO: modelo de busca semantica nao instalado ($($_.Exception.Message))" -ForegroundColor Yellow
  Write-Host "A busca exata funciona normalmente. Para tentar de novo, abra INICIAR.bat com internet." -ForegroundColor Yellow
  exit 1
}
