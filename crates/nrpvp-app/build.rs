use std::path::PathBuf;

fn main() {
    // The UI files are embedded into the exe by tauri::generate_context!, but tauri-build only declares tauri.conf.json and
    // capabilities\ as inputs: without this line an edit in ui\ leaves the old pages in the exe.
    println!("cargo:rerun-if-changed=ui");
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-env-changed=NRPVP_BUILD_VERSION");
    println!("cargo:rerun-if-env-changed=NRPVP_RELEASES_REPO");
    println!("cargo:rerun-if-env-changed=NRPVP_RELEASES_TOKEN");
    embed_releases_token();
    tauri_build::build()
}

/// Writes `$OUT_DIR/releases_token.rs`: the read-only token for the releases repository (CI secret `RELEASES_READ_TOKEN`,
/// passed as `NRPVP_RELEASES_TOKEN`), XOR-masked so the plain `github_pat_…` text never appears in the exe. The mask only keeps
/// secret scanners and `strings` from flagging it; anyone with the exe can still recover the token, which is why it must be a
/// fine-grained token limited to Contents: read-only on the releases repository and nothing else. Empty when unset (local builds).
fn embed_releases_token() {
    let token = std::env::var("NRPVP_RELEASES_TOKEN").unwrap_or_default();
    let token = token.trim();
    let masked: Vec<String> = token
        .bytes()
        .enumerate()
        .map(|(i, b)| (b ^ mask(i)).to_string())
        .collect();
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("releases_token.rs");
    std::fs::write(
        out,
        format!(
            "pub(crate) const MASKED: &[u8] = &[{}];\n",
            masked.join(",")
        ),
    )
    .unwrap();
}

/// Must match `release::mask`.
fn mask(i: usize) -> u8 {
    (i as u8).wrapping_mul(31).wrapping_add(0x5A)
}
