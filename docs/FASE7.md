# Fase 7 — busca semântica local, evidências visuais e distribuição

Este documento descreve o **estado real** da fase: o que foi executado e com que resultado, o que
foi escrito mas ainda depende de um recurso que o ambiente de desenvolvimento não tinha (Windows,
modelo semântico, PowerShell) e o que continua ausente. Substitui a versão anterior, escrita antes
da primeira compilação.

## Como ler os estados

| Estado | Significado |
|---|---|
| **V — validado** | existe teste automatizado **executado** (resultado registrado abaixo) |
| **E — escrito, não validado** | o código existe e compila, mas a verificação real depende de algo ausente no ambiente |
| **A — ausente** | não implementado; o motivo está nas limitações |

Ambiente de verificação desta rodada: Linux (Ubuntu 24.04), 1 vCPU Intel Xeon 2,8 GHz, 4 GB de RAM,
Rust 1.91.1, Vitest, Tesseract 5.3.4 (+ `por.traineddata` *fast*), PDFium 157 para Linux.
**Nenhuma verificação foi feita em Windows.** O Hugging Face estava inacessível (rede restrita), então
o modelo semântico real **não foi executado**.

## Resultados efetivamente executados

| Verificação | Resultado |
|---|---|
| `npm run typecheck`, `npm run lint` | sem erros |
| `npm run test` (Vitest) | 82 testes, 8 arquivos, todos passam |
| `npm run build` | concluído |
| `cargo test` completo, **com a rede bloqueada** (`unshare -n … --offline`) e Tesseract em português disponível | **258 passam, 0 falham, 0 avisos**; 10 marcados `ignored` |
| `cargo test --lib --no-default-features` (sem o motor semântico), com o código final | **258 passam, 0 falham**; 6 `ignored` |
| Testes `ignored` executados à parte | Tesseract real (EXIF), PDFium real (3 de `pdf_visual` + 1 de exportação) e benchmark: passam |
| Testes `ignored` **não** executados | 4 testes do modelo real (`real_model_*`): modelo indisponível |
| `cargo build --release` | **não executado** (disco do ambiente insuficiente); só o perfil de teste foi compilado |
| `tauri build` / instalador NSIS | **não executado** |

## Falhas encontradas e corrigidas nesta rodada

1. **Nada da Fase 7 tinha sido compilado.** Na primeira compilação real (`candle`, `tokenizers`, `png`)
   não houve erros de API. Foi necessário Rust mais novo que o 1.85 instalado primeiro; tudo foi
   compilado e testado com 1.91.1. O `rust-version = "1.77.2"` do `Cargo.toml` **não foi verificado** e
   provavelmente está desatualizado.
2. **5 testes falhavam** na primeira execução; todos eram defeitos do teste, não do código:
   importações de conteúdo idêntico (corretamente recusadas como duplicata); um limiar `> 50 pt`
   inadequado a uma imagem pequena (a geometria do código estava certa); uma comparação de texto
   contínuo que ignorava cabeçalho/rodapé entre páginas; e o teste de rede, que proibia a URL de
   **exibição** do modelo (agora há uma exceção explícita e única).
3. **Bug real: fotos JPEG com orientação EXIF** (celulares). O Tesseract ignora a tag, então o OCR lia
   a foto deitada e devolvia lixo (reproduzido: `eiDUZISJsuUel…`), e o relatório mostraria a imagem de
   lado. Corrigido (ver "Orientação EXIF").
4. **Código morto** (`ModelManager::with_embedder`) removido.
5. **Requisito de empacotamento descoberto:** o aplicativo lê o OCR em TSV; sem `tessdata\configs\tsv`
   o Tesseract falha. O script de distribuição agora confere esse arquivo.
6. **Limitação do extrator descoberta (não corrigida):** o `lopdf` pode **colar linhas** quando o PDF as
   posiciona só por `Td` dentro de um único `BT…ET` ("quintaPagamento"), e a busca por "pagamento" não
   encontra essa palavra. É comportamento da biblioteca; precisa de decisão (outro extrator ou
   pós-processamento) e de PDFs reais para validar.

## Busca semântica

### Como funciona (inalterado; agora compilado e testado quanto à lógica)

