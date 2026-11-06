fn main() {
    // The UI files are embedded into the exe by tauri::generate_context!, but tauri-build only declares tauri.conf.json and
    // capabilities\ as inputs: without this line an edit in ui\ leaves the old pages in the exe.
    println!("cargo:rerun-if-changed=ui");
    tauri_build::build()
}
