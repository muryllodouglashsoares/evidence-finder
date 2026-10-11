// Evita abrir uma janela de console extra no Windows em builds de release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    evidence_finder_lib::run()
}
