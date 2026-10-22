# Distribuição para Windows

> **Estado:** o script `scripts/preparar-distribuicao.ps1` e a lógica Rust que o acompanha
> (resolução do modelo empacotado, verificação SHA-256, localização do Tesseract e do PDFium)
> foram escritos e a parte Rust foi testada. **O script PowerShell e o instalador NSIS nunca foram
> executados**: o ambiente de desenvolvimento desta rodada era Linux, sem PowerShell. Use a lista
> da seção "Verificações manuais pendentes" antes de distribuir qualquer instalador.

## Variantes

| Variante | O que inclui | Funciona offline logo após instalar? | Como gerar |
|---|---|---|---|
| **Completa** | motor semântico + modelo `multilingual-e5-small` + Tesseract (`por`, `eng`) + PDFium (se informado) + WebView2 offline | Sim (busca exata, semântica, OCR e evidência visual de PDFs) | `-Completo -TesseractDir ... [-PdfiumDir ...]` |
| **Enxuta** | motor semântico + Tesseract + PDFium (se informado), **sem** o modelo | Busca exata e OCR sim. A semântica exige instalar o modelo uma vez, com internet (`scripts\instalar-modelo.ps1`) | `-TesseractDir ... [-PdfiumDir ...]` |
| **Sem motor** | tudo da enxuta, compilado com `--no-default-features` (sem busca semântica) | Busca exata e OCR sim; busca semântica indisponível por construção | `-SemMotorSemantico -TesseractDir ...` |

O aplicativo **nunca baixa nada**. Quem baixa o modelo é o script, no computador de quem gera o
instalador (variante completa) ou, na enxuta, o `instalar-modelo.ps1` executado pelo usuário.

## Gerar o instalador

Pré-requisitos no computador de build: Node.js, Rust (MSVC), dependências (`npm ci`) e um Tesseract
instalado (por exemplo `UB-Mannheim.TesseractOCR`) contendo `tesseract.exe`, as DLLs,
`tessdata\por.traineddata` **e `tessdata\configs\tsv`** (o aplicativo lê o resultado do OCR em TSV;
sem esse arquivo o Tesseract falha). O script confere esses itens e aborta se faltarem.

PDFium (opcional, para evidência visual de PDFs com texto digital): baixe `pdfium-win-x64.tgz` de
<https://github.com/bblanchon/pdfium-binaries/releases>, extraia e passe a pasta em `-PdfiumDir`
(contém `bin\pdfium.dll` e `LICENSE`).

```powershell
.\scripts\preparar-distribuicao.ps1 -TesseractDir "C:\Program Files\Tesseract-OCR" -PdfiumDir C:\pdfium -Completo
```

Saída: `dist\EvidenceFinder-<versão>-<variante>-setup.exe` e o `.sha256`. `-SoPreparar` apenas monta a
pasta `dist-staging\` para inspeção, sem compilar.

## Gerar o instalador pelo GitHub (sem instalar nada no seu computador)

O workflow `.github/workflows/build-windows.yml` roda `scripts/preparar-distribuicao.ps1` em uma máquina
Windows do GitHub Actions, que já instala o Tesseract (com `por.traineddata`) e baixa o PDFium. Duas formas
de usar:

- **Release automática:** `git tag v0.1.0` e `git push origin v0.1.0`. O `.exe` e o `.sha256` aparecem em
  **Releases**, prontos para baixar.
- **Manual:** aba **Actions** > **Gerar instalador Windows (.exe)** > **Run workflow**. Escolha a variante
  (`completo`, `enxuto` ou `sem-motor`). Preenchendo `release_tag`, também publica uma Release; em branco, o
  instalador fica como artefato da execução (baixado em um `.zip`, exige login no GitHub).

A variante **completa** baixa o modelo (~470 MB) durante a execução e leva mais tempo. O PDFium vem da última
versão publicada em `bblanchon/pdfium-binaries`; se a compilação quebrar após uma atualização dele, fixe uma
versão específica no workflow. O instalador gerado continua **sem assinatura** e **sem teste em Windows real**
(ver a lista de verificações pendentes abaixo).

## O que o instalador coloca no computador

Na pasta de instalação (ao lado do executável):

```
Evidence Finder.exe
tesseract\tesseract.exe, *.dll
tesseract\tessdata\por.traineddata, eng.traineddata, configs\tsv
pdfium\pdfium.dll                       (se incluído)
models\multilingual-e5-small\           (somente na variante completa)
    config.json, tokenizer.json, model.safetensors, MODEL-MANIFEST.json
