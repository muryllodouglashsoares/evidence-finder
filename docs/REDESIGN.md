# Redesign — Sala de análise (Lovable) + relatório PDF (Figma)

**Estado: escrito por revisão estática. NÃO compilado nem testado.** O ambiente em que
estas mudanças foram feitas não tinha Rust/cargo, `node_modules` nem rede. Antes de usar:
`npm install && npm run check` (typecheck, lint, vitest, cargo test) e `npm run app:build`.

## Feito
- **Tokens/estilo**: paleta grafite + âmbar do template (`src/styles/app.css`, oklch → hex), tema escuro único, raios curtos, marca-texto âmbar. Fontes: só do sistema (IBM Plex entra se instalada; o template as buscava na rede, o que o app proíbe).
- **Marca**: símbolo "folha sob foco" do Lovable (`src/components/BrandMark.tsx`, `public/favicon.svg`, `favicon.ico`, PNGs em `public/`, ícones do Tauri em `src-tauri/icons/`, logo do PDF em `src-tauri/assets/logo-report-128.rgb`). O Figma trazia uma variante diferente do símbolo (nós e ligações); foi mantida **uma só** marca (a do Lovable) para não haver duas identidades.
- **Tela inicial** como workspace de projetos (novo/abrir/recentes), sem contadores fictícios; cabeçalho do projeto com marca, nome e pasta.
- **Aba institucional removida**: `AboutDialog` e o botão "Sobre". Formatos/limites foram para o menu Ajuda; a versão fica no rodapé da tela inicial. A página "Marca e design" do template não foi portada.
- **Panda**: GLB e `PandaViewer`/`pandaCycle` originais mantidos (idênticos aos do template); fallback agora usa a marca; luz de contorno âmbar.
- **Exportação por projeto**: novo comando `export_evidence_to_project` grava em `<projeto>/exports/` (destino definido no núcleo) e botão "Salvar em exports/ do projeto". O destino manual continua disponível.
- **PDF**: paleta (petróleo/ocre) e logo do Figma aplicadas no gerador Rust existente (somente constantes de cor + logo).

## Já existia (auditado, não alterado)
Isolamento por projeto: cada projeto tem pasta e banco próprios; todo comando exige `project_id` e passa por `current_for`; consultas SQL filtram `project_id`; exclusão preserva originais referenciados; panda com redução de movimento e tratamento de erro.

## NÃO feito
- Layout editorial do Figma no PDF (capa, resumo, índice em tabela, rodapé "NN / NN", encerramento técnico) e fontes Source Serif/Sans: exige reescrever `build_layout` e testes; sem compilador isso seria arriscado. O PDF segue com DejaVu e a estrutura anterior.
- Sidebar/rotas do template (mantidas as abas do app), abas de Exportações/estados vazios adicionais, ícones de janela além dos listados.
- Testes de isolamento novos em Rust, cenários 1–6 do pedido (exigem execução real), teste visual do panda e do PDF em leitor externo.
