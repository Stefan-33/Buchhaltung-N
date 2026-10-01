mod auth;
mod commands;
mod db;
mod geschaeft;
mod sicherung;
mod stunden;

use std::sync::Mutex;
use tauri::Manager;

pub struct AppZustand {
    pub db: Mutex<rusqlite::Connection>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let conn = db::verbinden().expect("Datenbank konnte nicht geoeffnet werden");

            // Kein Konto wird mehr automatisch mit einem Zufallspasswort
            // angelegt - die Oberflaeche fragt bei leerer Benutzertabelle
            // selbst nach (ist_ersteinrichtung), Papa vergibt sein
            // Passwort direkt beim ersten Start.

            // Archiv-Status beim Start einmal auffrischen, falls das
            // Programm laenger nicht offen war.
            let _ = geschaeft::archiv_aktualisieren(&conn);

            app.manage(AppZustand { db: Mutex::new(conn) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::anmelden,
            commands::ist_ersteinrichtung,
            commands::ersteinrichtung_abschliessen,
            commands::passwort_aendern,
            commands::zweites_konto_anlegen,
            commands::mitarbeiterin_anlegen,
            commands::kunden_suchen,
            commands::kunde_anlegen,
            commands::kunden_importieren,
            commands::kunde_holen,
            commands::kartensatz_setzen,
            commands::auftraege_von_kunde,
            commands::auftrag_anlegen,
            commands::monatsstatistik,
            commands::jetzt_sichern,
            commands::stunden_erfassen,
            commands::stunden_importieren,
            commands::eigene_stunden,
            commands::alle_stunden,
            commands::stunden_loeschen,
            commands::alle_benutzer,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
