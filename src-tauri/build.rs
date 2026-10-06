fn main() {
  // Kennung des Builds fuer das automatische Update (update.rs): die
  // GitHub-Actions-Pipeline setzt den Commit und das Datum, ein lokaler
  // Build heisst "entwicklung" und sucht nie nach Updates.
  for (name, standard) in [("ATELIERBUCH_BUILD", "entwicklung"), ("ATELIERBUCH_BUILD_DATUM", "")] {
    println!("cargo:rerun-if-env-changed={name}");
    let wert = std::env::var(name).unwrap_or_else(|_| standard.to_string());
    println!("cargo:rustc-env={name}={wert}");
  }
  tauri_build::build()
}
