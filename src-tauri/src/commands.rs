// Die Bruecke zwischen JavaScript (Programmoberflaeche) und Rust
// (Datenbank/Logik). Jede Funktion hier mit #[tauri::command(rename_all = "snake_case")] kann das
// Frontend per invoke("funktionsname", { ... }) aufrufen.

use crate::auth::{self, Benutzer};
use crate::einstellungen::{self, Einstellungen};
use crate::geschaeft::{self, Auftrag, Kunde, NeuerAuftrag, NeuerKunde};
use crate::sicherung;
use crate::stunden::{self, StundenEintrag};
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
pub fn ist_ersteinrichtung(zustand: State<AppZustand>) -> Antwort<bool> {
    let conn = verbindung_sperren(&zustand);
    auth::ist_ersteinrichtung(&conn).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn ersteinrichtung_abschliessen(
    zustand: State<AppZustand>,
    benutzername: String,
    anzeigename: String,
    passwort: String,
) -> Antwort<Benutzer> {
    let conn = verbindung_sperren(&zustand);
    auth::ersteinrichtung_abschliessen(&conn, &benutzername, &anzeigename, &passwort).map_err(|e| e.to_string())
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
pub fn mitarbeiterin_anlegen(
    zustand: State<AppZustand>,
    benutzername: String,
    anzeigename: String,
    passwort: String,
) -> Antwort<()> {
    let conn = verbindung_sperren(&zustand);
    // Eigene Rolle "mitarbeiterin": loest in der Oberflaeche die
    // eingeschraenkte Ansicht aus (siehe rolleAnwenden() in app.js) -
    // kein Zugriff auf Monat & Jahr, Auswertung oder Sicherung, nur Kunden
    // suchen/anlegen/importieren und Auftraege buchen.
    auth::konto_anlegen(&conn, &benutzername, &anzeigename, &passwort, "mitarbeiterin").map_err(|e| e.to_string())
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
pub fn kunden_importieren(zustand: State<AppZustand>, eingaben: Vec<NeuerKunde>) -> Antwort<usize> {
    let mut conn = verbindung_sperren(&zustand);
    geschaeft::kunden_importieren(&mut conn, eingaben).map_err(|e| e.to_string())
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

#[tauri::command(rename_all = "snake_case")]
pub fn stunden_erfassen(zustand: State<AppZustand>, benutzer_id: i64, eingabe: stunden::NeuerStundenEintrag) -> Antwort<StundenEintrag> {
    let conn = verbindung_sperren(&zustand);
    stunden::stunden_erfassen(&conn, benutzer_id, &eingabe).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn stunden_importieren(
    zustand: State<AppZustand>,
    benutzer_id: i64,
    eingaben: Vec<stunden::NeuerStundenEintrag>,
) -> Antwort<usize> {
    let mut conn = verbindung_sperren(&zustand);
    stunden::stunden_importieren(&mut conn, benutzer_id, eingaben).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn eigene_stunden(zustand: State<AppZustand>, benutzer_id: i64, jahr: i32, monat: u32) -> Antwort<Vec<StundenEintrag>> {
    let conn = verbindung_sperren(&zustand);
    stunden::eigene_stunden(&conn, benutzer_id, jahr, monat).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn alle_benutzer(zustand: State<AppZustand>) -> Antwort<Vec<Benutzer>> {
    let conn = verbindung_sperren(&zustand);
    auth::alle_benutzer(&conn).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn einstellungen_lesen(zustand: State<AppZustand>) -> Antwort<Einstellungen> {
    let conn = verbindung_sperren(&zustand);
    einstellungen::einstellungen_lesen(&conn).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn einstellungen_speichern(zustand: State<AppZustand>, eingabe: Einstellungen) -> Antwort<()> {
    let conn = verbindung_sperren(&zustand);
    einstellungen::einstellungen_speichern(&conn, &eingabe).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn alle_stunden(zustand: State<AppZustand>, jahr: i32, monat: u32) -> Antwort<Vec<StundenEintrag>> {
    let conn = verbindung_sperren(&zustand);
    stunden::alle_stunden(&conn, jahr, monat).map_err(|e| e.to_string())
}

#[tauri::command(rename_all = "snake_case")]
pub fn stunden_loeschen(zustand: State<AppZustand>, id: i64, benutzer_id: i64) -> Antwort<()> {
    let conn = verbindung_sperren(&zustand);
    stunden::stunden_loeschen(&conn, id, benutzer_id).map_err(|e| e.to_string())
}
