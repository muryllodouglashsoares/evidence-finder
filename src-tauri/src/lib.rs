//! Núcleo local do Evidence Finder (Fase 1: fundação).
//!
//! Tudo ocorre na máquina do usuário: sem rede, sem telemetria, sem serviços
//! externos. A interface só se comunica com este núcleo pelos comandos de
//! `commands`.

pub mod authorization;
pub mod commands;
pub mod db;
pub mod error;
pub mod models;
pub mod paths;
pub mod registry;
pub mod service;
pub mod state;
pub mod validation;

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
        ])
        .run(tauri::generate_context!())
        .expect("falha ao iniciar o Evidence Finder");
}
