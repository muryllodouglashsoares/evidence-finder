//! Embeddings locais para a busca semântica (Fase 7).
//!
//! Tudo roda no próprio processo, em CPU, sem rede. O aplicativo NUNCA baixa
//! modelos: o usuário instala os arquivos do modelo em uma pasta conhecida
//! (instruções em `docs/FASE7.md` e no painel de busca) e o aplicativo apenas
//! os lê. Sem o modelo, a busca semântica informa que está indisponível e a
//! busca lexical continua completa.
//!
//! Modelo padrão: `intfloat/multilingual-e5-small` (multilíngue, inclui
//! português; 384 dimensões; licença MIT — confira a licença no repositório do
//! modelo antes de distribuí-lo). O E5 exige os prefixos `query: ` e
//! `passage: `, aplicados aqui.
//!
//! O texto indexado é sempre tratado como DADO: é passado ao tokenizador como
//! texto e nunca interpretado como instrução.

use crate::error::{AppError, ErrorCode};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Muda quando a forma de preparar o texto ou de gerar/normalizar vetores muda.
/// Vetores de outra versão são considerados desatualizados e refeitos.
pub const PIPELINE_VERSION: i64 = 1;
/// Máximo de caracteres de um trecho enviado ao modelo (o modelo também trunca
/// em 512 tokens). Trechos maiores são indexados só pelo início — limitação
/// documentada; a busca lexical continua cobrindo o trecho inteiro.
pub const MAX_EMBED_CHARS: usize = 2000;

pub const MODEL_ID: &str = "multilingual-e5-small";
pub const MODEL_DISPLAY_NAME: &str = "multilingual-e5-small (intfloat)";
pub const MODEL_SOURCE_URL: &str = "https://huggingface.co/intfloat/multilingual-e5-small";
pub const MODEL_LICENSE: &str = "MIT (verifique no repositório do modelo)";
/// Arquivos exigidos na pasta do modelo.
pub const MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];
/// Limiar INICIAL de similaridade (cosseno) abaixo do qual um trecho não é
/// considerado relacionado. Os cossenos do E5 ficam concentrados em faixa alta;
/// o valor precisa ser calibrado com os documentos reais (ver docs/FASE7.md).
pub const E5_MIN_SCORE: f32 = 0.80;

/// Contrato de um gerador de embeddings. Os vetores devolvidos têm norma 1, de
/// modo que o produto escalar é o cosseno.
pub trait Embedder: Send + Sync + fmt::Debug {
    fn model_id(&self) -> &str;
    /// Identifica o conteúdo exato do modelo (muda se os arquivos mudarem).
    fn model_version(&self) -> &str;
    fn dimension(&self) -> usize;
    /// Similaridade mínima para um trecho contar como relacionado.
    fn min_score(&self) -> f32;
    fn embed_passage(&self, text: &str) -> Result<Vec<f32>, AppError>;
    fn embed_query(&self, text: &str) -> Result<Vec<f32>, AppError>;
}

pub fn unavailable(message: impl Into<String>) -> AppError {
    AppError::new(ErrorCode::SemanticUnavailable, message)
}

/// Texto enviado ao modelo: espaços normalizados e limitado a `MAX_EMBED_CHARS`.
pub fn prepare_passage(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_EMBED_CHARS * 2));
    let mut count = 0usize;
    let mut pending_space = false;
    for c in text.chars() {
        if c.is_whitespace() || c.is_control() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= MAX_EMBED_CHARS {
            break;
        }
        out.push(c);
        count += 1;
    }
    out
}

