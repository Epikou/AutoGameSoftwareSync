// Heure de construction (ms UTC) : borne basse pour detecter une horloge de console manifestement fausse (pile a plat,
// date repartie des annees en arriere). Aucune decision de synchronisation n'en depend : affichage d'un avertissement.
fn main() {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    println!("cargo:rustc-env=AGSS_BUILD_MS={}", ms);
}
