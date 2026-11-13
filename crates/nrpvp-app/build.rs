fn main() {
    // The UI files are embedded into the exe by tauri::generate_context!, but tauri-build only declares tauri.conf.json and
    // capabilities\ as inputs: without this line an edit in ui\ leaves the old pages in the exe.
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-env-changed=NRPVP_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=NRPVP_RELEASES_REPO");
    tauri_build::build()
}