/// Chave de conteúdo: SHA-256 do texto já preparado.
pub fn content_key(prepared: &str) -> String {
    Sha256::digest(prepared.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub fn vector_to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// `None` se o BLOB não tem o tamanho esperado (registro corrompido).
pub fn bytes_to_vector(bytes: &[u8], dim: usize) -> Option<Vec<f32>> {
    if dim == 0 || bytes.len() != dim * 4 {
        return None;
    }
    Some(
        bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect(),
    )
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ---------------------------------------------------------------------------
// Estado do recurso (para a interface)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelState {
    /// Arquivos presentes e modelo utilizável.
    Ready,
    /// Arquivos do modelo não instalados.
    Missing,
    /// Arquivos presentes, mas o modelo não pôde ser carregado.
    Invalid,
    /// Este executável foi compilado sem o motor de embeddings.
    UnsupportedBuild,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelStatus {
    pub state: ModelState,
    pub model_id: String,
    pub display_name: String,
    pub license: String,
    pub source_url: String,
    /// Pasta onde os arquivos devem ficar (apenas para exibição).
    pub directory: String,
    pub required_files: Vec<String>,
    pub missing_files: Vec<String>,
    pub message: Option<String>,
}

/// Identificação do conteúdo do modelo instalado, SEM carregá-lo: hash de
/// `config.json` e `tokenizer.json` mais o tamanho de `model.safetensors`.
pub fn fingerprint(dir: &Path) -> Result<String, AppError> {
    let read = |name: &str| std::fs::read(dir.join(name)).map_err(|_| unavailable("Arquivos do modelo ilegíveis."));
    let config = read("config.json")?;
    let tokenizer = read("tokenizer.json")?;
    let weights_len = std::fs::metadata(dir.join("model.safetensors"))
        .map_err(|_| unavailable("Arquivos do modelo ilegíveis."))?
        .len();
    let mut hasher = Sha256::new();
    hasher.update(&config);
    hasher.update(&tokenizer);
    hasher.update(weights_len.to_le_bytes());
    let short: String = hasher.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect();
    Ok(format!("{MODEL_ID}@{short}"))
}

pub fn model_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("models").join(MODEL_ID)
}

/// Manifesto (SHA-256 e tamanho de cada arquivo) que acompanha o modelo EMPACOTADO no
/// instalador. Gerado em `scripts/preparar-distribuicao.ps1`.
pub const MANIFEST_FILE: &str = "MODEL-MANIFEST.json";

/// Pasta do modelo incluído na instalação: `<pasta do executável>/models/<id>`.
pub fn bundled_model_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    Some(exe.parent()?.join("models").join(MODEL_ID))
}

fn has_all_files(dir: &Path) -> bool {
    MODEL_FILES.iter().all(|f| dir.join(f).is_file())
}

/// Escolhe onde ler o modelo. A cópia do usuário (`%APPDATA%`) tem precedência: se já está
/// completa, nada é usado do instalador nem baixado de novo. Sem ela, usa o modelo
/// empacotado, se completo. Sem nenhum dos dois, devolve a pasta do usuário, que é onde
/// `scripts/instalar-modelo.ps1` instala.
pub fn resolve_model_dir(app_data_dir: &Path, bundled: Option<PathBuf>) -> PathBuf {
    let user = model_dir(app_data_dir);
    if has_all_files(&user) {
        return user;
    }
    match bundled {
        Some(b) if has_all_files(&b) => b,
        _ => user,
    }
}

#[derive(serde::Deserialize)]
struct ManifestEntry {
    sha256: String,
    size: u64,
}

#[derive(serde::Deserialize)]
struct Manifest {
    files: std::collections::BTreeMap<String, ManifestEntry>,
}

fn sha256_of_file(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Confere os arquivos do modelo contra o manifesto empacotado. `Ok(false)` se a pasta não
/// tem manifesto (modelo instalado pelo script, já conferido no download); `Ok(true)` se
/// tinha e tudo confere; erro se algo estiver corrompido, alterado ou fora do manifesto.
/// Detecta corrupção e alteração acidental; não é assinatura digital.
pub fn verify_manifest(dir: &Path) -> Result<bool, AppError> {
    let path = dir.join(MANIFEST_FILE);
    if !path.is_file() {
        return Ok(false);
    }
    let bad = |m: &str| unavailable(format!("O modelo incluído na instalação está inválido: {m} Reinstale o aplicativo."));
    let text = std::fs::read_to_string(&path).map_err(|_| bad("manifesto ilegível."))?;
    let manifest: Manifest = serde_json::from_str(&text).map_err(|_| bad("manifesto malformado."))?;
    for name in MODEL_FILES {
        let entry = manifest.files.get(name).ok_or_else(|| bad(&format!("{name} não consta no manifesto.")))?;
        let file = dir.join(name);
        let size = std::fs::metadata(&file).map_err(|_| bad(&format!("{name} ausente.")))?.len();
        if size != entry.size {
            return Err(bad(&format!("{name} tem tamanho diferente do esperado.")));
        }
        let actual = sha256_of_file(&file).map_err(|_| bad(&format!("{name} ilegível.")))?;
        if !actual.eq_ignore_ascii_case(&entry.sha256) {
            return Err(bad(&format!("{name} falhou na conferência SHA-256.")));
        }
    }
    Ok(true)
}

/// Mantém o modelo carregado (carregar leva segundos e centenas de MB).
pub struct ModelManager {
    dir: PathBuf,
    cached: Mutex<Option<Arc<dyn Embedder>>>,
}

impl ModelManager {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, cached: Mutex::new(None) }
    }

    fn missing_files(&self) -> Vec<String> {
        MODEL_FILES
            .iter()
            .filter(|f| !self.dir.join(f).is_file())
            .map(|f| f.to_string())
            .collect()
    }

    /// Estado pelos arquivos e pela compilação (não consulta o cache).
    fn disk_state(&self) -> (ModelState, Option<String>, Vec<String>) {
        let missing = self.missing_files();
        if !cfg!(feature = "semantic-model") {
            return (
                ModelState::UnsupportedBuild,
                Some("Esta versão do aplicativo foi compilada sem o motor de busca semântica.".to_string()),
                missing,
            );
        }
        if !missing.is_empty() {
            return (
                ModelState::Missing,
                Some("O modelo de busca semântica não está instalado. A busca por palavras continua funcionando.".to_string()),
                missing,
            );
        }
        (ModelState::Ready, None, missing)
    }

    /// Diagnóstico barato: não carrega o modelo.
    pub fn status(&self) -> ModelStatus {
        let cached = self.cached.lock().map(|c| c.is_some()).unwrap_or(false);
        let (state, message, missing) = if cached {
            (ModelState::Ready, None, Vec::new())
        } else {
            self.disk_state()
        };
        ModelStatus {
            state,
            model_id: MODEL_ID.to_string(),
            display_name: MODEL_DISPLAY_NAME.to_string(),
            license: MODEL_LICENSE.to_string(),
            source_url: MODEL_SOURCE_URL.to_string(),
            directory: self.dir.to_string_lossy().into_owned(),
            required_files: MODEL_FILES.iter().map(|s| s.to_string()).collect(),
            missing_files: missing,
            message,
        }
    }

    /// Obtém o gerador (carregando-o na primeira vez). Nunca baixa nada.
    pub fn get(&self) -> Result<Arc<dyn Embedder>, AppError> {
        let mut cached = self.cached.lock().map_err(|_| AppError::internal())?;
        if let Some(e) = cached.as_ref() {
            return Ok(Arc::clone(e));
        }
        let (state, message, _) = self.disk_state();
        if state != ModelState::Ready {
            return Err(unavailable(
                message.unwrap_or_else(|| "Busca semântica indisponível.".to_string()),
            ));
        }
        // Modelo incluído no instalador: confere a integridade antes de carregar.
        verify_manifest(&self.dir)?;
        let loaded = load_local(&self.dir)?;
        *cached = Some(Arc::clone(&loaded));
        Ok(loaded)
    }

    /// Versão do modelo (instalado ou já carregado), sem carregá-lo. `None` se indisponível.
    pub fn version(&self) -> Option<String> {
        if let Ok(c) = self.cached.lock() {
            if let Some(e) = c.as_ref() {
                return Some(e.model_version().to_string());
            }
        }
        if self.disk_state().0 != ModelState::Ready {
            return None;
        }
        fingerprint(&self.dir).ok()
    }

    /// Descarta o modelo da memória (por exemplo, depois de uma falha).
    pub fn unload(&self) {
        if let Ok(mut c) = self.cached.lock() {
            *c = None;
        }
    }
}

