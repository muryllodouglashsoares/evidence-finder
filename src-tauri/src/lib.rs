//! Núcleo local do Evidence Finder (Fases 1–5: fundação, documentos, extração, busca e
//! exportação de evidências).
//!
//! Tudo ocorre na máquina do usuário: sem rede, sem telemetria, sem serviços
//! externos. A interface só se comunica com este núcleo pelos comandos de
//! `commands`.

pub mod authorization;
pub mod commands;
pub mod db;
pub mod docformat;
pub mod embeddings;
pub mod docstore;
pub mod documents;
pub mod error;
pub mod export;
pub mod extraction;
pub mod extraction_store;
pub mod models;
pub mod ocr;
pub mod orientation;
pub mod pdf_extract;
pub mod pdf_visual;
pub mod pdf_report;
pub mod paths;
pub mod registry;
pub mod search;
pub mod semantic;
pub mod semantic_store;
pub mod service;
pub mod state;
pub mod txt_extract;
pub mod validation;
pub mod visual;

#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod perf_tests;
#[cfg(all(test, feature = "semantic-model"))]
mod real_model_tests;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        // O plugin é usado apenas pelo lado Rust (diálogo de pasta). Nenhuma
        // permissão do plugin é concedida ao frontend em capabilities/.
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            app.manage(state::AppState::new(&data_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::pick_directory,
            commands::create_project,
            commands::open_project,
            commands::list_recent_projects,
            commands::open_recent_project,
            commands::forget_recent_project,
            commands::preview_delete_project,
            commands::delete_project,
            commands::close_project,
            commands::list_documents,
            commands::pick_documents,
            commands::import_documents,
            commands::add_text_document,
            commands::verify_document,
            commands::remove_document,
            commands::get_ocr_status,
            commands::start_extraction,
            commands::get_extraction_progress,
            commands::search_segments,
            commands::get_semantic_status,
            commands::start_semantic_indexing,
            commands::cancel_semantic_indexing,
            commands::clear_semantic_index,
            commands::get_segment_context,
            commands::open_document_source,
            commands::prepare_export_review,
            commands::export_evidence,
            commands::export_evidence_to_project,
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Evidence Finder");
}
