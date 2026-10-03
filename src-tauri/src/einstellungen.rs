// Einstellungen, die Stefan selbst anpassen koennen soll, ohne dass dafuer
// Code geaendert und neu gebaut werden muss: Geschaeftsangaben (erscheinen
// auf der Quittung) und die beiden Kartengebuehr-Saetze. Liegen als
// Schluessel/Wert-Paare in der schon vorhandenen "einstellungen"-Tabelle
// (dieselbe, in der auch die laufenden Nummern stehen).
//
// Bewusst NICHT hier drin: die Hell/Dunkel-Darstellung - die ist eine reine
// Geraete-Einstellung der Person, die gerade am PC sitzt, nicht etwas, das
// fuer den ganzen Betrieb gilt. Die bleibt komplett im Frontend
// (localStorage), siehe app.js.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum EinstellungenFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Der Geschäftsname darf nicht leer sein")]
    NameLeer,
    #[error("Kartengebühr-Satz muss zwischen 0 und 100 liegen")]
    UngueltigerKartensatz,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Einstellungen {
    pub geschaeft_name: String,
    pub geschaeft_zeile2: String,
    pub geschaeft_adresse: String,
    pub geschaeft_telefon: String,
    pub geschaeft_web: String,
    pub kartensatz_a: f64,
    pub kartensatz_b: f64,
    // Die beiden Kleingedruckt-Zeilen ganz unten auf der Quittung - auch
    // Stefans eigener Wunsch, "die Quittungen-Darstellung selbst ändern
    // zu können".
    pub quittung_hinweis1: String,
    pub quittung_hinweis2: String,
}

impl Default for Einstellungen {
    fn default() -> Self {
        // Exakt die Werte, die bisher im Code fix einprogrammiert waren -
        // damit sich beim allerersten Start (noch keine Zeile in der
        // Tabelle) nichts sichtbar aendert.
        Einstellungen {
            geschaeft_name: "Nähservice Straub".into(),
            geschaeft_zeile2: "Änderungen und Reparaturen · Rosmarie Straub".into(),
            geschaeft_adresse: "Staldenbachstrasse 13, 8808 Pfäffikon SZ".into(),
            geschaeft_telefon: "055 410 72 06".into(),
            geschaeft_web: "naehservicestraub.ch".into(),
            kartensatz_a: 1.5,
            kartensatz_b: 2.5,
            quittung_hinweis1: "Reklamationen innert 10 Tagen nach Abholung".into(),
            quittung_hinweis2: "Kundenexemplar · Kartensatz und Gebühr erscheinen hier nie.".into(),
        }
    }
}

fn lesen(conn: &Connection, schluessel: &str, standard: &str) -> rusqlite::Result<String> {
    let wert: Option<String> = conn
        .query_row("SELECT wert FROM einstellungen WHERE schluessel = ?1", [schluessel], |z| z.get(0))
        .optional()?;
    Ok(wert.unwrap_or_else(|| standard.to_string()))
}

pub fn einstellungen_lesen(conn: &Connection) -> Result<Einstellungen, EinstellungenFehler> {
    let d = Einstellungen::default();
    Ok(Einstellungen {
        geschaeft_name: lesen(conn, "geschaeft_name", &d.geschaeft_name)?,
        geschaeft_zeile2: lesen(conn, "geschaeft_zeile2", &d.geschaeft_zeile2)?,
        geschaeft_adresse: lesen(conn, "geschaeft_adresse", &d.geschaeft_adresse)?,
        geschaeft_telefon: lesen(conn, "geschaeft_telefon", &d.geschaeft_telefon)?,
        geschaeft_web: lesen(conn, "geschaeft_web", &d.geschaeft_web)?,
        kartensatz_a: lesen(conn, "kartensatz_a", &d.kartensatz_a.to_string())?.parse().unwrap_or(d.kartensatz_a),
        kartensatz_b: lesen(conn, "kartensatz_b", &d.kartensatz_b.to_string())?.parse().unwrap_or(d.kartensatz_b),
        quittung_hinweis1: lesen(conn, "quittung_hinweis1", &d.quittung_hinweis1)?,
        quittung_hinweis2: lesen(conn, "quittung_hinweis2", &d.quittung_hinweis2)?,
    })
}