licencas\AVISOS-DE-TERCEIROS.txt e licenças dos componentes
```

- **Tesseract:** o aplicativo procura `<pasta do exe>\tesseract\tesseract.exe` (e `tessdata` ao lado dele);
  nenhum caminho da máquina de desenvolvimento é usado. Sem ele, importar e buscar texto continua
  funcionando; imagens e PDFs digitalizados informam que o OCR está indisponível.
- **Modelo:** o aplicativo prefere a cópia do usuário em `%APPDATA%\<identificador>\models\...` se estiver
  completa (nada é duplicado nem baixado); senão usa a da instalação. Antes de carregar a cópia da
  instalação, os três arquivos são conferidos (tamanho + SHA-256) contra `MODEL-MANIFEST.json`; qualquer
  divergência recusa o carregamento com a mensagem "reinstale o aplicativo". O manifesto detecta
  corrupção e alteração acidental; **não é uma assinatura digital** (quem consegue alterar o modelo
  também consegue alterar o manifesto).
- **PDFium:** carregado apenas de caminhos controlados (`<pasta do exe>\pdfium\pdfium.dll` ou a variável
  `EVIDENCE_FINDER_PDFIUM`, útil em desenvolvimento), nunca pelo PATH.

## Requisitos e espaço em disco

| Item | Valor | Situação |
|---|---|---|
| Sistema | Windows 10 (1809+) ou 11, 64 bits | declarado; **não testado** |
| Modelo semântico | ~470 MB em disco (3 arquivos) | tamanho publicado pelo repositório do modelo |
| Instalador completo | **não medido** (modelo + WebView2 offline + Tesseract + app) | medir após gerar |
| Memória ao carregar o modelo | **não medida** neste ambiente (estimativa da ordem de 0,5–1 GB) | medir em Windows real |
| Dados dos projetos | pasta escolhida pelo usuário; cópias gerenciadas usam disco extra | ver README |

Não há número medido de instalador nem de memória do modelo nesta rodada: o ambiente não tinha acesso
ao Hugging Face. Preencha esta tabela após a primeira geração real.

## Licenças

| Componente | Licença | Observação |
|---|---|---|
| Tesseract e `tessdata` | Apache-2.0 | o instalador copia os arquivos de licença que existirem |
| multilingual-e5-small | MIT (conforme o repositório do modelo) | **confirme no repositório antes de distribuir**; citação em `AVISOS-DE-TERCEIROS.txt` |
| PDFium | BSD-3-Clause / Apache-2.0 | licenças copiadas de `-PdfiumDir` |
| Dependências Rust/JS | ver `Cargo.lock` e `package-lock.json` | auditoria de licenças e vulnerabilidades **pendente** (ver `docs/FASE7.md`) |

## Desinstalação e dados do usuário

Os projetos ficam na pasta que o usuário escolheu e **não são tocados** pelo instalador nem pelo
desinstalador. Em `%APPDATA%\<identificador>` ficam a lista de projetos recentes e, se instalado pelo
script, o modelo. O desinstalador padrão do Tauri/NSIS oferece remover esses dados de aplicativo; o
comportamento exato precisa ser conferido no teste de reinstalação abaixo.

## Verificações manuais pendentes (Windows real)

Marque cada item ao executá-lo; nenhum foi executado até agora.

- [ ] Instalação limpa da variante **completa** em um Windows 10/11 sem WebView2, Node ou Rust.
- [ ] Primeira inicialização: o painel de busca semântica mostra o modelo como instalado (sem baixar nada).
- [ ] Criar projeto, fechar o app, reabrir e abrir o projeto de novo.
- [ ] Importar TXT, PNG, JPEG (incluindo foto de celular rotacionada), PDF textual e PDF digitalizado; processar.
- [ ] Busca exata, semântica e híbrida; abrir a fonte original.
- [ ] Exportar: relatório com evidência visual (imagem, PDF digital se o PDFium foi incluído), pasta e manifesto.
- [ ] Repetir o fluxo com o adaptador de rede desativado / regra de firewall bloqueando a saída do app.
- [ ] Variante **enxuta** sem o modelo: busca exata funciona; a semântica informa que falta o modelo.
- [ ] Variante **sem motor**: a interface informa que a busca semântica não existe nesta versão.
- [ ] Remover `tesseract\` da instalação: o app informa a ausência do OCR sem travar.
- [ ] Alterar um byte de `model.safetensors` da instalação: o app recusa com mensagem clara.
- [ ] Desinstalar e reinstalar: projetos intactos; conferir o que ocorre com `%APPDATA%`.
- [ ] Antivírus/SmartScreen: registrar avisos (o instalador não é assinado).
