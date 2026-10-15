# Fase 7 — busca semântica local e evidências visuais em PDF

> **Estado de verificação (leia primeiro).** Esta fase foi escrita sem acesso a
> `cargo`, `npm` nem à rede. **Nada foi compilado nem executado ainda**: nem os
> testes Rust, nem `npm run check`, nem o instalador. O código foi revisado à mão,
> mas é provável que a primeira compilação aponte erros de tipo/API (principalmente
> nas crates `candle`/`tokenizers`/`png`, cujas versões não puderam ser conferidas).
> Veja "Como verificar" abaixo e não considere a fase concluída antes disso.

## O que a fase entrega

1. **Busca semântica local** com embeddings, nas modalidades *exata*, *por
   significado* e *híbrida*, integrada à busca por palavras (FTS5) existente.
2. **Evidências visuais no PDF**: para trechos achados em imagens/PDFs
   digitalizados, o relatório inclui a imagem original com a região encontrada em
   amarelo à esquerda e a descrição à direita.
3. **Trechos textuais com destaque amarelo** dos termos literais.

## Busca semântica

### Como funciona
- Cada trecho extraído (com extração concluída e hash conferido com o documento)
  vira um vetor de 384 números, gerado **dentro do aplicativo**, em CPU, pelo
  modelo `intfloat/multilingual-e5-small` via `candle` (Rust puro). A consulta vira
  um vetor do mesmo modo; a proximidade é o cosseno.
- **Híbrida**: fusão por posição (RRF, `k = 60`, pesos 1,0/1,0, em
  `semantic::HybridConfig`). Escalas de BM25 e cosseno não são somadas. Toda a
  lista lexical entra na fusão sem limiar: **resultado com termo literal nunca é
  descartado por similaridade baixa**.
- **Semântica pura**: só trechos com cosseno ≥ limiar do modelo (`E5_MIN_SCORE =
  0,80`, valor **inicial** a calibrar com documentos reais).
- A similaridade é um **indicador técnico**; a interface e o relatório dizem que
  não é probabilidade nem confirmação do fato.
- Resultado só semântico **não tem destaque amarelo literal**.

### Modelo (instalado pelos scripts `.bat`; o aplicativo nunca baixa nada)
| | |
|---|---|
| Modelo | `intfloat/multilingual-e5-small` (multilíngue, inclui português) |
| Origem | https://huggingface.co/intfloat/multilingual-e5-small |
| Licença | MIT segundo o repositório do modelo (confirme antes de distribuir) |
| Tamanho | ~470 MB em disco; uso de RAM da ordem de 0,5–1 GB ao carregar (a medir) |
| Arquivos | `config.json`, `tokenizer.json`, `model.safetensors` |
| Pasta | `<dados do aplicativo>\models\multilingual-e5-small` (o caminho exato aparece no painel) |

Instalação: **automática** em `INICIAR.bat` e `RECOMPILAR.bat`. Eles chamam
`scripts/instalar-modelo.ps1 -Sim`, que baixa os 3 arquivos para a mesma pasta que o app lê
(`%APPDATA%\<identifier do tauri.conf.json>\models\multilingual-e5-small`) só se faltarem; se já
estiverem lá, a verificação é local e sem internet. Cada arquivo é baixado por HTTPS para `.part`,
conferido por tamanho e SHA-256 contra os metadados oficiais do Hugging Face e só então movido; se a
conferência falhar, nada é instalado e o app abre só com busca exata. O script também roda sozinho
(`-Destino` para outra pasta, sem `-Sim` pede confirmação) e a cópia manual pelo navegador continua
válida. **Não testado em Windows real nem contra a rede.** Para o instalador do Windows, os
arquivos podem ser incluídos no pacote (plano da seção 3.5 do documento de
arquitetura).

Sem o modelo: o painel mostra o estado real, a busca exata continua completa, a
híbrida cai (com aviso) para a exata e a semântica pura informa indisponibilidade.
`cargo build --no-default-features` gera um executável sem o motor.

### Índice semântico
- Migração **aditiva v5** (nenhuma tabela existente é alterada): `ocr_word`,
  `embedding_vector`, `segment_embedding`. Projetos antigos abrem normalmente.
- Vetores são guardados por conteúdo (SHA-256 do texto preparado + versão do
  modelo + versão do processo). Texto idêntico reaproveita o vetor; **versão do
  modelo** = hash de `config.json` + `tokenizer.json` + tamanho dos pesos.
- Remover ou reprocessar um documento apaga o vínculo (cascata); vetores órfãos e
  de versões antigas são coletados ao fim da indexação.
- Indexação em segundo plano, cancelável, com progresso; falha em um trecho não
  interrompe os demais (`catch_unwind`); reexecutar completa o que faltou.
- Trechos acima de 2000 caracteres são indexados só pelo início (a busca exata
  cobre o trecho inteiro).
