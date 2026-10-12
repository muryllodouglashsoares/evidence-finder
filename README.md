# Evidence Finder

Aplicação desktop para Windows, **100% local**, para pesquisar informações em capturas de tela, imagens, conversas em texto, arquivos TXT e PDFs.

> **Esta é a Fase 2 (importação e gerenciamento local de arquivos) e NÃO representa o produto completo.** O aplicativo cria projetos, importa e organiza documentos. Leitura de conteúdo (OCR, extração de texto de PDF), indexação, busca e exportação de evidências **ainda não existem**: os documentos ficam organizados e persistidos, mas **não são pesquisáveis**.

## Objetivo

Permitir que a pessoa reúna material em projetos locais (até ~500 arquivos por projeto, conforme hardware e tamanho dos arquivos) e, nas próximas fases, encontre nele o que procura, com trechos e referência à fonte, exportando evidências para PDFs e pastas.

Princípios: privacidade, rastreabilidade das fontes, confiabilidade e funcionamento offline.

## Estado atual (Fase 2)

| Funcionalidade | Estado |
| --- | --- |
| Criar/abrir projeto, metadados em SQLite, projetos recentes (Fase 1) | Implementado |
| Importar PNG, JPEG, TXT e PDF (um ou vários por vez) | Implementado |
| Adicionar texto copiado (salvo como TXT dentro do projeto) | Implementado |
| Modalidade A: referenciar os originais | Implementado |
| Modalidade B: copiar para a pasta do projeto, com verificação de integridade | Implementado |
| Detecção de duplicatas (SHA-256) | Implementado |
| Disponibilidade (arquivo movido/alterado/cópia danificada) e verificação completa sob demanda | Implementado |
| Remover documento (nunca apaga o original) | Implementado |
| Limite de 500 documentos por projeto (interface e núcleo) | Implementado |
| OCR, extração de texto de PDF, indexação, busca, exportação | **Não implementado** (Fase 3 em diante) |

## Como importar documentos

1. Abra ou crie um projeto e clique em **Importar arquivos…** (ou **Adicionar texto…** para colar um texto).
2. Escolha um ou vários arquivos no diálogo do Windows. Só PNG, JPEG, TXT e PDF aparecem.
3. Escolha como guardar e confirme:
   - **Copiar para o projeto (recomendado):** uma cópia de cada arquivo fica em `<projeto>/files/`. O projeto continua completo se você mover ou apagar os originais. Usa mais disco.
   - **Apenas referenciar os originais:** nada é copiado; o projeto guarda o local de cada arquivo. Se o arquivo for movido, renomeado ou alterado, o documento aparece como indisponível/alterado.
4. Ao final, um resumo lista duplicatas ignoradas e arquivos com erro. A falha de um arquivo não interrompe os demais.

Detalhes do comportamento:

- **Formato:** é conferido pelo conteúdo (assinatura PNG/JPEG, marcador `%PDF-`, texto sem bytes nulos), e a extensão precisa ser coerente com ele. Arquivos de outro tipo, vazios ou com extensão trocada são recusados. TXT em UTF-8, Windows-1252 ou UTF-16 com BOM é aceito (a decodificação fica para a Fase 3).
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
- Rust (núcleo): `rusqlite` (SQLite embutido), `serde`, `uuid`, `chrono`, `dunce`, `sha2` (SHA-256 local, Rust puro)
- Testes: Vitest (frontend) e `cargo test` (núcleo); lint com ESLint

Nenhum serviço pago, conta externa, telemetria ou API de IA.

## Uso simples (sem linha de comando)

1. Baixe e extraia o ZIP em uma pasta de caminho curto (ex.: `C:\EvidenceFinder`), **não** dentro do OneDrive.
2. Dê dois cliques em **`INICIAR.bat`**.
3. Na primeira vez, o script instala sozinho o que faltar (Node.js, Rust, Ferramentas C++, WebView2) via `winget`, compila o app e o abre. Se o Windows pedir permissão de administrador, clique em **Sim**. Leva de 10 a 30+ minutos e precisa de internet e vários GB livres.
4. Nas próximas vezes, `INICIAR.bat` abre o app em segundos. **Depois de atualizar o código (como nesta Fase 2), use `RECOMPILAR.bat`:** o `INICIAR.bat` só abre o executável já compilado e, se ele existir, não recompila.

Requer Windows 10 (1809+) ou 11 com o "Instalador de Aplicativo" (winget). Se algo falhar, a mensagem aparece na janela e o log fica em `instalacao-log.txt`. Estes scripts não foram alterados na Fase 2 e ainda não foram testados em um Windows real por quem escreveu o código.

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
npm run test:rust      # cargo test (núcleo: criação, abertura, persistência, erros, segurança)
npm run check          # todos os anteriores
```

Os testes Rust (64) cobrem, além da Fase 1: importação de PNG/JPEG/TXT/PDF nas duas modalidades, texto colado, lotes com falhas individuais, persistência após reabrir, duplicatas, arquivos inexistentes/sem permissão, falhas de cópia, nomes e caminhos maliciosos (banco adulterado, pasta `files` substituída por arquivo ou atalho), remoção sem tocar nos originais, limite de 500, integridade das cópias, arquivos modificados e migração v1→v2 preservando dados. Os testes do front (Vitest) cobrem contratos com o núcleo (só ids/tokens, nunca caminhos) e regras de apresentação.

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
- **Núcleo Rust** (`src-tauri/src/`): `commands.rs` (superfície exposta), `service.rs` (criar/abrir), `documents.rs` (importar, listar, verificar, remover), `docformat.rs` (formatos), `docstore.rs` (SQL de documentos), `db.rs` (esquema e migrações), `registry.rs` (recentes), `validation.rs`, `paths.rs`, `authorization.rs`, `error.rs` (erros estruturados `{ code, message }`).
- **Projeto no disco:** `<pasta escolhida>/<nome do projeto>/evidence-finder.db` e, ao copiar documentos, `files/`.
- **Projeto aberto:** o núcleo guarda qual projeto está aberto; os comandos de documentos operam só nele e a interface nunca informa caminhos de projeto.
- **Metadados:** o `.db` do projeto guarda id (UUID v4), nome e datas (RFC 3339 UTC). O diretório associado **não** é gravado no projeto (para que a pasta possa ser movida); ele fica na lista de recentes, no diretório de dados do aplicativo.
- **Migrações:** v1 = metadados do projeto; v2 (Fase 2) = tabela `document` (id, projeto, nome original, formato, tamanho, caminho de origem, caminho da cópia, modalidade, data, SHA-256, disponibilidade, estado e mensagem de erro) com índice único em `sha256`. A v2 é aditiva: projetos da Fase 1 são migrados sem perder dados. `PRAGMA user_version` + `PRAGMA application_id`; novas migrações são acrescentadas ao fim de `PROJECT_MIGRATIONS`. Projetos de versão mais nova são recusados, não alterados.

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
- Arquivos que falham não geram registro (são só relatados no resumo); é preciso selecioná-los de novo para tentar outra vez.
- Atalho/junção do Windows escolhido como arquivo é resolvido para o destino real antes de importar.
- Importações simultâneas são serializadas; se o app fechar no meio de uma cópia, pode sobrar um arquivo `*.part` em `files/` (sem registro; pode ser apagado).
- Sem instalador assinado; o Windows SmartScreen pode alertar. Interface somente em português do Brasil.

## Próxima fase (não iniciada)

**Fase 3 — extração de texto e indexação local:** OCR de imagens, texto de PDFs, indexação e busca. Nada disso existe ainda.
