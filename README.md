# Evidence Finder

Aplicação desktop para Windows, **100% local**, para pesquisar informações em capturas de tela, imagens, conversas em texto, arquivos TXT e PDFs.

> **Esta é a Fase 5 (exportação de evidências) e NÃO representa o produto completo.** O aplicativo cria projetos, importa documentos, extrai o texto localmente, permite **pesquisar** esse texto (SQLite FTS5) e agora **exporta** os resultados selecionados em um pacote local (relatório PDF, fontes verificadas e `manifest.json`). **Busca semântica/IA e o endurecimento final de segurança ainda não existem, e a Fase 5 ainda não foi validada em um Windows real (veja "Limitações da Fase 5").**

## Objetivo

Permitir que a pessoa reúna material em projetos locais (até ~500 arquivos por projeto, conforme hardware e tamanho dos arquivos) e, nas próximas fases, encontre nele o que procura, com trechos e referência à fonte, e (em fase futura) exporte evidências para PDFs e pastas.

Princípios: privacidade, rastreabilidade das fontes, confiabilidade e funcionamento offline.

## Estado atual (Fase 5)

| Funcionalidade | Estado |
| --- | --- |
| Criar/abrir projeto, metadados em SQLite, projetos recentes (Fase 1) | Implementado |
| Importar PNG, JPEG, TXT e PDF (um ou vários por vez) | Implementado |
| Adicionar texto copiado (guardado no projeto e processado direto, sem salvar TXT) | Implementado |
| Modalidade A: referenciar os originais | Implementado |
| Modalidade B: copiar para a pasta do projeto, com verificação de integridade | Implementado |
| Detecção de duplicatas (SHA-256) | Implementado |
| Disponibilidade (arquivo movido/alterado/cópia danificada) e verificação completa sob demanda | Implementado |
| Remover documento (nunca apaga o original) | Implementado |
| Limite de 500 documentos por projeto (interface e núcleo) | Implementado |
| Extração de TXT (UTF-8, BOM, UTF-16 com BOM, Windows-1252) e de texto digitado | Implementado |
| Extração de texto de PDF, por página | Implementado |
| OCR local (Tesseract) de PNG/JPEG e de PDFs digitalizados, com caixas e confiança | Implementado — **exige o Tesseract instalado** (ver "Extração de texto") |
| Estado, progresso e tentativa novamente na interface | Implementado |
| Busca lexical local (SQLite FTS5): palavras, frase exata, combinação de termos, ordenada por relevância | Implementado (Fase 4) |
| Filtros (documento, formato, período de importação) e ordenação, aplicados no núcleo | Implementado (Fase 4) |
| Contexto do trecho (vizinhos do mesmo documento) e abertura segura do original | Implementado (Fase 4) |
| Seleção de resultados, revisão, relatório PDF, pasta de evidências com fontes verificadas por SHA-256 e `manifest.json` | Implementado (Fase 5; pendente de validação no Windows) |
| Busca semântica / IA local | **Não implementado** (Fase 7) |

## Como importar documentos

1. Abra ou crie um projeto e clique em **Importar arquivos…** (ou **Adicionar texto…** para colar um texto).
2. Escolha um ou vários arquivos no diálogo do Windows. Só PNG, JPEG, TXT e PDF aparecem.
3. Escolha como guardar e confirme:
   - **Copiar para o projeto (recomendado):** uma cópia de cada arquivo fica em `<projeto>/files/`. O projeto continua completo se você mover ou apagar os originais. Usa mais disco.
   - **Apenas referenciar os originais:** nada é copiado; o projeto guarda o local de cada arquivo. Se o arquivo for movido, renomeado ou alterado, o documento aparece como indisponível/alterado.
4. Ao final, um resumo lista duplicatas ignoradas e arquivos com erro. A falha de um arquivo não interrompe os demais.

Detalhes do comportamento:

- **Formato:** é conferido pelo conteúdo (assinatura PNG/JPEG, marcador `%PDF-`, texto sem bytes nulos), e a extensão precisa ser coerente com ele. Arquivos de outro tipo, vazios ou com extensão trocada são recusados. TXT em UTF-8, Windows-1252 ou UTF-16 com BOM é aceito (a decodificação é feita na extração de texto, Fase 3).
- **Duplicatas:** documentos com o mesmo SHA-256 não são importados de novo (nem entre modalidades). Mesmo nome com conteúdo diferente é mantido. O hash identifica conteúdo idêntico; **não** prova autenticidade ou procedência.
- **Tamanho:** até 500 MB por arquivo; texto colado até 5 MB. Arquivos são lidos em blocos (sem carregar tudo na memória).
- **Limite:** 500 documentos por projeto. Ao atingi-lo, os arquivos excedentes são recusados.
- **Remoção:** em "referência" só o registro é removido; em "cópia", o registro e a cópia gerenciada são excluídos. **O arquivo original nunca é apagado.**
- **Verificar:** relê o arquivo e confere o SHA-256. Cópia diferente → "Cópia danificada"; original diferente → "Alterado após a importação". A listagem faz só uma checagem rápida (existência/tamanho/data).

