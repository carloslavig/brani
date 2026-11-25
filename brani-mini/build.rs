fn main() {
    // a pagina e uma so (src/miniatura.html, tambem servida em /miniatura): copia para a pasta que o Tauri embute
    let _ = std::fs::create_dir_all("ui");
    let _ = std::fs::copy("../src/miniatura.html", "ui/index.html");
    println!("cargo:rerun-if-changed=../src/miniatura.html");
    tauri_build::build()
}
