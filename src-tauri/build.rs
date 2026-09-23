fn main() {
    println!("cargo:rerun-if-env-changed=CODEX_CHAT_PANE_FRONTEND_BUILD");
    println!("cargo:rerun-if-changed=../dist");
    tauri_build::build()
}