## Onde os dados privados ficam

Tudo fica na **pasta do projeto que você escolheu** (fora do repositório Git):

```
<pasta escolhida>/<nome do projeto>/
  evidence-finder.db      banco SQLite (metadados, hashes, caminhos)
  files/<uuid>.<ext>      cópias gerenciadas (nome gerado; o nome original fica só no banco)
```

A lista de projetos recentes fica em `app-registry.db`, na pasta de dados do aplicativo. O `.gitignore` ignora bancos, exportações e arquivos temporários.

## Tecnologias

- Tauri 2 (casca desktop) + `tauri-plugin-dialog` (usado só pelo lado Rust)
- React 19 + TypeScript em modo estrito + Vite
- Rust (núcleo): `rusqlite` (SQLite embutido), `serde`, `uuid`, `chrono`, `dunce`, `sha2` (SHA-256 local, Rust puro), `lopdf` (leitura de PDF, Rust puro, MIT), `encoding_rs` (decodificação de TXT); Fase 5: `flate2` e `ttf-parser` (geração do PDF) e as fontes DejaVu Sans (ver "Exportação de evidências")
- Testes: Vitest (frontend) e `cargo test` (núcleo); lint com ESLint

Nenhum serviço pago, conta externa, telemetria ou API de IA.

## Uso simples (sem linha de comando)

1. Baixe e extraia o ZIP em uma pasta de caminho curto (ex.: `C:\EvidenceFinder`), **não** dentro do OneDrive.
2. Dê dois cliques em **`INICIAR.bat`**.
3. Na primeira vez, o script instala sozinho o que faltar (Node.js, Rust, Ferramentas C++, WebView2 e o Tesseract OCR com o idioma português) via `winget`, compila o app e o abre. Se o Windows pedir permissão de administrador, clique em **Sim**. Leva de 10 a 30+ minutos e precisa de internet e vários GB livres.
4. Nas próximas vezes, `INICIAR.bat` abre o app em segundos. **Depois de atualizar o código (como nesta Fase 4), use `RECOMPILAR.bat`:** o `INICIAR.bat` só abre o executável já compilado e, se ele existir, não recompila.

Requer Windows 10 (1809+) ou 11 com o "Instalador de Aplicativo" (winget). Se algo falhar, a mensagem aparece na janela e o log fica em `instalacao-log.txt`. O script também instala o Tesseract (`UB-Mannheim.TesseractOCR`) e baixa `por.traineddata` para a pasta `tessdata` dele (pode pedir permissão de administrador); se isso falhar, mostra um aviso e o app abre mesmo assim, sem OCR. Os scripts ainda não foram testados em um Windows real por quem escreveu o código.

## Pré-requisitos para desenvolvedores (Windows 10/11)

