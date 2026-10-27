//! Integre l'icone et les metadonnees Windows (nom du produit, description, version) a AutoGameSoftwareSync.exe.
//! Declare aussi `has_tray` : icone de zone de notification disponible (toujours sous Windows ; sous Linux avec la feature `tray`).
fn main() {
    println!("cargo:rerun-if-changed=media/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(has_tray)");
    let windows_target = std::env::var("CARGO_CFG_TARGET_OS").map(|o| o == "windows").unwrap_or(false);
    if windows_target || std::env::var_os("CARGO_FEATURE_TRAY").is_some() {
        println!("cargo:rustc-cfg=has_tray");
    }
    #[cfg(windows)]
    windows_resources(windows_target);
}

#[cfg(windows)]
fn windows_resources(windows_target: bool) {
    if windows_target {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("media/icon.ico");
        res.set("ProductName", "AutoGameSoftwareSync");
        res.set("FileDescription", "AutoGameSoftwareSync - game and software saves and settings synchronised between your PCs through a shared folder");
        res.set("OriginalFilename", "AutoGameSoftwareSync.exe");
        res.set("InternalName", "AutoGameSoftwareSync");
        res.set("LegalCopyright", "AutoGameSoftwareSync");
        if let Err(e) = res.compile() {
            // Pas de rc.exe (Windows SDK) : l'exe est produit sans icone integree plutot que d'echouer.
            println!("cargo:warning=ressources Windows non integrees : {}", e);
        }
    }
}
