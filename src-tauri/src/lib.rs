mod auth;
mod commands;
mod db;
mod geschaeft;
mod sicherung;

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

            // Beim allerersten Start: Papas Konto automatisch anlegen und
            // das Start-Passwort ins Log schreiben, damit er es sofort
            // sieht und beim ersten Login gleich aendern kann.
            if let Ok(Some((benutzername, start_passwort))) = auth::erstkonto_sicherstellen(&conn) {
                log::info!(
                    "Erstes Konto angelegt - Benutzername '{benutzername}', Start-Passwort '{start_passwort}'. \
                     Bitte beim ersten Login sofort ein eigenes Passwort setzen."
                );
            }

            // Archiv-Status beim Start einmal auffrischen, falls das
            // Programm laenger nicht offen war.
            let _ = geschaeft::archiv_aktualisieren(&conn);

            app.manage(AppZustand { db: Mutex::new(conn) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::anmelden,
            commands::passwort_aendern,
            commands::zweites_konto_anlegen,
            commands::kunden_suchen,
            commands::kunde_anlegen,
            commands::kunde_holen,
            commands::kartensatz_setzen,
            commands::auftraege_von_kunde,
            commands::auftrag_anlegen,
            commands::monatsstatistik,
            commands::jetzt_sichern,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