- Cada trecho vira um vetor de 384 números gerado **dentro do aplicativo**, em CPU, pelo modelo
  `intfloat/multilingual-e5-small` via `candle` (Rust puro). A consulta vira um vetor do mesmo modo;
  a proximidade é o cosseno.
- **Híbrida:** fusão por posição (RRF, `k = 60`). Toda a lista lexical entra na fusão sem limiar:
  resultado com termo literal nunca é descartado por similaridade baixa.
- **Semântica pura:** só trechos com cosseno ≥ `E5_MIN_SCORE = 0,80`, valor **inicial, não calibrado**.
- A similaridade é um indicador técnico; não é probabilidade nem confirmação do fato.
- Resultado só semântico não tem destaque amarelo literal.

### O que foi validado e o que não foi

| Aspecto | Estado | Evidência |
|---|---|---|
| Compilação com o motor (`candle`, `tokenizers`) | V | `cargo test` compila e roda |
| Compilação sem o motor (`--no-default-features`) | V | 258 testes passam com o código final; busca lexical completa |
| Indexação, reaproveitamento por conteúdo, invalidação por versão do modelo, cancelamento, isolamento entre projetos, fusão híbrida, preservação de literais, filtros | V | `semantic_tests` (gerador **de teste** por conceitos) |
| Modelo ausente / incompleto / corrompido → erro claro, sem pânico | V | `embeddings::tests` |
| Nenhuma chamada de rede | V | teste estático + suíte com rede bloqueada |
| **Carregamento dos pesos e do tokenizador reais** | **E** | `real_model_vectors_are_sane` (ignored) |
| **Qualidade da busca por significado em português** (P@3, R@3, MRR) | **E** | `real_model_relevance_metrics_over_the_annotated_corpus` (ignored) |
| **Calibração de `E5_MIN_SCORE` e dos pesos do RRF** | **A** | só com o modelo real; `real_model_unrelated_query_scores_are_reported_for_calibration` imprime os escores necessários |

Para executar os testes do modelo real:

```powershell
scripts\instalar-modelo.ps1 -Destino C:\modelos\multilingual-e5-small -Sim
$env:EF_MODEL_DIR = "C:\modelos\multilingual-e5-small"
cargo test --manifest-path src-tauri/Cargo.toml real_model -- --ignored --nocapture
```

