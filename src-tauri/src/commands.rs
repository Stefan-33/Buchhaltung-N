// Die Bruecke zwischen JavaScript (Programmoberflaeche) und Rust
// (Datenbank/Logik). Jede Funktion hier mit #[tauri::command(rename_all = "snake_case")] kann das
// Frontend per invoke("funktionsname", { ... }) aufrufen.

use crate::auth::{self, Benutzer};
use crate::geschaeft::{self, Auftrag, Kunde, NeuerAuftrag, NeuerKunde};
use crate::sicherung;
use crate::AppZustand;
use tauri::State;

type Antwort<T> = Result<T, String>;

fn verbindung_sperren<'a>(zustand: &'a State<'a, AppZustand>) -> std::sync::MutexGuard<'a, rusqlite::Connection> {
    zustand.db.lock().expect("Datenbank-Sperre konnte nicht geholt werden")
}

#[tauri::command(rename_all = "snake_case")]
pub fn anmelden(zustand: State<AppZustand>, benutzername: String, passwort: String) -> Antwort<Benutzer> {
    let conn = verbindung_sperren(&zustand);
    auth::anmelden(&conn, &benutzername, &passwort).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn passwort_aendern(zustand: State<AppZustand>, benutzer_id: i64, neues_passwort: String) -> Antwort<()> {
    let conn = verbindung_sperren(&zustand);
    auth::passwort_aendern(&conn, benutzer_id, &neues_passwort).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn zweites_konto_anlegen(
    zustand: State<AppZustand>,
    benutzername: String,
    anzeigename: String,
    passwort: String,
) -> Antwort<()> {
    let conn = verbindung_sperren(&zustand);
    // Notfall-Zugang, z.B. fuer Mama - "inhaber", damit sie im Ernstfall
    // wirklich alles machen kann und nicht an kuenstlichen Grenzen haengt.
    auth::konto_anlegen(&conn, &benutzername, &anzeigename, &passwort, "inhaber").map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn kunden_suchen(zustand: State<AppZustand>, suchtext: String, archiv_zeigen: bool) -> Antwort<Vec<Kunde>> {
    let conn = verbindung_sperren(&zustand);
    geschaeft::kunden_suchen(&conn, &suchtext, archiv_zeigen).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn kunde_anlegen(zustand: State<AppZustand>, eingabe: NeuerKunde) -> Antwort<Kunde> {
    let conn = verbindung_sperren(&zustand);
    geschaeft::kunde_anlegen(&conn, eingabe).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn kunde_holen(zustand: State<AppZustand>, id: i64) -> Antwort<Kunde> {
    let conn = verbindung_sperren(&zustand);
    geschaeft::kunde_holen(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn kartensatz_setzen(zustand: State<AppZustand>, kunde_id: i64, kartensatz: Option<f64>) -> Antwort<()> {
    let conn = verbindung_sperren(&zustand);
    geschaeft::kartensatz_setzen(&conn, kunde_id, kartensatz).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn auftraege_von_kunde(zustand: State<AppZustand>, kunde_id: i64) -> Antwort<Vec<Auftrag>> {
    let conn = verbindung_sperren(&zustand);
    geschaeft::auftraege_von_kunde(&conn, kunde_id).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn auftrag_anlegen(zustand: State<AppZustand>, eingabe: NeuerAuftrag) -> Antwort<Auftrag> {
    let mut conn = verbindung_sperren(&zustand);
    geschaeft::auftrag_anlegen(&mut conn, eingabe).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn monatsstatistik(zustand: State<AppZustand>, jahr: i32) -> Antwort<Vec<geschaeft::MonatsZeile>> {
    let conn = verbindung_sperren(&zustand);
    geschaeft::monatsstatistik(&conn, jahr).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn jetzt_sichern(zustand: State<AppZustand>) -> Antwort<String> {
    let conn = verbindung_sperren(&zustand);
    sicherung::jetzt_sichern(&conn).map(|p| p.display().to_string())
}
