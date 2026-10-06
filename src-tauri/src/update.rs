// Automatisches Update: Stefans Wunsch, nicht jedes Mal selbst die neue
// Installationsdatei herunterladen zu muessen. Jeder Build auf GitHub legt
// neben die Installationsdateien eine kleine "version.json" (Commit, Datum,
// Dateiname). Das Programm vergleicht beim Start seinen eigenen Commit
// damit; ist er anders, gibt es eine neue Version. "Jetzt aktualisieren"
// laedt die Installationsdatei in den Temp-Ordner, startet sie und beendet
// das Programm, damit der Installer die Dateien ersetzen kann. Die Daten
// (Datenbank in %APPDATA%) beruehrt das nicht.

use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;

const RELEASE_BASIS: &str = "https://github.com/Stefan-33/Buchhaltung-N/releases/download/neueste-version/";

/// Commit dieses Builds (siehe build.rs) - "entwicklung" bei lokalen Builds.
pub const BUILD: &str = env!("ATELIERBUCH_BUILD");
/// Datum dieses Builds als Text, z.B. "06.10.2026 12:30" (leer bei lokal).
pub const BUILD_DATUM: &str = env!("ATELIERBUCH_BUILD_DATUM");

/// Inhalt von version.json auf der Release-Seite.
#[derive(Debug, Deserialize, Clone)]
pub struct VersionDatei {
    pub build: String,
    pub datum: String,
    pub datei: String,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct UpdateInfo {
    /// true = es gibt eine neuere Version als die installierte.
    pub verfuegbar: bool,
    /// Lokaler Entwicklungs-Build: sucht nie nach Updates.
    pub entwicklung: bool,
    pub installiert_datum: String,
    pub neu_datum: String,
}

/// Reine Vergleichslogik, ohne Netz - testbar.
fn info_bauen(eigener_build: &str, eigenes_datum: &str, neu: Option<&VersionDatei>) -> UpdateInfo {
    let entwicklung = eigener_build == "entwicklung" || eigener_build.trim().is_empty();
    let (verfuegbar, neu_datum) = match neu {
        Some(v) if !entwicklung => (!v.build.trim().is_empty() && v.build.trim() != eigener_build.trim(), v.datum.clone()),
        Some(v) => (false, v.datum.clone()),
        None => (false, String::new()),
    };
    UpdateInfo { verfuegbar, entwicklung, installiert_datum: eigenes_datum.to_string(), neu_datum }
}

/// Nur ein einfacher Dateiname einer .exe - verhindert, dass eine
/// manipulierte version.json einen Pfad wie "..\\..\\x" unterjubelt.
fn dateiname_gueltig(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 120
        && name.to_ascii_lowercase().ends_with(".exe")
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

fn version_holen() -> Result<VersionDatei, String> {
    let antwort = ureq::get(&format!("{RELEASE_BASIS}version.json"))
        .timeout(std::time::Duration::from_secs(15))
        .call()
        .map_err(|e| format!("Keine Verbindung zur Update-Seite ({e})."))?;
    let text = antwort.into_string().map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|_| "Die Update-Angaben konnten nicht gelesen werden.".to_string())
}

pub fn update_pruefen() -> Result<UpdateInfo, String> {
    if info_bauen(BUILD, BUILD_DATUM, None).entwicklung {
        return Ok(info_bauen(BUILD, BUILD_DATUM, None));
    }
    let neu = version_holen()?;
    Ok(info_bauen(BUILD, BUILD_DATUM, Some(&neu)))
}

/// Laedt die neue Installationsdatei in den Temp-Ordner und gibt den Pfad
/// zurueck. Starten (und das Programm beenden) macht commands.rs.
pub fn update_herunterladen() -> Result<PathBuf, String> {
    let neu = version_holen()?;
    if !dateiname_gueltig(&neu.datei) {
        return Err("Die Update-Angaben sind ungültig.".to_string());
    }
    let antwort = ureq::get(&format!("{RELEASE_BASIS}{}", neu.datei))
        .timeout(std::time::Duration::from_secs(600))
        .call()
        .map_err(|e| format!("Die neue Version konnte nicht heruntergeladen werden ({e})."))?;
    let mut bytes = Vec::new();
    antwort
        .into_reader()
        .take(500 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Download abgebrochen ({e})."))?;
    // Eine Windows-Programmdatei beginnt immer mit "MZ" - sonst ist etwas
    // anderes angekommen (z.B. eine Fehlerseite).
    if bytes.len() < 1024 || !bytes.starts_with(b"MZ") {
        return Err("Die heruntergeladene Datei ist keine Installationsdatei.".to_string());
    }
    let pfad = std::env::temp_dir().join(&neu.datei);
    std::fs::write(&pfad, bytes).map_err(|e| format!("Datei konnte nicht gespeichert werden ({e})."))?;
    Ok(pfad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(build: &str) -> VersionDatei {
        VersionDatei { build: build.into(), datum: "06.10.2026 12:30".into(), datei: "Atelierbuch.Straub_0.1.0_x64-setup.exe".into() }
    }

    #[test]
    fn anderer_commit_auf_der_release_seite_ist_ein_update() {
        let info = info_bauen("abc123", "01.10.2026 08:00", Some(&version("def456")));
        assert!(info.verfuegbar && !info.entwicklung);
        assert_eq!(info.neu_datum, "06.10.2026 12:30");
        assert!(!info_bauen("abc123", "", Some(&version("abc123"))).verfuegbar, "gleicher Commit = aktuell");
        assert!(!info_bauen("abc123", "", Some(&version(" "))).verfuegbar, "leere Angabe = kein Update");
    }

    #[test]
    fn lokaler_entwicklungs_build_meldet_nie_ein_update() {
        let info = info_bauen("entwicklung", "", Some(&version("def456")));
        assert!(info.entwicklung && !info.verfuegbar);
    }

    #[test]
    fn nur_einfache_exe_dateinamen_werden_heruntergeladen() {
        assert!(dateiname_gueltig("Atelierbuch.Straub_0.1.0_x64-setup.exe"));
        assert!(!dateiname_gueltig("..\\..\\Windows\\x.exe"));
        assert!(!dateiname_gueltig("setup.msi"));
        assert!(!dateiname_gueltig(""));
    }
}