#[cfg(feature = "semantic-model")]
pub(crate) fn load_local(dir: &Path) -> Result<Arc<dyn Embedder>, AppError> {
    let e = candle_backend::CandleEmbedder::load(dir)?;
    Ok(Arc::new(e))
}

#[cfg(not(feature = "semantic-model"))]
fn load_local(_dir: &Path) -> Result<Arc<dyn Embedder>, AppError> {
    Err(unavailable("Esta versão do aplicativo foi compilada sem o motor de busca semântica."))
}

// ---------------------------------------------------------------------------
// Backend local (candle, CPU)
// ---------------------------------------------------------------------------

#[cfg(feature = "semantic-model")]
mod candle_backend {
    use super::*;
    use candle_core::{DType, Device, Tensor};
    use candle_nn::VarBuilder;
    use candle_transformers::models::bert::{BertModel, Config, DTYPE};
    use tokenizers::{Tokenizer, TruncationParams};

    const MAX_TOKENS: usize = 512;

    pub struct CandleEmbedder {
        model: BertModel,
        tokenizer: Tokenizer,
        device: Device,
        version: String,
        dim: usize,
    }

    impl fmt::Debug for CandleEmbedder {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.debug_struct("CandleEmbedder").field("version", &self.version).finish()
        }
    }

    fn invalid(_detail: &str) -> AppError {
        AppError::new(
            ErrorCode::SemanticUnavailable,
            "Os arquivos do modelo de busca semântica não puderam ser carregados. Reinstale o modelo conforme as instruções.",
        )
    }

    fn failed() -> AppError {
        AppError::new(ErrorCode::ExtractionFailed, "Falha ao gerar a representação semântica do texto.")
    }

    impl CandleEmbedder {
        pub fn load(dir: &Path) -> Result<Self, AppError> {
            let config_bytes = std::fs::read(dir.join("config.json")).map_err(|_| invalid("config"))?;
            let config: Config = serde_json::from_slice(&config_bytes).map_err(|_| invalid("config"))?;
            let tokenizer_bytes = std::fs::read(dir.join("tokenizer.json")).map_err(|_| invalid("tokenizer"))?;
            let mut tokenizer = Tokenizer::from_bytes(&tokenizer_bytes).map_err(|_| invalid("tokenizer"))?;
            tokenizer
                .with_truncation(Some(TruncationParams { max_length: MAX_TOKENS, ..Default::default() }))
                .map_err(|_| invalid("truncation"))?;
            tokenizer.with_padding(None);

            let weights = dir.join("model.safetensors");
            let version = fingerprint(dir)?;
            let device = Device::Cpu;
            // SAFETY: o arquivo é aberto só para leitura e não é modificado pelo
            // aplicativo; é um arquivo instalado pelo próprio usuário.
            let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[weights], DTYPE, &device) }
                .map_err(|_| invalid("weights"))?;
            let dim = config.hidden_size;
            let model = BertModel::load(vb, &config).map_err(|_| invalid("model"))?;

            Ok(Self { model, tokenizer, device, version, dim })
        }

        fn embed(&self, text: &str) -> Result<Vec<f32>, AppError> {
            let encoding = self.tokenizer.encode(text, true).map_err(|_| failed())?;
            let ids = encoding.get_ids().to_vec();
            let n = ids.len();
            if n == 0 {
                return Err(failed());
            }
            let run = || -> candle_core::Result<Vec<f32>> {
                let input_ids = Tensor::new(ids.as_slice(), &self.device)?.unsqueeze(0)?;
                let type_ids = input_ids.zeros_like()?;
                let mask = Tensor::ones((1, n), DType::U32, &self.device)?;
                let hidden = self.model.forward(&input_ids, &type_ids, Some(&mask))?;
                // Média dos vetores dos tokens (todos válidos: sem preenchimento).
                let pooled = (hidden.sum(1)? / (n as f64))?;
                let norm = pooled.sqr()?.sum_keepdim(1)?.sqrt()?;
                pooled.broadcast_div(&norm)?.squeeze(0)?.to_vec1::<f32>()
            };
            let vector = run().map_err(|_| failed())?;
            if vector.len() != self.dim || vector.iter().any(|v| !v.is_finite()) {
                return Err(failed());
            }
            Ok(vector)
        }
    }

    impl Embedder for CandleEmbedder {
        fn model_id(&self) -> &str {
            MODEL_ID
        }
        fn model_version(&self) -> &str {
            &self.version
        }
        fn dimension(&self) -> usize {
            self.dim
        }
        fn min_score(&self) -> f32 {
            E5_MIN_SCORE
        }
        fn embed_passage(&self, text: &str) -> Result<Vec<f32>, AppError> {
            self.embed(&format!("passage: {text}"))
        }
        fn embed_query(&self, text: &str) -> Result<Vec<f32>, AppError> {
            self.embed(&format!("query: {text}"))
        }
    }
}