fn schreiben(conn: &Connection, schluessel: &str, wert: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO einstellungen (schluessel, wert) VALUES (?1, ?2)
         ON CONFLICT(schluessel) DO UPDATE SET wert = excluded.wert",
        params![schluessel, wert],
    )?;
    Ok(())
}

pub fn einstellungen_speichern(conn: &Connection, e: &Einstellungen) -> Result<(), EinstellungenFehler> {
    if e.geschaeft_name.trim().is_empty() {
        return Err(EinstellungenFehler::NameLeer);
    }
    for satz in [e.kartensatz_a, e.kartensatz_b] {
        if !(0.0..=100.0).contains(&satz) {
            return Err(EinstellungenFehler::UngueltigerKartensatz);
        }
    }

    schreiben(conn, "geschaeft_name", e.geschaeft_name.trim())?;
    schreiben(conn, "geschaeft_zeile2", e.geschaeft_zeile2.trim())?;
    schreiben(conn, "geschaeft_adresse", e.geschaeft_adresse.trim())?;
    schreiben(conn, "geschaeft_telefon", e.geschaeft_telefon.trim())?;
    schreiben(conn, "geschaeft_web", e.geschaeft_web.trim())?;
    schreiben(conn, "kartensatz_a", &e.kartensatz_a.to_string())?;
    schreiben(conn, "kartensatz_b", &e.kartensatz_b.to_string())?;
    schreiben(conn, "quittung_hinweis1", e.quittung_hinweis1.trim())?;
    schreiben(conn, "quittung_hinweis2", e.quittung_hinweis2.trim())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    #[test]
    fn ohne_gespeicherte_werte_kommen_die_bisherigen_fest_einprogrammierten_zurueck() {
        let conn = test_db();
        let e = einstellungen_lesen(&conn).unwrap();
        assert_eq!(e.geschaeft_name, "Nähservice Straub");
        assert_eq!(e.kartensatz_a, 1.5);
        assert_eq!(e.kartensatz_b, 2.5);
    }

    #[test]
    fn gespeicherte_werte_werden_korrekt_wieder_gelesen() {
        let conn = test_db();
        let neu = Einstellungen {
            geschaeft_name: "Neuer Name".into(),
            geschaeft_zeile2: "Zeile 2".into(),
            geschaeft_adresse: "Irgendwo 1".into(),
            geschaeft_telefon: "000".into(),
            geschaeft_web: "example.ch".into(),
            kartensatz_a: 1.8,
            kartensatz_b: 2.9,
            quittung_hinweis1: "Hinweis 1".into(),
            quittung_hinweis2: "Hinweis 2".into(),
        };
        einstellungen_speichern(&conn, &neu).unwrap();
        let gelesen = einstellungen_lesen(&conn).unwrap();
        assert_eq!(gelesen.geschaeft_name, "Neuer Name");
        assert_eq!(gelesen.kartensatz_a, 1.8);
        assert_eq!(gelesen.kartensatz_b, 2.9);
        assert_eq!(gelesen.quittung_hinweis1, "Hinweis 1");
    }

    #[test]
    fn leerer_name_wird_abgelehnt() {
        let conn = test_db();
        let mut e = Einstellungen::default();
        e.geschaeft_name = "   ".into();
        assert!(matches!(einstellungen_speichern(&conn, &e), Err(EinstellungenFehler::NameLeer)));
    }

    #[test]
    fn kartensatz_ausserhalb_0_bis_100_wird_abgelehnt() {
        let conn = test_db();
        let mut e = Einstellungen::default();
        e.kartensatz_a = -1.0;
        assert!(matches!(einstellungen_speichern(&conn, &e), Err(EinstellungenFehler::UngueltigerKartensatz)));

        let mut e2 = Einstellungen::default();
        e2.kartensatz_b = 150.0;
        assert!(matches!(einstellungen_speichern(&conn, &e2), Err(EinstellungenFehler::UngueltigerKartensatz)));
    }
}
