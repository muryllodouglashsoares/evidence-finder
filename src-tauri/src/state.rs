use crate::authorization::AuthorizedDirs;
use crate::registry::Registry;
use std::path::Path;

pub struct AppState {
    pub authorized: AuthorizedDirs,
    pub registry: Registry,
}

impl AppState {
    pub fn new(app_data_dir: &Path) -> Self {
        Self {
            authorized: AuthorizedDirs::default(),
            registry: Registry::new(app_data_dir.join("app-registry.db")),
        }
    }
}