- Cada projeto tem seu próprio banco: índices e vetores nunca se misturam.

### Interface
Painel "Busca semântica local" (estado do modelo, instruções, cobertura, gerar /
cancelar / apagar índice), seletor de modalidade, selo do tipo de correspondência
(Literal, Semântica, Literal + semântica) e a similaridade como indicador. Filtros,
ordenação e paginação valem nas três modalidades.

## Evidências visuais

### Coordenadas do OCR
O Tesseract já devolvia caixas por palavra, mas só a do parágrafo era guardada. A
v5 guarda cada palavra (`ocr_word`: posição em caracteres no trecho, linha, caixa,
confiança). O destaque usa **apenas essas coordenadas**; nada é deduzido do texto.
`EXTRACTOR_VERSION` subiu para 2: projetos antigos aparecem como "desatualizados"
e precisam ser **reprocessados** para ter evidência visual (nada é apagado
automaticamente). Sem coordenadas, o relatório não inventa imagem e explica o
motivo.

### O que recebe imagem
| Origem | Evidência visual |
|---|---|
| PNG / JPEG (com OCR) | Sim |
| Página de PDF digitalizado com **uma** imagem | Sim (a imagem da página é a usada pelo OCR) |
| Página de PDF digitalizado com várias imagens | Não (coordenadas ambíguas) |
| PDF com texto digital | **Não**: o aplicativo não tem renderizador de páginas |
| TXT / texto colado | Não se aplica |

### Como o destaque é feito
- Literal: as palavras do OCR que cobrem os trechos encontrados, unidas por linha.
- Só semântico: o **parágrafo reconhecido** que é o próprio resultado (a ligação é
  exata); o relatório rotula como "sem termo literal".
- O amarelo é um retângulo vetorial desenhado **sobre** a imagem com mistura
  *Multiply* (texto escuro continua visível) e borda. A imagem original não é
  alterada: JPEG entra com os bytes originais; PNG é decodificado (transparência
  sobre branco; reduzido a 3000 px no maior lado, com aviso).
- A imagem vem da **cópia exportada já verificada por SHA-256**. Fonte ausente,
  alterada ou ilegível → sem evidência visual, com aviso.
- Validação: coordenadas fora da imagem, caixas vazias ou dimensões diferentes das
  registradas pelo OCR desativam o destaque (com motivo). Orientação EXIF ≠ 1 gera
  aviso: o relatório usa a orientação dos pixels (a mesma do OCR).

### Layout do PDF
Capa com modalidade, modelo (se houve) e **índice das ocorrências**; seção de
trechos com termos literais em amarelo atrás do texto (texto continua
selecionável); seção "Evidências visuais" com bloco de duas colunas — imagem e
legenda à esquerda, descrição à direita (fonte, localização, consulta, modalidade,
tipo de correspondência, destaque, confiança do OCR, arquivo exportado, trecho
literal). Ocorrências do mesmo documento/página compartilham a imagem, com
etiquetas numeradas. Trechos muito longos são truncados no bloco (o texto completo
está na seção de trechos) e o bloco não é dividido entre páginas. Seção final de
avisos e limitações.

### Manifesto
Formato mantido (`schemaVersion` 1), com campos **acrescentados**: `search.mode`,
`search.semanticModel`, e por ocorrência `matchKind`, `semanticScore`,
`visualEvidence { included, highlight, reason }`.

## Como verificar (pendente)
```powershell
npm install
npm run check                 # tsc + eslint + vitest
cd src-tauri
cargo test                    # inclui semantic_tests, visual, pdf_report_tests, export_tests
cargo build --release         # ou: npm run tauri build
cargo build --no-default-features   # sem o motor semântico
```
O `Cargo.lock` precisa ser atualizado na primeira compilação (novas crates:
`candle-core/nn/transformers`, `tokenizers`, `png`); revise as licenças (todas
Apache-2.0/MIT, a confirmar).

Verificações manuais ainda necessárias: instalar o modelo, gerar o índice em um
projeto real, medir RAM/tempo (até 500 arquivos), calibrar `E5_MIN_SCORE` com
consultas reais e abrir o PDF gerado em um leitor (Adobe/Edge/SumatraPDF) para
conferir o Multiply e a legibilidade.

## Limitações conhecidas
- Nada foi compilado/executado (ver topo).
- Os testes semânticos usam um gerador **de teste** (grupos de sinônimos): validam
  a lógica (indexação, reuso, invalidação, fusão, filtros, isolamento, exportação),
  **não** a qualidade semântica do modelo real.
- Limiar de similaridade e pesos do RRF são valores iniciais, sem calibração.
- Sem evidência visual para PDF com texto digital e páginas com várias imagens.
- Orientação EXIF não é aplicada (apenas avisada).
- A comparação de vetores é força bruta (adequada a ~500 arquivos; não medida).
- O instalador do Windows com o modelo embutido não faz parte desta entrega.
