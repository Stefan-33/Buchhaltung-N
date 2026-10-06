mod auftraege;
mod auth;
mod commands;
mod datei;
mod db;
mod einstellungen;
mod geschaeft;
mod mitarbeiter;
mod preisliste;
mod quittung;
mod sicherung;
mod stunden;
mod treuhand;

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
            commands::mitarbeiterin_profil_aktualisieren,
            commands::kunden_suchen,
            commands::kunde_anlegen,
            commands::kunden_importieren,
            commands::kunde_holen,
            commands::kartensatz_setzen,
            commands::kunde_aktualisieren,
            commands::kunde_loeschen,
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
            commands::einstellungen_lesen,
            commands::einstellungen_speichern,
            commands::datei_als_tabelle_lesen,
            commands::stunden_fuer_treuhand_exportieren,
            commands::ausgaben_kategorien,
            commands::ausgabe_erfassen,
            commands::ausgaben_importieren,
            commands::ausgaben_eines_jahres,
            commands::ausgabe_loeschen,
            commands::beleg_oeffnen,
            commands::treuhand_bericht_exportieren,
            commands::lohnabrechnung_exportieren,
            commands::lohn_jahresuebersicht,
            commands::lohn_jahresuebersicht_exportieren,
            commands::formulare_lesen,
            commands::formular_setzen,
            commands::quittung_als_pdf_oeffnen,
            commands::quittung_logo_setzen,
            commands::quittung_logo_entfernen,
            commands::kunde_nach_nummer,
            commands::auftrag_annehmen,
            commands::auftrag_abrechnen,
            commands::auftrag_status_setzen,
            commands::auftrag_bezahlt_markieren,
            commands::auftraege_liste,
            commands::uebersicht,
            commands::preisliste_lesen,
            commands::preis_eintrag_speichern,
            commands::preis_eintrag_aktiv_setzen,
            commands::preisliste_importieren,
            commands::beleg_muster_oeffnen,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