1. [Node.js](https://nodejs.org) LTS (>= 20)
2. [Rust](https://rustup.rs) estável (>= 1.77.2) com o toolchain MSVC
3. Microsoft C++ Build Tools (carga "Desenvolvimento para desktop com C++")
4. WebView2 (já vem no Windows 11; no Windows 10 instale o Evergreen Runtime)

Detalhes oficiais: <https://v2.tauri.app/start/prerequisites/>

## Instalação e execução

```bash
npm install            # gera package-lock.json na primeira vez; versione-o
npm run app:dev        # abre o aplicativo desktop em desenvolvimento
```

`npm run dev` sobe apenas o frontend no navegador; nesse modo o núcleo local não existe e a interface mostra um aviso.

## Verificações e testes

```bash
npm run typecheck      # TypeScript estrito
npm run lint           # ESLint
npm run test           # Vitest (frontend)
npm run test:rust      # cargo test (núcleo: criação, abertura, persistência, erros, segurança, busca)
npm run check          # todos os anteriores
```

Os testes Rust (164: 125 das fases anteriores + 39 da Fase 5) cobrem, além da Fase 1: importação de PNG/JPEG/TXT/PDF nas duas modalidades, texto colado, lotes com falhas individuais, persistência após reabrir, duplicatas, arquivos inexistentes/sem permissão, falhas de cópia, nomes e caminhos maliciosos (banco adulterado, pasta `files` substituída por arquivo ou atalho), remoção sem tocar nos originais, limite de 500, integridade das cópias, arquivos modificados e migração v1→v2 preservando dados. Os da Fase 3 cobrem decodificação e segmentação de texto, PDF com páginas, PDF digitalizado com OCR real (JPEG, Flate cinza e 1 bit), PNG/JPEG com e sem texto, dependência de OCR ausente, arquivos corrompidos, falha individual sem interromper o lote, reprocessamento sem duplicar, detecção de arquivo alterado por hash, extrator desatualizado, persistência após reabrir, remoção em cascata, migração v2→v3 e limites. Os testes de OCR real são pulados (com aviso em stderr) se o Tesseract com idioma `por` não estiver instalado; os demais rodam sempre. Os da Fase 4 cobrem a montagem segura da consulta, busca por palavra/frase/combinação, acentos e maiúsculas, caracteres especiais e operadores, ordenação por relevância, prévia e destaque, filtros combinados, paginação, isolamento entre projetos, contexto, migração de um projeto da Fase 3 (v3→v4) sem reimportar, ausência de duplicação ao reprocessar, remoção em cascata no índice, recuperação de um índice interrompido, consistência ao reabrir e a validação da abertura da fonte (ausente, alterada, outro projeto, extensão adulterada). Os testes do front (Vitest) cobrem contratos com o núcleo (só ids/tokens, nunca caminhos), validação da consulta, filtros, descrição da origem e o redutor que descarta respostas obsoletas. Os da Fase 5 estão descritos em "Exportação de evidências".

## Build

```bash
npm run app:build      # gera o instalador NSIS em src-tauri/target/release/bundle/
```

Os ícones em `src-tauri/icons/` são provisórios (`icon-source.png` pode ser substituído; depois rode `npm run tauri icon src-tauri/icons/icon-source.png`).

## Arquitetura

```
React (src/)  ──invoke──▶  Comandos Tauri (commands.rs)
 pages/components             │  valida e autoriza
 services/core.ts (única      ▼
 ponte com o núcleo)       service.rs (regras) ─▶ db.rs (SQLite do projeto)
                              │                  registry.rs (SQLite de recentes)
                              └▶ paths.rs · validation.rs · authorization.rs
```

- **Frontend** (`src/`): telas, componentes e `services/core.ts`, a única camada que chama o núcleo. Contratos em `types/contracts.ts`.
- **Núcleo Rust** (`src-tauri/src/`): `commands.rs` (superfície exposta), `service.rs` (criar/abrir), `documents.rs` (importar, listar, verificar, remover, validar a fonte a abrir), `docformat.rs` (formatos), `docstore.rs` (SQL de documentos), `extraction.rs` (pipeline e lote), `extraction_store.rs` (SQL de extração/segmentos), `txt_extract.rs`, `pdf_extract.rs`, `ocr.rs` (Tesseract), `search.rs` (consulta FTS5, filtros, prévia, contexto), `db.rs` (esquema e migrações), `registry.rs` (recentes), `validation.rs`, `paths.rs`, `authorization.rs`, `error.rs` (erros estruturados `{ code, message }`).
- **Projeto no disco:** `<pasta escolhida>/<nome do projeto>/evidence-finder.db` e, ao copiar documentos, `files/`.
- **Projeto aberto:** o núcleo guarda qual projeto está aberto; os comandos de documentos operam só nele e a interface nunca informa caminhos de projeto.
- **Metadados:** o `.db` do projeto guarda id (UUID v4), nome e datas (RFC 3339 UTC). O diretório associado **não** é gravado no projeto (para que a pasta possa ser movida); ele fica na lista de recentes, no diretório de dados do aplicativo.
- **Migrações:** v1 = metadados do projeto; v2 (Fase 2) = tabela `document` (id, projeto, nome original, formato, tamanho, caminho de origem, caminho da cópia, modalidade, data, SHA-256, disponibilidade, estado e mensagem de erro) com índice único em `sha256`. A v2 é aditiva: projetos da Fase 1 são migrados sem perder dados. `PRAGMA user_version` + `PRAGMA application_id`; novas migrações são acrescentadas ao fim de `PROJECT_MIGRATIONS`. v3 (Fase 3) = tabelas `extraction` (uma linha por documento) e `text_segment` (trechos com origem), ambas com `ON DELETE CASCADE` a partir de `document` (a conexão agora ativa `PRAGMA foreign_keys`). Também aditiva: documentos da Fase 2 não têm linha em `extraction`, o que significa "pendente". v4 (Fase 4) = índice FTS5 `text_segment_fts` (ver "Busca e análise dos resultados"), também aditiva. Projetos de versão mais nova são recusados, não alterados.

## Segurança: o que existe e o que não existe

Implementado:

- A interface **não envia caminhos**: a pasta é escolhida no diálogo nativo (aberto pelo Rust) e o núcleo devolve um token opaco, válido só na sessão. Criar/abrir só aceitam esses tokens.
- Nome do projeto validado no núcleo (sem separadores, `..`, caracteres proibidos, nomes reservados do Windows, controles/bidi, limite de 80 caracteres).
- Caminhos canônicos (`dunce::canonicalize`) e checagem de que o projeto criado é filho direto da pasta autorizada, em vez de comparar apenas texto.
- Criação com `create_dir` (falha se já existir): nunca sobrescreve; em falha, remove apenas o que a própria operação criou.
- Abrir não modifica nem apaga nada; arquivos que não sejam um projeto íntegro são recusados sem "reparo". Atalho simbólico no lugar do `.db` é recusado.
- Permissões Tauri mínimas (`capabilities/default.json` sem permissões; o frontend não tem acesso a plugins de arquivos, shell ou rede) e CSP restritiva.
- Arquivos a importar são escolhidos no diálogo nativo (aberto pelo Rust) e entregues à interface como tokens de uso único; o núcleo canonicaliza o caminho e abre o arquivo uma só vez (metadados, formato, hash e cópia vêm do mesmo arquivo aberto).
- Cópias gerenciadas: nome gerado (`<uuid>.<ext>`, sem nomes de usuário no disco), criação com `create_new` (nunca sobrescreve), gravação em `.part`, releitura e comparação do SHA-256 antes de publicar; em falha, só o que a própria operação criou é removido. A pasta `files` precisa ser uma pasta real, filha direta do projeto (atalhos são recusados).
- Remoção: só apaga caminhos no formato exato `files/<uuid>.<ext>`; um banco adulterado não consegue apontar para arquivos fora da pasta gerenciada. Originais nunca são apagados.
- Nomes exibidos têm controles e caracteres bidirecionais invisíveis neutralizados. Arquivos importados nunca são executados.
- Mensagens de erro e avisos não incluem caminhos completos.
- Busca: o texto digitado nunca é concatenado ao SQL nem vira sintaxe FTS5 (cada termo vira uma frase entre aspas passada como parâmetro; filtros também são parâmetros); a consulta tem limite de tamanho e de termos. O núcleo restringe tudo ao projeto aberto (`project_id`), e o texto extraído é exibido sempre como texto (nunca HTML).
- Abrir o original: a interface envia só o id do documento. O núcleo busca o caminho no banco do projeto, recusa fonte ausente, alterada, danificada ou sem permissão, exige arquivo comum com extensão coerente com o formato importado (PNG/JPEG/PDF/TXT; nunca executáveis) e chama o aplicativo padrão do sistema (crate `open`, ShellExecute no Windows, sem passar por um shell). Nenhuma permissão nova do Tauri foi concedida ao frontend.
- Sem telemetria, sem rede, sem logs de caminhos ou conteúdo.

Limitações (não prometemos segurança absoluta):

- **O SQLite não criptografa o conteúdo por padrão.** Nomes, caminhos, hashes e datas ficam legíveis para quem tiver acesso ao arquivo; **as cópias gerenciadas também ficam sem criptografia** na pasta do projeto. O mesmo vale para a lista de recentes (`app-registry.db`).
- Não protege contra malware, outros usuários com acesso à sua conta do Windows ou ao disco.
- A checagem de caminhos tem uma janela teórica entre verificar e usar (TOCTOU) se outro processo trocar pastas por links no meio da operação.
- Dois projetos copiados com o mesmo id compartilham a mesma entrada em "recentes".
- Se registrar o projeto em "recentes" falhar, a criação/abertura continua válida e o projeto apenas não aparece na lista.

## Limitações conhecidas

- **Verificado em Linux** (cargo test/clippy, typecheck, lint, Vitest, build do front e teste visual com núcleo simulado). **Não verificado em Windows:** o diálogo nativo de arquivos, caminhos reais do Windows (unidades de rede, OneDrive, caminhos longos, nomes reservados), o executável/instalador gerado e os scripts `.bat`/`.ps1`. Faça esses testes no Windows antes de confiar.
- A importação roda em uma única operação sem barra de progresso; lotes grandes de arquivos pesados podem levar tempo (a interface mostra "Importando…").
- A detecção de arquivo alterado em modo "referência" usa tamanho e data de modificação na listagem; só **Verificar** compara o conteúdo.
- Arquivos que falham na importação não geram registro (são só relatados no resumo); é preciso selecioná-los de novo para tentar outra vez.
- Atalho/junção do Windows escolhido como arquivo é resolvido para o destino real antes de importar.
- Importações simultâneas são serializadas; se o app fechar no meio de uma cópia, pode sobrar um arquivo `*.part` em `files/` (sem registro; pode ser apagado).
- Sem instalador assinado; o Windows SmartScreen pode alertar. Interface somente em português do Brasil.

## Extração de texto (Fase 3)

**O que faz.** Para cada documento, lê o conteúdo (conferindo o SHA-256 registrado na importação), extrai o texto e grava **segmentos** no SQLite (`text_segment`) ligados ao documento e ao projeto, com: página (PDF), linhas e posição em caracteres (TXT/texto digitado), caixa delimitadora e dimensões da imagem (OCR), confiança do OCR e método (`text`, `pdf_text`, `ocr`, `manual`). O estado por documento fica em `extraction` (pendente, processando, concluído, sem texto, falha, conteúdo alterado), com hash do conteúdo extraído, versão do extrator, motor usado, mensagem segura e datas. A migração v3 é só aditiva; projetos da Fase 2 abrem normalmente e seus documentos aparecem como "não processados". O índice FTS5 foi criado na Fase 4.

**Como usar.** Na lista de documentos: **Processar pendentes** (roda em segundo plano, com progresso; falha em um documento não interrompe os outros), **Tentar novamente** por documento e **Tentar falhas novamente**. Texto adicionado com **Adicionar texto…** é processado automaticamente.

**Regras.** Idempotente: reprocessar um documento inalterado, com a versão atual do extrator, não regrava nem duplica segmentos. Se o conteúdo não confere mais com o hash registrado (arquivo "referência" alterado), os resultados antigos são descartados e o documento fica "Conteúdo alterado" (a extração nunca usa um conteúdo diferente do registrado). Quando `EXTRACTOR_VERSION` (em `extraction.rs`) aumenta, os resultados antigos aparecem como "desatualizados" e são refeitos em **Processar pendentes**. Originais nunca são alterados.

**Limites (explícitos, com mensagem):** TXT até 50 MB; imagem até 50 MB e 100 megapixels; PDF até 100 MB e 500 páginas; no máximo 100 páginas de PDF com OCR por documento; 180 s por imagem/página no OCR. PDFs são lidos inteiros em memória (limite de 100 MB).

### OCR: dependência do Tesseract (leia antes de usar imagens e PDFs digitalizados)

O OCR usa o **Tesseract 5** (Apache-2.0) como processo local, sem rede, recebendo a imagem pela entrada padrão (sem arquivos temporários). **O Tesseract não é empacotado dentro do app**; quem o instala é o script `INICIAR.bat`/`RECOMPILAR.bat` (passo extra, ver "Uso simples"), ou você manualmente. Sem ele, TXT, texto digitado e PDFs com texto selecionável funcionam; imagens e PDFs digitalizados ficam com a falha "OCR indisponível" e a explicação, e podem ser reprocessados depois com **Tentar novamente**.

Como obter (Windows): instale o Tesseract (por exemplo, o instalador do projeto UB Mannheim ou `winget install UB-Mannheim.TesseractOCR`) e **marque o idioma Portuguese** (arquivo `por.traineddata` em `tessdata`). O **idioma `por` é obrigatório** (sem ele o OCR é recusado em vez de gerar texto ruim em silêncio); `eng` é usado junto se existir. O app procura o executável, nesta ordem: variável `EVIDENCE_FINDER_TESSERACT` (caminho do `tesseract.exe`), pasta `tesseract\` ao lado do executável do app (com `tessdata\` dentro, preparada para empacotamento futuro), `C:\Program Files\Tesseract-OCR`, e as pastas do `PATH`. Depois de instalar, use **Verificar novamente** no aviso da lista. Funciona sem internet. **Pendência de empacotamento:** incluir o Tesseract e `por.traineddata` no instalador (`bundle.resources`) e testá-lo em um Windows real.

### Limitações conhecidas da Fase 3

- **PDF digitalizado:** o OCR usa a imagem embutida em cada página. Suportados: JPEG (DCT) e imagens Flate/LZW em cinza ou RGB de 8 bits, e cinza de 1 bit. **Não suportados** (falha explícita, sem OCR simulado): CCITT, JBIG2 e JPEG 2000 (comuns em digitalizações em preto e branco), paleta indexada, CMYK, imagens dentro de formulários aninhados e páginas cujo texto foi convertido em desenho vetorial. Páginas com mais de uma imagem relevante são lidas, mas sem caixa delimitadora.
- PDFs protegidos por senha são recusados com mensagem; o texto de PDFs com fontes incomuns pode sair incompleto (a leitura de PDF é em Rust puro; PDFs problemáticos são isolados e falham sem derrubar o app). Texto de PDF não guarda linha/posição (somente página).
- OCR de imagens não aplica rotação EXIF; a qualidade depende de resolução e legibilidade (a confiança fica registrada; média abaixo de 60% gera aviso).
- O progresso é por documento (não por página). Não há botão de cancelar o lote.
- O conteúdo extraído fica **sem criptografia** no SQLite do projeto (como o resto dos dados). Proteja a pasta do projeto (por exemplo com BitLocker).
- Não testado em Windows (ver "Limitações conhecidas"); os testes de OCR real rodaram em Linux com Tesseract 5.3.4.

## Busca e análise dos resultados (Fase 4)

**Como usar.** Na aba **Pesquisar** do projeto aberto, digite a consulta e pressione **Pesquisar**. Cada resultado mostra o documento, formato, página/linhas/região da imagem (só o que existe), o método de extração, a confiança do OCR (quando houver) e um trecho com os termos destacados. **Ver contexto** abre o trecho completo com os vizinhos do mesmo documento; **Abrir original** abre o arquivo no aplicativo padrão. Filtros e ordenação reaplicam a consulta; **Remover filtros** e **Limpar tudo** voltam ao estado inicial. Resultados em lotes de 50 (**Mostrar mais resultados**).

**Consultas:**

| Digite | Significa |
| --- | --- |
| `pagamento` | trechos com a palavra |
| `promessa pagamento` | trechos com **todas** as palavras (E), em qualquer ordem e posição |
| `"próxima sexta-feira"` | expressão exata: palavras adjacentes, nesta ordem |

Maiúsculas, minúsculas e acentos são ignorados (`transferencia` encontra `transferência`). A busca é por **palavra inteira** e **não há operadores**: `OR`, `NOT`, `*`, `:`, parênteses etc. são tratados como texto comum. Limites: 200 caracteres e 16 termos. Consulta vazia ou com aspas sem fechar gera uma mensagem de erro de uso (sem afetar os dados).

**Relevância.** A ordem padrão usa o BM25 do FTS5 (menos diluído em trechos curtos e com mais ocorrências sobe); empates são desfeitos de forma determinística por nome do documento e posição. A interface mostra apenas a posição (`#n`), **não** uma pontuação ou porcentagem. A outra ordenação é "Documento e posição".

**Filtros (executados no SQLite):** documentos específicos, formato (PDF, PNG, JPEG, TXT) e **período de importação** (data em que o arquivo entrou no projeto; o app não tem a data de criação do conteúdo e por isso não oferece filtro por ela). Os filtros se combinam (E). O app não guarda horário de mensagens; nenhum horário é inventado.

**Índice.** A migração v4 cria a tabela virtual `text_segment_fts` (FTS5, conteúdo externo ligado a `text_segment`, tokenizador `unicode61 remove_diacritics 2`) e três gatilhos (inserção, exclusão, atualização) que a mantêm sincronizada em qualquer gravação: nova extração, reprocessamento (substitui os segmentos sem duplicar), remoção de documento (cascata) e invalidação por arquivo alterado. **Projetos antigos:** a própria migração (uma transação) indexa os segmentos já existentes ao abrir o projeto, sem reimportar nem refazer OCR; se for interrompida, é desfeita e repetida na próxima abertura. Além disso, cada busca compara o número de segmentos com o do índice e **reconstroem o índice a partir do texto guardado** se divergirem (recuperação automática, sem OCR). Pesquisar nunca reprocessa documentos.

**Documentos indisponíveis.** Continuam pesquisáveis (o texto está no banco), com o aviso de disponibilidade no resultado; **Abrir original** fica desativado e o núcleo recusa fontes ausentes, alteradas ou danificadas. Documentos ainda não processados ou com falha **não** aparecem nos resultados; a aba avisa quantos são.

**Limitações da Fase 4:** sem busca por prefixo/radical, sinônimos, erros de digitação ou significado (a busca semântica é de fase futura); texto de OCR com erros pode não casar com a consulta (confira a confiança e o original); a busca é feita sobre trechos de até ~1200 caracteres, então termos separados por mais de um trecho não se combinam; sem exportação das evidências; abrir o original depende de haver um aplicativo padrão para o tipo de arquivo; **a interface e a abertura de arquivos no Windows não foram testadas em um Windows real** (testes executados em Linux).

## Exportação de evidências (Fase 5)

### Como usar

1. Em **Pesquisar**, faça a busca e marque a caixa de cada resultado que deseja exportar (ou use **Selecionar os N exibidos**). A barra acima da lista mostra quantos estão selecionados e permite **Limpar seleção**. Nada é exportado automaticamente: só o que foi marcado.
2. **Revisar e exportar** abre a revisão, montada pelo núcleo: consulta e filtros, documentos de origem, **trechos completos** com suas referências (página, linhas, posição, região, método de extração, confiança do OCR quando existe) e os arquivos que serão copiados, com o nome que terão em `evidencias/`. Fontes que já aparecem como ausentes/alteradas ficam destacadas.
3. **Escolher pasta de destino** abre o diálogo nativo (controlado pelo Rust). **Exportar** cria, dentro dela, uma pasta nova `EvidenceFinder-Exportacao-AAAAMMDD-HHMMSS` (se o nome já existir, acrescenta `-2`, `-3`…; nunca sobrescreve).

**Seleção e pesquisa.** A seleção pertence a uma pesquisa (consulta + filtros + estado dos documentos). Se qualquer um deles mudar, a seleção é descartada e a tela avisa quantos itens foram descartados; alterar só a ordenação preserva a seleção. O núcleo confere de novo: cada trecho precisa existir no projeto aberto **e** ainda corresponder à consulta e aos filtros enviados; caso contrário a exportação é recusada. Limite: 500 ocorrências por exportação.

### Estrutura do pacote

```
EvidenceFinder-Exportacao-20261014-112037/
├── relatorio.pdf
├── manifest.json
└── evidencias/
    ├── arquivo_001.txt
    ├── arquivo_002.pdf
    └── arquivo_003.png
```

- Um arquivo de origem é copiado **uma única vez**, mesmo com várias ocorrências; cada ocorrência continua identificada no PDF (`Ocorrência 001`, `002`…) e no manifesto (`occurrenceId`, `segmentId`). Os nomes em `evidencias/` são gerados pelo sistema (`arquivo_NNN` + extensão do formato), nunca a partir do nome original (que fica no manifesto), então não há colisão nem travessia de diretório.
- **`relatorio.pdf`** (A4, numeração "Página i de N"): capa com projeto, ID da exportação, data/hora local, consulta, filtros e contagens; uma seção por ocorrência com documento, arquivo exportado, SHA-256, localização, método e motor de extração. Caixas **azuis** = metadados produzidos pelo sistema; caixas **cinzas** = texto extraído da fonte, literal; caixas **âmbar** = avisos (fonte não incluída, caracteres sem glifo). Acentos e Unicode usam fontes embutidas; textos longos são quebrados por largura real (palavras gigantes por caractere) e continuam entre páginas, sem corte.
- **`manifest.json`** (UTF-8, `schemaVersion` 1): projeto, consulta, filtros, `exportId`, datas (UTC e local), referência ao relatório (nome, SHA-256, tamanho), `occurrences[]` (segmento, documento, nome original, arquivo exportado, hash da fonte **verificado**, localização, método, motor, confiança de OCR, tamanho e SHA-256 do texto), `files[]` (hash registrado × hash verificado, estado, ocorrências) e `statements[]`. Não inclui o texto integral, caminhos absolutos nem dados do ambiente.

### Hashes: o que significam e o que não significam

Ao exportar, cada fonte é lida **uma vez** (somente leitura): os mesmos bytes que são gravados em `evidencias/` alimentam o SHA-256, que é comparado com o hash e o tamanho registrados na importação. Só se conferir o arquivo é incluído e o hash aparece como "verificado". O hash mostra que **aquele conteúdo** é idêntico ao que foi importado; **não comprova autenticidade, autoria, data de criação nem cadeia de custódia formal**. Correspondência textual e OCR também não confirmam fatos. Relatório e manifesto dizem isso e **registram a operação realizada**, nada além.

### Avisos para fontes indisponíveis ou alteradas

- **Ausente, movida ou apagada** · **conteúdo alterado desde a importação** (referência) · **cópia do projeto danificada** · **sem permissão/falha de leitura**: o arquivo **não** é copiado nem apresentado como fonte válida. A ocorrência continua no relatório e no manifesto com o estado (`missing`, `modified`, `corrupted`, `inaccessible`), hash apenas "registrado, NÃO verificado" e um aviso; ao terminar, a tela lista os itens afetados.
- Se **nenhuma** fonte puder ser incluída, a exportação falha e nada é publicado.
- Caracteres sem glifo na fonte do relatório (por exemplo, japonês) são desenhados como "�" com aviso na ocorrência; o texto original permanece no projeto.

### Segurança e confiabilidade

- A interface envia só identificadores de trechos, a consulta/filtros e o **token** da pasta escolhida no diálogo nativo; nunca caminhos. Origem dos arquivos vem do banco do projeto; o destino é revalidado (canônico, existente, fora da área gerenciada `files/` do projeto).
- O pacote é montado em uma pasta temporária (`.EvidenceFinder-temp-*`) no destino e **renomeado** para o nome final só no fim; falhas removem apenas o que a operação criou. Duas exportações simultâneas são recusadas (`BUSY`); importar/buscar continuam livres porque a exportação trabalha sobre um instantâneo lido de uma só vez.
- Sem shell, sem rede, sem logs de conteúdo ou caminhos. Originais e cópias gerenciadas nunca são modificados.

### Bibliotecas e licenças adicionadas

| Componente | Uso | Licença |
|---|---|---|
| `flate2` 1.x (backend Rust puro) | compressão dos fluxos do PDF (já era dependência transitiva do `lopdf`) | MIT OR Apache-2.0 |
| `ttf-parser` 0.25 | mapa de caracteres e larguras da fonte | MIT OR Apache-2.0 |
| DejaVu Sans / Sans Bold (`src-tauri/assets/fonts/`) | fonte embutida no executável e no PDF | Bitstream Vera/DejaVu (permissiva; texto em `LICENSE-DejaVu.txt`) |

O PDF é escrito diretamente pelo módulo `pdf_report.rs`; não há serviço de conversão nem biblioteca de documentos de terceiros.

### Testes da Fase 5

- **Rust (39 novos):** uma ocorrência; várias do mesmo documento (arquivo único); documentos diferentes e nomes iguais; validade e conteúdo do manifesto (sem texto integral nem caminhos); Unicode e trechos longos relidos do próprio PDF pelos mapas `ToUnicode`; paginação e numeração; texto hostil tratado só como dado; documento ausente, alterado (mesmo tamanho), cópia adulterada, não-arquivo e sem permissão; falha total sem publicar nada; ids inexistentes/inválidos/duplicados/de outro projeto/de documento removido; seleção que não corresponde mais à consulta ou filtros; token inválido e destinos inválidos (inexistente, arquivo, área gerenciada); pasta de destino já existente (sem sobrescrita); falhas injetadas em cada etapa (cópia, relatório, manifesto, publicação) sem sobras e sem tocar em arquivos preexistentes; originais preservados byte a byte; hash registrado = hash dos bytes exportados; revisão com texto completo; bloqueio de exportações simultâneas.
- **Frontend (Vitest, 16 novos + 2 de contrato):** seleção/desmarcação por id de trecho, contagem, limite, escopo da pesquisa e descarte de seleção desatualizada, revisão, prevenção de exportação vazia/sem destino, carregamento/sucesso/falha/nova tentativa e chamadas aos comandos Rust (só ids e tokens).

### Limitações da Fase 5

- **Não validada em Windows real:** o diálogo de pasta, caminhos do Windows (OneDrive, rede, caminhos longos) e o instalador NSIS não foram exercitados; a interface (diálogo de revisão) foi verificada por tipos, lint e testes da lógica, **sem testes de renderização de componentes** (a suíte atual não os tem) e sem uso manual. O PDF foi conferido em imagem (poppler, Linux), não em Acrobat/Edge.
- Se o aplicativo for encerrado à força durante a exportação, pode restar a pasta `.EvidenceFinder-temp-*` no destino (incompleta, nunca com nome final; pode ser apagada). A publicação por renomeação verifica antes se o nome existe; no Linux/macOS há uma janela mínima entre verificar e renomear.
- O relatório usa DejaVu Sans (latim, grego, cirílico e símbolos comuns); CJK, emoji e escritas não cobertas viram "�". O PDF embute as fontes completas (~750 KB).
- Fontes de documentos exportados em modo "referência" são lidas do caminho registrado no momento da exportação; o hash é a única garantia de que são as mesmas importadas.
- Sem pré-visualização do PDF no aplicativo e sem botão para abrir a pasta criada (o caminho é exibido).

## Próxima fase (não iniciada)

**Fase 6 — segurança e confiabilidade:** revisão das permissões, arquivos temporários, logs e testes com a rede bloqueada. Antes dela, a Fase 5 precisa ser validada em um Windows real (ver "Limitações da Fase 5").
