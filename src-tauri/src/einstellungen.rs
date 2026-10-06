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
    #[error("Prozentsatz muss zwischen 0 und 100 liegen")]
    UngueltigerProzentsatz,
    #[error("Ungültige Farbe (erwartet z.B. #92D050)")]
    UngueltigeFarbe,
    #[error("Unbekannte Beleg-Vorlage oder unbekanntes Papierformat")]
    UngueltigeVorlage,
}

pub const VORLAGEN: &[&str] = &["klassisch", "schlicht"];
pub const FORMATE: &[&str] = &["A5", "A4"];

/// "#92D050" - genau 6 Hex-Ziffern, damit die Farbe gefahrlos ins
/// Beleg-HTML eingesetzt werden kann.
pub fn farbe_gueltig(farbe: &str) -> bool {
    farbe.len() == 7 && farbe.starts_with('#') && farbe[1..].chars().all(|c| c.is_ascii_hexdigit())
}

// serde(default): fehlt ein (neueres) Feld beim Speichern aus der
// Oberflaeche, gilt der Standardwert statt eines Fehlers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
    // Logo/Foto fuer den Quittungskopf - Pfad im Sicherungsordner (siehe
    // quittung::logo_setzen). None = kein Logo hinterlegt, dann erscheint
    // keine Bildzeile auf der Quittung.
    pub quittung_logo_pfad: Option<String>,
    // Beleg-Design (Einstellungen -> Beleg-Design): "klassisch" ist das
    // Layout von Stefans bisheriger Excel-Rechnung (Logo links, gruener
    // Balken, Tabelle mit Linien, "Besten Dank"), "schlicht" das einfache
    // Layout ohne Linien.
    pub geschaeft_email: String,
    pub beleg_vorlage: String,
    pub beleg_format: String,
    pub beleg_farbe: String,
    pub beleg_titel_zusatz: String,
    pub beleg_dank: String,
    // Nur auf noch offenen Rechnungen gedruckt (z.B. Zahlungsfrist, IBAN).
    pub beleg_zahlungshinweis: String,
    // Saetze fuer die automatische Lohnabrechnung einer Mitarbeiterin
    // (treuhand.rs), in Prozent - aendern sich gelegentlich von Jahr zu
    // Jahr, darum hier einstellbar statt im Code fest einprogrammiert.
    pub lohn_ferienzuschlag_satz: f64,
    pub lohn_ahv_satz: f64,
    pub lohn_alv_satz: f64,
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
            quittung_logo_pfad: None,
            geschaeft_email: String::new(),
            beleg_vorlage: "klassisch".into(),
            beleg_format: "A5".into(),
            beleg_farbe: "#92D050".into(),
            beleg_titel_zusatz: "für Aenderungen / Reparaturen".into(),
            beleg_dank: "Besten Dank".into(),
            beleg_zahlungshinweis: String::new(),
            // Stand 2024/2025 fuer den Kanton Schwyz, genau wie in Stefans
            // bisheriger Lohnabrechnung-Excel (KTV/NBU bewusst nicht
            // automatisiert - bei ihm bisher ohne Abzug).
            lohn_ferienzuschlag_satz: 8.33,
            lohn_ahv_satz: 5.3,
            lohn_alv_satz: 1.1,
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
        quittung_logo_pfad: {
            let p = lesen(conn, "quittung_logo_pfad", "")?;
            if p.is_empty() { None } else { Some(p) }
        },
        geschaeft_email: lesen(conn, "geschaeft_email", &d.geschaeft_email)?,
        beleg_vorlage: lesen(conn, "beleg_vorlage", &d.beleg_vorlage)?,
        beleg_format: lesen(conn, "beleg_format", &d.beleg_format)?,
        beleg_farbe: lesen(conn, "beleg_farbe", &d.beleg_farbe)?,
        beleg_titel_zusatz: lesen(conn, "beleg_titel_zusatz", &d.beleg_titel_zusatz)?,
        beleg_dank: lesen(conn, "beleg_dank", &d.beleg_dank)?,
        beleg_zahlungshinweis: lesen(conn, "beleg_zahlungshinweis", &d.beleg_zahlungshinweis)?,
        lohn_ferienzuschlag_satz: lesen(conn, "lohn_ferienzuschlag_satz", &d.lohn_ferienzuschlag_satz.to_string())?
            .parse()
            .unwrap_or(d.lohn_ferienzuschlag_satz),
        lohn_ahv_satz: lesen(conn, "lohn_ahv_satz", &d.lohn_ahv_satz.to_string())?.parse().unwrap_or(d.lohn_ahv_satz),
        lohn_alv_satz: lesen(conn, "lohn_alv_satz", &d.lohn_alv_satz.to_string())?.parse().unwrap_or(d.lohn_alv_satz),
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
    for satz in [e.kartensatz_a, e.kartensatz_b, e.lohn_ferienzuschlag_satz, e.lohn_ahv_satz, e.lohn_alv_satz] {
        if !(0.0..=100.0).contains(&satz) {
            return Err(EinstellungenFehler::UngueltigerProzentsatz);
        }
    }
    if !farbe_gueltig(&e.beleg_farbe) {
        return Err(EinstellungenFehler::UngueltigeFarbe);
    }
    if !VORLAGEN.contains(&e.beleg_vorlage.as_str()) || !FORMATE.contains(&e.beleg_format.as_str()) {
        return Err(EinstellungenFehler::UngueltigeVorlage);
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
    schreiben(conn, "quittung_logo_pfad", e.quittung_logo_pfad.as_deref().unwrap_or(""))?;
    schreiben(conn, "geschaeft_email", e.geschaeft_email.trim())?;
    schreiben(conn, "beleg_vorlage", &e.beleg_vorlage)?;
    schreiben(conn, "beleg_format", &e.beleg_format)?;
    schreiben(conn, "beleg_farbe", &e.beleg_farbe.to_uppercase())?;
    schreiben(conn, "beleg_titel_zusatz", e.beleg_titel_zusatz.trim())?;
    schreiben(conn, "beleg_dank", e.beleg_dank.trim())?;
    schreiben(conn, "beleg_zahlungshinweis", e.beleg_zahlungshinweis.trim())?;
    schreiben(conn, "lohn_ferienzuschlag_satz", &e.lohn_ferienzuschlag_satz.to_string())?;
    schreiben(conn, "lohn_ahv_satz", &e.lohn_ahv_satz.to_string())?;
    schreiben(conn, "lohn_alv_satz", &e.lohn_alv_satz.to_string())?;
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
            ..Einstellungen::default()
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
        assert!(matches!(einstellungen_speichern(&conn, &e), Err(EinstellungenFehler::UngueltigerProzentsatz)));

        let mut e2 = Einstellungen::default();
        e2.kartensatz_b = 150.0;
        assert!(matches!(einstellungen_speichern(&conn, &e2), Err(EinstellungenFehler::UngueltigerProzentsatz)));
    }

    #[test]
    fn beleg_design_wird_gespeichert_und_ungueltige_werte_abgelehnt() {
        let conn = test_db();
        let mut e = Einstellungen::default();
        assert_eq!(e.beleg_vorlage, "klassisch");
        e.beleg_vorlage = "schlicht".into();
        e.beleg_format = "A4".into();
        e.beleg_farbe = "#1c9b3b".into();
        e.beleg_zahlungshinweis = "Zahlbar innert 30 Tagen".into();
        einstellungen_speichern(&conn, &e).unwrap();
        let gelesen = einstellungen_lesen(&conn).unwrap();
        assert_eq!(gelesen.beleg_vorlage, "schlicht");
        assert_eq!(gelesen.beleg_format, "A4");
        assert_eq!(gelesen.beleg_farbe, "#1C9B3B");
        assert_eq!(gelesen.beleg_zahlungshinweis, "Zahlbar innert 30 Tagen");

        let mut falsch = Einstellungen::default();
        falsch.beleg_farbe = "red\"><script>".into();
        assert!(matches!(einstellungen_speichern(&conn, &falsch), Err(EinstellungenFehler::UngueltigeFarbe)));
        let mut falsch = Einstellungen::default();
        falsch.beleg_vorlage = "bunt".into();
        assert!(matches!(einstellungen_speichern(&conn, &falsch), Err(EinstellungenFehler::UngueltigeVorlage)));
    }

    #[test]
    fn lohn_saetze_werden_gespeichert_und_wieder_gelesen() {
        let conn = test_db();
        let mut e = Einstellungen::default();
        e.lohn_ferienzuschlag_satz = 10.6;
        e.lohn_ahv_satz = 5.3;
        e.lohn_alv_satz = 1.1;
        einstellungen_speichern(&conn, &e).unwrap();
        let gelesen = einstellungen_lesen(&conn).unwrap();
        assert_eq!(gelesen.lohn_ferienzuschlag_satz, 10.6);
    }
}
