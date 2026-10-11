# Evidence Finder

Aplicação desktop para Windows, **100% local**, para pesquisar informações em capturas de tela, imagens, conversas em texto, arquivos TXT e PDFs.

> **Esta é a Fase 1 (fundação) e NÃO representa o produto completo.** Hoje o aplicativo só cria e abre projetos locais. Importação, OCR, indexação, busca e exportação de evidências **ainda não existem**.

## Objetivo

Permitir que a pessoa reúna material em projetos locais (até ~500 arquivos por projeto, conforme hardware e tamanho dos arquivos) e, nas próximas fases, encontre nele o que procura, com trechos e referência à fonte, exportando evidências para PDFs e pastas.

Princípios: privacidade, rastreabilidade das fontes, confiabilidade e funcionamento offline.

## Estado atual (Fase 1)

| Funcionalidade | Estado |
| --- | --- |
| Criar projeto (nome + pasta escolhida no diálogo nativo) | Implementado |
| Abrir projeto existente (com validação de integridade básica) | Implementado |
| Metadados persistidos em SQLite local (id, nome, datas) | Implementado |
| Projetos recentes (atalhos; remover da lista não apaga arquivos) | Implementado |
| Estados vazio/carregando/sucesso/erro e confirmações | Implementado |
| Importação, OCR, PDF, indexação, busca, exportação | **Não implementado** (fases futuras) |
| Funcionamento offline integral | **Não** declarado: ainda não há OCR/IA; a arquitetura não depende de rede |

> **Verificação:** o código foi escrito em um ambiente sem Rust, sem Tauri e sem acesso à internet. Portanto **ainda não foi compilado nem testado de ponta a ponta**. Rode `npm run check` na sua máquina (veja abaixo) antes de confiar nele e corrija o que aparecer.

## Tecnologias

- Tauri 2 (casca desktop) + `tauri-plugin-dialog` (usado só pelo lado Rust)
- React 19 + TypeScript em modo estrito + Vite
- Rust (núcleo): `rusqlite` (SQLite embutido), `serde`, `uuid`, `chrono`, `dunce`
- Testes: Vitest (frontend) e `cargo test` (núcleo); lint com ESLint

Nenhum serviço pago, conta externa, telemetria ou API de IA.

## Uso simples (sem linha de comando)

1. Baixe e extraia o ZIP em uma pasta de caminho curto (ex.: `C:\EvidenceFinder`), **não** dentro do OneDrive.
2. Dê dois cliques em **`INICIAR.bat`**.
3. Na primeira vez, o script instala sozinho o que faltar (Node.js, Rust, Ferramentas C++, WebView2) via `winget`, compila o app e o abre. Se o Windows pedir permissão de administrador, clique em **Sim**. Leva de 10 a 30+ minutos e precisa de internet e vários GB livres.
4. Nas próximas vezes, `INICIAR.bat` abre o app em segundos. Use `RECOMPILAR.bat` após atualizar o código.

Requer Windows 10 (1809+) ou 11 com o "Instalador de Aplicativo" (winget). Se algo falhar, a mensagem aparece na janela e o log fica em `instalacao-log.txt`. Estes scripts ainda não foram testados em um Windows real.

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

Os testes Rust cobrem: criação válida, rejeição de nomes inválidos, persistência e recuperação, abertura, projeto inexistente/inválido/corrompido/de versão futura, erros de acesso (pasta inexistente, arquivo no lugar de pasta, permissão negada em Unix), colisões sem sobrescrita, tentativas de *path traversal* (`..`, separadores, nomes reservados do Windows, links simbólicos) e tokens de pasta não autorizados.

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
- **Núcleo Rust** (`src-tauri/src/`): `commands.rs` (superfície exposta), `service.rs` (criar/abrir), `db.rs` (esquema e migrações), `registry.rs` (recentes), `validation.rs`, `paths.rs`, `authorization.rs`, `error.rs` (erros estruturados `{ code, message }`).
- **Projeto no disco:** `<pasta escolhida>/<nome do projeto>/evidence-finder.db`. Nenhuma outra pasta é criada ainda.
- **Metadados:** o `.db` do projeto guarda id (UUID v4), nome e datas (RFC 3339 UTC). O diretório associado **não** é gravado no projeto (para que a pasta possa ser movida); ele fica na lista de recentes, no diretório de dados do aplicativo.
- **Migrações:** `PRAGMA user_version` + `PRAGMA application_id`; novas migrações são acrescentadas ao fim de `PROJECT_MIGRATIONS`. Projetos de versão mais nova são recusados, não alterados.

## Segurança: o que existe e o que não existe

Implementado:

- A interface **não envia caminhos**: a pasta é escolhida no diálogo nativo (aberto pelo Rust) e o núcleo devolve um token opaco, válido só na sessão. Criar/abrir só aceitam esses tokens.
- Nome do projeto validado no núcleo (sem separadores, `..`, caracteres proibidos, nomes reservados do Windows, controles/bidi, limite de 80 caracteres).
- Caminhos canônicos (`dunce::canonicalize`) e checagem de que o projeto criado é filho direto da pasta autorizada, em vez de comparar apenas texto.
- Criação com `create_dir` (falha se já existir): nunca sobrescreve; em falha, remove apenas o que a própria operação criou.
- Abrir não modifica nem apaga nada; arquivos que não sejam um projeto íntegro são recusados sem "reparo". Atalho simbólico no lugar do `.db` é recusado.
- Permissões Tauri mínimas (`capabilities/default.json` sem permissões; o frontend não tem acesso a plugins de arquivos, shell ou rede) e CSP restritiva.
- Sem telemetria, sem rede, sem logs de caminhos ou conteúdo.

Limitações (não prometemos segurança absoluta):

- **O SQLite não criptografa o conteúdo por padrão.** Nomes, caminhos e datas ficam legíveis para quem tiver acesso ao arquivo. O mesmo vale para a lista de recentes (`app-registry.db`).
- Não protege contra malware, outros usuários com acesso à sua conta do Windows ou ao disco.
- A checagem de caminhos tem uma janela teórica entre verificar e usar (TOCTOU) se outro processo trocar pastas por links no meio da operação.
- Dois projetos copiados com o mesmo id compartilham a mesma entrada em "recentes".
- Se registrar o projeto em "recentes" falhar, a criação/abertura continua válida e o projeto apenas não aparece na lista.

## Limitações conhecidas

- Não compilado/testado de ponta a ponta por quem escreveu (ver aviso acima); versões das dependências usam intervalos compatíveis e **não há lockfile** ainda.
- Sem instalador assinado; o Windows SmartScreen pode alertar.
- Interface somente em português do Brasil.

## Próxima fase (não iniciada)

Importação de arquivos para o projeto (cópia/referência, hash, metadados) e modelo de dados para documentos. OCR, indexação e busca vêm depois.