// ---------------------------------------------------------------------------
// Apoio para testes
// ---------------------------------------------------------------------------

/// Gerador DE TESTE: não é semântico. Associa palavras a "conceitos" definidos à
/// mão (grupos de sinônimos) e conta conceitos. Serve para validar o
/// ENCANAMENTO (armazenamento, invalidação, classificação, filtros, exportação)
/// de forma determinística; a qualidade semântica real só pode ser avaliada com
/// o modelo instalado.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    #[derive(Debug)]
    pub struct ConceptEmbedder {
        pub groups: Vec<Vec<&'static str>>,
        pub version: String,
        pub fail_on: Option<&'static str>,
    }

    impl ConceptEmbedder {
        pub fn new() -> Self {
            Self {
                groups: vec![
                    vec!["pagamento", "pagar", "pago", "transferencia", "deposito", "comprovante", "devolver", "dinheiro"],
                    vec!["confirmou", "confirmar", "confirmacao", "recebi", "recebimento", "recebido", "combinado"],
                    vec!["reuniao", "encontro", "agenda"],
                ],
                version: "teste@1".to_string(),
                fail_on: None,
            }
        }

        fn fold(word: &str) -> String {
            word.chars()
                .map(|c| match c {
                    'á' | 'à' | 'ã' | 'â' => 'a',
                    'é' | 'ê' => 'e',
                    'í' => 'i',
                    'ó' | 'õ' | 'ô' => 'o',
                    'ú' => 'u',
                    'ç' => 'c',
                    c => c,
                })
                .collect()
        }

        fn vector(&self, text: &str) -> Vec<f32> {
            let mut v = vec![0.0f32; self.groups.len() + 1];
            for raw in text.split(|c: char| !c.is_alphanumeric()) {
                let w = Self::fold(&raw.to_lowercase());
                if w.is_empty() {
                    continue;
                }
                match self.groups.iter().position(|g| g.contains(&w.as_str())) {
                    Some(i) => v[i] += 1.0,
                    None => v[self.groups.len()] += 0.05,
                }
            }
            let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            if norm == 0.0 {
                v[self.groups.len()] = 1.0;
                return v;
            }
            v.iter().map(|x| x / norm).collect()
        }
    }

    impl Embedder for ConceptEmbedder {
        fn model_id(&self) -> &str {
            "conceitos-de-teste"
        }
        fn model_version(&self) -> &str {
            &self.version
        }
        fn dimension(&self) -> usize {
            self.groups.len() + 1
        }
        fn min_score(&self) -> f32 {
            0.5
        }
        fn embed_passage(&self, text: &str) -> Result<Vec<f32>, AppError> {
            if self.fail_on.is_some_and(|w| text.contains(w)) {
                return Err(AppError::new(ErrorCode::ExtractionFailed, "falha simulada"));
            }
            Ok(self.vector(text))
        }
        fn embed_query(&self, text: &str) -> Result<Vec<f32>, AppError> {
            Ok(self.vector(text))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepare_collapses_whitespace_and_limits_length() {
        assert_eq!(prepare_passage("  olá \n\t mundo  "), "olá mundo");
        let long = "a ".repeat(MAX_EMBED_CHARS);
        assert!(prepare_passage(&long).chars().count() <= MAX_EMBED_CHARS);
    }

    #[test]
    fn content_key_is_stable_and_sensitive() {
        assert_eq!(content_key("a"), content_key("a"));
        assert_ne!(content_key("a"), content_key("b"));
        assert_eq!(content_key("a").len(), 64);
    }

    #[test]
    fn vector_roundtrip_and_rejects_wrong_size() {
        let v = vec![0.5f32, -1.0, 2.25];
        let bytes = vector_to_bytes(&v);
        assert_eq!(bytes_to_vector(&bytes, 3), Some(v));
        assert_eq!(bytes_to_vector(&bytes, 4), None);
        assert_eq!(bytes_to_vector(&bytes, 0), None);
    }

    #[test]
    fn missing_model_files_are_reported_without_loading() {
        let dir = tempfile::tempdir().unwrap();
        let manager = ModelManager::new(dir.path().join("models"));
        let status = manager.status();
        assert!(matches!(status.state, ModelState::Missing | ModelState::UnsupportedBuild));
        let err = manager.get().unwrap_err();
        assert_eq!(err.code, ErrorCode::SemanticUnavailable);
    }

    #[test]
    fn garbage_model_files_are_invalid_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        for f in MODEL_FILES {
            std::fs::write(dir.path().join(f), b"nao e um modelo").unwrap();
        }
        let manager = ModelManager::new(dir.path().to_path_buf());
        let err = manager.get().unwrap_err();
        assert_eq!(err.code, ErrorCode::SemanticUnavailable);
    }

    // --- modelo empacotado no instalador ----------------------------------------

    fn write_model(dir: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        for (i, f) in MODEL_FILES.iter().enumerate() {
            std::fs::write(dir.join(f), format!("conteudo {i} de {f}")).unwrap();
        }
    }

    fn write_manifest(dir: &Path) {
        let entries: Vec<String> = MODEL_FILES
            .iter()
            .map(|f| {
                let path = dir.join(f);
                format!(
                    "\"{f}\":{{\"sha256\":\"{}\",\"size\":{}}}",
                    sha256_of_file(&path).unwrap(),
                    std::fs::metadata(&path).unwrap().len()
                )
            })
            .collect();
        std::fs::write(dir.join(MANIFEST_FILE), format!("{{\"files\":{{{}}}}}", entries.join(","))).unwrap();
    }

    #[test]
    fn user_model_wins_then_bundled_then_user_dir_is_the_install_target() {
        let tmp = tempfile::tempdir().unwrap();
        let app = tmp.path().join("appdata");
        let bundled = tmp.path().join("install").join("models").join(MODEL_ID);

        // Nenhum dos dois: aponta para a pasta do usuário (onde o script instala).
        assert_eq!(resolve_model_dir(&app, Some(bundled.clone())), model_dir(&app));
        // Só o empacotado, completo: usa o empacotado (sem copiar nem baixar).
        write_model(&bundled);
        assert_eq!(resolve_model_dir(&app, Some(bundled.clone())), bundled);
        // O do usuário, completo, tem precedência.
        write_model(&model_dir(&app));
        assert_eq!(resolve_model_dir(&app, Some(bundled.clone())), model_dir(&app));
        // Empacotado incompleto é ignorado.
        std::fs::remove_dir_all(model_dir(&app)).unwrap();
        std::fs::remove_file(bundled.join("model.safetensors")).unwrap();
        assert_eq!(resolve_model_dir(&app, Some(bundled)), model_dir(&app));
    }

    #[test]
    fn manifest_accepts_intact_files_and_rejects_any_change() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("m");
        write_model(&dir);
        assert!(!verify_manifest(&dir).unwrap(), "sem manifesto: nada a conferir");
        write_manifest(&dir);
        assert!(verify_manifest(&dir).unwrap());

        // Mesmo tamanho, conteúdo diferente: só o SHA-256 pega.
        let target = dir.join("tokenizer.json");
        let mut bytes = std::fs::read(&target).unwrap();
        bytes[0] ^= 0x01;
        std::fs::write(&target, &bytes).unwrap();
        let err = verify_manifest(&dir).unwrap_err();
        assert!(err.message.contains("SHA-256"), "{}", err.message);

        // Tamanho diferente.
        std::fs::write(&target, b"curto").unwrap();
        assert!(verify_manifest(&dir).unwrap_err().message.contains("tamanho"));
    }

    #[test]
    fn manifest_that_omits_a_required_file_or_is_malformed_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("m");
        write_model(&dir);
        std::fs::write(dir.join(MANIFEST_FILE), "{\"files\":{}}").unwrap();
        assert!(verify_manifest(&dir).is_err(), "manifesto incompleto não vale");
        std::fs::write(dir.join(MANIFEST_FILE), "isto não é json").unwrap();
        assert!(verify_manifest(&dir).is_err());
    }

    #[test]
    fn corrupted_bundled_model_is_refused_before_loading() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("m");
        write_model(&dir);
        write_manifest(&dir);
        std::fs::write(dir.join("config.json"), b"adulterado").unwrap();
        let manager = ModelManager::new(dir);
        let err = manager.get().err().expect("deve recusar");
        assert!(
            err.message.contains("inválido") || err.message.contains("compilada"),
            "{}",
            err.message
        );
    }
}