O corpus anotado (14 frases fictícias em português, 5 consultas) está em
`src-tauri/src/real_model_tests.rs`. Os testes **imprimem** as métricas e afirmam só relações básicas
(a paráfrase "Consigo te devolver o dinheiro no próximo mês" entre os 3 primeiros para "promessas de
pagamento de dívidas"; algum relevante no top-3 em cada consulta). **Esses testes nunca rodaram**: as
afirmações podem precisar de ajuste na primeira execução, e nenhum limiar deve ser fixado antes dela.

### Modelo e instalação

| | |
|---|---|
| Modelo | `intfloat/multilingual-e5-small` (multilíngue, inclui português) |
| Origem | https://huggingface.co/intfloat/multilingual-e5-small |
| Licença | MIT segundo o repositório do modelo (**confirme antes de distribuir**) |
| Tamanho | ~470 MB em disco; memória ao carregar **não medida** |
| Arquivos | `config.json`, `tokenizer.json`, `model.safetensors` |
| Onde o app procura | 1º `%APPDATA%\<identificador>\models\multilingual-e5-small` (se completo); 2º `<pasta do exe>\models\multilingual-e5-small` (modelo do instalador) |

Instalação para desenvolvimento: `INICIAR.bat`/`RECOMPILAR.bat` chamam `scripts/instalar-modelo.ps1`
(HTTPS, `.part`, conferência de tamanho e SHA-256 contra os metadados do Hugging Face). Instalador com o
modelo embutido: ver [`DISTRIBUICAO.md`](DISTRIBUICAO.md). O modelo empacotado é conferido contra
`MODEL-MANIFEST.json` **antes de carregar** (lógica testada); o script PowerShell que gera o manifesto
**não foi executado**.

### Índice semântico

Migração **aditiva v5** (`ocr_word`, `embedding_vector`, `segment_embedding`); projetos antigos abrem
normalmente. Vetores guardados por conteúdo (SHA-256 do texto preparado + versão do modelo + versão do
processo); texto idêntico reaproveita o vetor; remover/reprocessar um documento remove o vínculo; vetores
órfãos são coletados. Indexação em segundo plano, cancelável, com progresso; falha em um trecho não
interrompe os demais. Trechos acima de 2000 caracteres são indexados só pelo início. Cada projeto tem o
seu próprio banco.

## Evidências visuais

### O que recebe imagem

| Origem | Evidência visual | Estado |
|---|---|---|
| PNG / JPEG com OCR | Sim, pelas caixas das palavras do OCR | V |
| **JPEG com orientação EXIF 2–8** | Sim, imagem girada para a posição correta (a mesma lida pelo OCR) | **V** (novo) |
| Página de PDF digitalizado com **uma** imagem | Sim | V |
| Página de PDF digitalizado com **várias** imagens | **Não**, por decisão: aviso explícito | A (ver limitações) |
| **PDF com texto digital** | Sim, se o PDFium estiver instalado e o trecho for localizado com segurança | **V em Linux**; `pdfium.dll` do Windows **E** |
| PDF com texto digital sem PDFium | Não: texto + página + motivo no relatório | V |
| TXT / texto colado | Não se aplica | — |

### Orientação EXIF (novo)

`src-tauri/src/orientation.rs` decodifica o JPEG (`zune-jpeg`, Rust puro), aplica a transformação das 8
orientações e reencoda como PNG para o OCR. O relatório usa **os mesmos pixels**, então as coordenadas
ficam no mesmo espaço visual. O arquivo original nunca é alterado.

Validação: 8 JPEGs reais (um por orientação, gerados com Pillow e conferidos contra o `exif_transpose`
dele) saem com os quatro quadrantes coloridos na posição certa; tabela exata em grade 3×2; e um teste
com **Tesseract real em português** em uma foto EXIF 6 (controle negativo: sem a correção o OCR não lê a
frase; com ela lê "Pagamento realizado hoje", imagem 900×200).

`EXTRACTOR_VERSION` subiu para **4**; projetos antigos aparecem como "desatualizados". Proteção: um JPEG
com EXIF ≠ 1 cuja extração é anterior à v4 **não recebe destaque** (as coordenadas guardadas estão em
outro espaço; a orientação 3 mantém as dimensões e o destaque cairia no lugar errado), e o motivo manda
reprocessar o documento. Imagens dentro de PDFs não sofrem correção: leitores de PDF e OCR as tratam sem
EXIF, de forma consistente.

### PDF com texto digital (novo)

`src-tauri/src/pdf_visual.rs` usa o **PDFium** (biblioteca dinâmica local, carregada só de caminhos
absolutos: `<pasta do exe>\pdfium\pdfium.dll` ou `EVIDENCE_FINDER_PDFIUM`; nunca pelo PATH). Ele
renderiza a página (~150 dpi) e localiza o termo. Um destaque **só é desenhado** quando:

1. o trecho encontrado aparece **uma única vez** no texto da página;
2. o termo literal ocorre **o mesmo número de vezes** no texto indexado (lopdf) e no que o PDFium enxerga;
3. então a k-ésima ocorrência do aplicativo é a k-ésima do PDFium.

Qualquer dúvida vira um motivo explícito e só o texto é mantido. As coordenadas são convertidas pelo
próprio PDFium para o espaço do bitmap (`FPDF_PageToDevice`), o que trata página rotacionada e caixa de
recorte. Correspondências só semânticas (sem termo literal) não são localizadas nesses PDFs.

Validação com PDFium real (Linux): segunda ocorrência marcada na linha certa (22,1% do topo; esperado
22,2%) com tinta sob o destaque; página girada 90°; segmento ambíguo recusado; exportação completa
(importar → buscar → exportar) com imagem, um destaque dentro dela e o original intacto.

### Layout do relatório e manifesto (inalterados)

Capa com modalidade, modelo e índice das ocorrências; trechos com termos literais em amarelo atrás do
texto (selecionável); blocos de duas colunas (imagem à esquerda, descrição à direita); fonte exportada já
verificada por SHA-256; aviso quando não há imagem. Manifesto `schemaVersion` 1 com campos acrescentados
(`search.mode`, `search.semanticModel`, `matchKind`, `semanticScore`, `visualEvidence { included,
highlight, reason }`).

**Não verificado:** a legibilidade do PDF gerado em um leitor real (Adobe/Edge/SumatraPDF), em
particular a mistura *Multiply* do amarelo. Os testes analisam a estrutura do PDF (posições, imagens,
texto), não a renderização.

## Desempenho (500 arquivos)

Medido com `perf_project_with_up_to_500_files` (`src-tauri/src/perf_tests.rs`), reproduzível:

```text
TESSDATA_PREFIX=<tessdata com por e configs/tsv> \
  cargo test perf_ -- --ignored --nocapture --test-threads=1     # use --release para números realistas
```

Hardware: Linux, 1 vCPU Intel Xeon 2,8 GHz, 4 GB. **Perfil de teste (debug)**: o lado Rust é mais lento
que em `release`; trate como limite superior. O OCR é um processo externo e não depende do perfil.

| Conjunto | 250 TXT, 100 PDF textuais (3 págs), 100 PNG, 50 PDF digitalizados = **500 arquivos**, 6,7 MB |
|---|---|
| Importação | 1,59 s (3,2 ms/arquivo) |
| Extração total | 73,0 s, **0 falhas** |
| Por tipo (mediana / p95) | TXT 6 / 9 ms · PDF textual 16 / 23 ms · PNG (OCR) 0,45 / 0,57 s · PDF digitalizado (OCR) 0,44 / 0,53 s |
| Banco | 1100 trechos; pasta do projeto 2,1 MB |
| Pesquisa lexical (60 consultas) | mediana 62,8 ms · p95 71,4 ms · máx 96,7 ms |
| Reprocessar 25 documentos | trechos 1100 → 1100 (**sem duplicar**) |
| Comparação bruta de vetores | 4400 vetores × 384 dims em 39,3 ms (debug) |

Limites da medição: PNG e PDF digitalizados repetem o mesmo texto (fixture) com 1 pixel alterado; o OCR
usa `tessdata_fast`; **não foram medidos** o tempo de embeddings nem a memória do modelo (sem modelo); o
pico de memória do processo de teste (56 MB) **não inclui** o Tesseract; **não foi medida** a
responsividade da interface; não há medição em Windows. Nenhuma meta de desempenho é afirmada. A
comparação de vetores é força bruta e, a cerca de 10 µs por vetor em debug, não é gargalo na escala do
MVP; nenhuma otimização foi feita sem evidência de necessidade.

## Segurança e privacidade

| Item | Estado | Evidência |
|---|---|---|
| Rede de saída bloqueada | **V (Linux)** | suíte completa com `unshare -n … --offline` (258 testes, incluindo OCR real e exportação) |
| Rede bloqueada no Windows (firewall) | E | lista em `DISTRIBUICAO.md` |
| Nenhum cliente de rede/telemetria no código | V | testes estáticos de `security_tests` (única URL: a de exibição do modelo, por exceção explícita) |
| Permissões do Tauri | revisado | `capabilities/default.json` com `permissions: []`; único plugin é o de diálogo, usado só pelo Rust; CSP sem origens remotas |
| Validação de caminhos, gravações restritas, sem sobrescrita silenciosa, originais intactos | V | testes das Fases 1–6 mantidos; export test compara o PDF original byte a byte |
| Carregamento de DLL (PDFium) | novo, revisado | só caminhos absolutos controlados, nunca o PATH. `EVIDENCE_FINDER_PDFIUM` permite carregar uma biblioteca arbitrária a quem controla o ambiente do usuário (vale também para `EVIDENCE_FINDER_TESSERACT`, já existente); isso já exige acesso à conta do usuário |
| Integridade do modelo empacotado | V (lógica) | SHA-256 + tamanho contra o manifesto, antes de carregar; **não é assinatura** |
| `cargo audit` e licenças das novas dependências (`zune-jpeg`, `pdfium-render` e transitivas) | **A (pendente)** | `cargo audit` indisponível; revisão jurídica não feita |

### Criptografia do SQLite, dos caches e dos dados derivados — avaliação

**Não implementada.** O banco, os vetores (no mesmo SQLite), as cópias gerenciadas e as exportações
continuam em claro; a proteção recomendada é a criptografia de disco do sistema (BitLocker). Estratégia
compatível com projetos existentes, para uma fase própria:

1. **Opcional, por projeto**, nunca automática: nenhum projeto antigo é alterado sem pedido.
2. **SQLCipher** (via `rusqlite` com `bundled-sqlcipher*`) cobre o banco, o FTS5 e os vetores. No Windows
   exige OpenSSL empacotado e complica o build; avaliar em Windows real.
3. **Migração segura:** criar um banco novo cifrado (`sqlcipher_export`), conferir contagens e hashes,
   manter o original como backup até o usuário confirmar, trocar por renomeação atômica; se o processo for
   interrompido, o original permanece intacto.
4. **Chave:** protegida pelo DPAPI do usuário do Windows; perder a chave significa perder o projeto, o que
   precisa ser dito claramente (há o dilema entre recuperação e segurança).
5. **O que o SQLCipher não cobre:** cópias gerenciadas em `files/`, miniaturas/caches e relatórios
   exportados; cobri-los exige criptografia por arquivo ou depender do BitLocker.
6. Nenhuma criptografia improvisada deve ser introduzida.

O aplicativo **não** promete proteção absoluta, eliminação forense nem cadeia de custódia formal. O
SHA-256 identifica conteúdo idêntico e não prova autenticidade ou autoria.

## Requisitos do sistema

Windows 10 (1809+) ou 11, 64 bits (declarado, **não testado**). Espaço e memória: ver
[`DISTRIBUICAO.md`](DISTRIBUICAO.md) (modelo ~470 MB; memória do modelo e tamanho do instalador **não
medidos**). Desenvolvimento: Node.js ≥ 20, Rust estável recente (a árvore foi compilada com 1.91.1),
Ferramentas C++ e WebView2.

## Como verificar

```bash
npm ci
npm run check                                                           # tsc + eslint + vitest + cargo test
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features   # sem o motor semântico

# Com a rede bloqueada (Linux):
unshare -n cargo test --offline --manifest-path src-tauri/Cargo.toml

# Testes que dependem de recursos externos (todos `ignored` por padrão):
TESSDATA_PREFIX=<tessdata> cargo test orientation -- --ignored          # Tesseract + português
EVIDENCE_FINDER_PDFIUM=<libpdfium> cargo test real_pdfium -- --ignored  # PDFium
EF_MODEL_DIR=<modelo> cargo test real_model -- --ignored --nocapture    # modelo semântico
```

## Limitações que permanecem

- **Windows:** nada foi verificado em Windows real (instalador, scripts `.ps1`/`.bat`, `pdfium.dll`,
  diálogos, firewall).
- **Modelo real:** carregamento dos pesos, qualidade em português, calibração do limiar e dos pesos da
  fusão, tempo e memória dos embeddings: tudo pendente do modelo.
- **Instalador:** `preparar-distribuicao.ps1` e o instalador NSIS nunca foram executados; tamanho do
  instalador e consumo de memória não medidos.
- **Build de produção:** `cargo build --release` não foi executado nesta rodada.
- **PDF digitalizado com várias imagens por página:** sem evidência visual. Seria preciso conhecer a
  posição de cada imagem na página (matriz do conteúdo do PDF) para associar o OCR à região; sem isso,
  qualquer destaque seria aproximado. O relatório mantém o texto e explica o motivo.
- **PDF com texto digital:** só termos literais; texto que o lopdf e o PDFium leem de formas diferentes
  não recebe destaque; linhas coladas pelo `lopdf` (item 6 das falhas) prejudicam a busca.
- **Legibilidade do PDF gerado** (Multiply do amarelo) não conferida em leitor real.
- **Criptografia do armazenamento**, `cargo audit` e revisão de licenças: pendentes.
- Similaridade semântica indica possível relação de significado, não confirmação factual.

## Matriz de requisitos do roteiro de arquitetura

Estados: **V** validado por teste executado · **E** escrito, não validado · **A** ausente.

| # | Requisito (roteiro) | Estado | Evidência / observação |
|---|---|---|---|
| 1 | Importa imagens, TXT, texto copiado e PDFs | V | testes de importação; 500 arquivos em 1,59 s (debug) |
| 2 | Trata arquivos inválidos sem interromper o projeto | V | testes de lote com falhas; 0 falhas nos 500 |
| 3 | Preserva os arquivos originais | V | testes anteriores; export test compara o PDF original byte a byte |
| 4 | Registra falhas de OCR e extração | V | testes de extração (OCR ausente, corrompido, baixa confiança) |
| 5 | Encontra termos conhecidos em um conjunto de teste | V | testes de busca; consultas do benchmark |
| 6 | Apresenta a origem de cada resultado | V | testes de busca e exportação |
| 7 | Identifica páginas, linhas ou regiões | V | páginas de PDF, linhas de TXT, caixas do OCR; posição do destaque conferida |
| 8 | Gera PDFs legíveis | V (estrutura) / E (leitor real) | texto selecionável, imagens e destaques conferidos por análise do PDF; leitor real pendente |
| 9 | Cria pastas com os arquivos selecionados | V | export tests |
| 10 | Gera manifesto com referências às fontes | V | export tests; campos novos documentados |
| 11 | Funciona com a conexão externa bloqueada | V (Linux) / E (Windows) | suíte completa com `unshare -n`; firewall no Windows pendente |
| 12 | Reabre projetos existentes | V | testes de persistência e migração v1→v5 |
| 13 | Tolera falhas individuais de processamento | V | `catch_unwind` por documento; benchmark sem falhas |
| 14 | Processa até 500 arquivos sem travar a interface | E | backend medido com 500 arquivos; responsividade da interface não medida |
| 15 | Registra desempenho com configuração documentada | V (Linux, debug) | seção "Desempenho"; falta Windows e release |
| 16 | Busca lexical independente do modelo semântico | V | 258 testes sem o motor; modalidades degradam com aviso |
| 17 | Expressões exatas, combinações, filtros, ordenação | V | testes de busca |
| 18 | Busca semântica: lógica (indexação, reuso, invalidação, isolamento, cancelamento) | V | `semantic_tests` com gerador de teste |
| 19 | Busca semântica com o modelo real em português | E | `real_model_tests` escritos, nunca executados |
| 20 | Híbrida preserva resultados literais | V | `semantic_tests` |
| 21 | Calibração do limiar e dos pesos da fusão | A | exige o modelo real |
| 22 | Evidência visual: imagem à esquerda, destaque amarelo, descrição à direita | V | `pdf_report_tests`, `export_tests` |
| 23 | Orientação EXIF (imagem e coordenadas na mesma orientação) | V | 8 orientações reais + Tesseract real |
| 24 | PDF com texto digital: evidência visual com coordenadas confiáveis | V (Linux, PDFium real) / E (Windows) | `pdf_visual`, `export_tests` |
| 25 | PDF digitalizado com várias imagens por página | A (aviso seguro) | decisão documentada |
| 26 | Aviso explícito quando a evidência visual não pode ser gerada | V | motivos testados (sem PDFium, ambíguo, sem termo literal, EXIF antigo) |
| 27 | Integridade da fonte verificada antes da exportação | V | testes das Fases 5–7 |
| 28 | Instalador Windows com modelo e OCR, offline | E | script escrito, lógica Rust testada; **instalador nunca gerado** |
| 29 | Modelo no diretório que o app usa; integridade; sem novo download | V (lógica) / E (script) | `resolve_model_dir`, `verify_manifest` |
| 30 | Preservar a variante sem o motor semântico | V | 258 testes com `--no-default-features` |
| 31 | Espaço em disco e requisitos de memória informados | E | disco do modelo informado; memória e instalador **não medidos** |
| 32 | Testes de instalação em Windows | A | lista de verificações manuais em `DISTRIBUICAO.md` (todas pendentes) |
| 33 | Permissões do Tauri, caminhos, temporários e exclusão revisados | V | revisão + testes das fases 5–6 |
| 34 | Criptografia do SQLite/caches | A | avaliada; estratégia documentada; não implementada |
| 35 | Vulnerabilidades e licenças das dependências | A | `cargo audit` e revisão jurídica pendentes |
| 36 | Documentação reflete o estado real | V | README, este documento e `DISTRIBUICAO.md` revisados nesta rodada |
