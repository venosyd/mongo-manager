// Embute o ícone e os metadados no executável do Windows.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("ProductName", "Mongo Manager")
            .set("FileDescription", "Mongo Manager")
            .set("CompanyName", "Venosyd")
            .set("LegalCopyright", "Copyright (c) 2026 Venosyd");
        res.compile().expect("falha ao compilar recursos do Windows");
    }
}
