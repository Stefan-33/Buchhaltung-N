// Reiter "Mitarbeiter": alles, was Papa als Arbeitgeber fuer eine
// Mitarbeiterin eintragen und melden muss, an einem Ort. Die Stunden
// (stunden.rs), das Lohnprofil (auth.rs) und die monatliche Lohnabrechnung
// (treuhand.rs) gab es schon, hier kommen dazu:
// - Personalien (Geburtsdatum, Eintritt, Austritt) fuer Lohnausweis und
//   AHV-Anmeldung,
// - die Jahresuebersicht des Lohns: die Zahlen fuer Lohnausweis und
//   AHV-Lohnbescheinigung, ohne zwoelf Monatsabrechnungen zusammenzuzaehlen,
// - die Checkliste "Formulare" (was ist schon angemeldet/gemeldet).

use crate::sicherung::{csv_feld, schreiben, sicherungs_ordner};
use crate::treuhand::lohn_berechnen;
use chrono::NaiveDate;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum MitarbeiterFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Ungültiges Datum bei {0} - bitte als TT.MM.JJJJ wählen")]
    UngueltigesDatum(&'static str),
    #[error("Der Austritt liegt vor dem Eintritt")]
    AustrittVorEintritt,
    #[error("Für diese Person ist kein Stundenlohn hinterlegt (siehe Mitarbeiterin bearbeiten).")]
    KeinStundenlohn,
    #[error("Unbekanntes Formular")]
    UnbekanntesFormular,
    #[error("{0}")]
    Sonstiges(String),
}

/// Leer ist erlaubt (noch nicht bekannt), sonst muss es ein echtes Datum
/// "JJJJ-MM-TT" sein - so wie es das Datumsfeld der Oberflaeche liefert.
fn datum_pruefen(wert: &str, feld: &'static str) -> Result<String, MitarbeiterFehler> {
    let wert = wert.trim();
    if wert.is_empty() {
        return Ok(String::new());
    }
    NaiveDate::parse_from_str(wert, "%Y-%m-%d").map_err(|_| MitarbeiterFehler::UngueltigesDatum(feld))?;
    Ok(wert.to_string())
}

/// Prueft die Personalien vor dem Speichern und gibt sie bereinigt zurueck.
pub fn personalien_pruefen(geburtsdatum: &str, eintritt: &str, austritt: &str) -> Result<(String, String, String), MitarbeiterFehler> {
    let geburtsdatum = datum_pruefen(geburtsdatum, "Geburtsdatum")?;
    let eintritt = datum_pruefen(eintritt, "Eintritt")?;
    let austritt = datum_pruefen(austritt, "Austritt")?;
    // ISO-Daten lassen sich als Text vergleichen.
    if !eintritt.is_empty() && !austritt.is_empty() && austritt < eintritt {
        return Err(MitarbeiterFehler::AustrittVorEintritt);
    }
    Ok((geburtsdatum, eintritt, austritt))
}

pub fn personalien_setzen(conn: &Connection, benutzer_id: i64, geburtsdatum: &str, eintritt: &str, austritt: &str) -> Result<(), MitarbeiterFehler> {
    let (geburtsdatum, eintritt, austritt) = personalien_pruefen(geburtsdatum, eintritt, austritt)?;
    conn.execute(
        "UPDATE benutzer SET geburtsdatum = ?1, eintritt = ?2, austritt = ?3 WHERE id = ?4",
        params![geburtsdatum, eintritt, austritt, benutzer_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Jahresuebersicht Lohn
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct LohnMonat {
    pub monat: u32,
    pub stunden: f64,
    pub arbeitslohn: f64,
    pub ferienzuschlag: f64,
    pub bruttolohn: f64,
    pub ahv: f64,
    pub alv: f64,
    pub total_abzuege: f64,
    pub nettolohn: f64,
}

#[derive(Debug, Serialize)]
pub struct LohnJahr {
    pub jahr: i32,
    pub stundenlohn: f64,
    /// Nur Monate mit erfassten Stunden.
    pub monate: Vec<LohnMonat>,
    pub total: LohnMonat,
    /// Beschaeftigungszeitraum im Jahr fuer den Lohnausweis (Feld E),
    /// aus Eintritt/Austritt abgeleitet - sonst 1.1. bis 31.12.
    pub von: String,
    pub bis: String,
}

fn rappen(betrag: f64) -> f64 {
    (betrag * 100.0).round() / 100.0
}

/// Pro Monat genau dieselbe Rechnung wie die monatliche Lohnabrechnung
/// (treuhand::lohn_berechnen), jeder Betrag auf den Rappen gerundet wie auf
/// der Abrechnung - die Jahrestotale sind die Summe dieser gerundeten
/// Monatsbetraege und stimmen so mit den ausgezahlten Loehnen ueberein.
///
/// Gerechnet wird mit dem heute hinterlegten Stundenlohn und den heutigen
/// Prozentsaetzen - aendert sich der Lohn unter dem Jahr, gilt der neue
/// rueckwirkend auch fuer die frueheren Monate.
pub fn lohn_jahresuebersicht(conn: &Connection, benutzer_id: i64, jahr: i32) -> Result<LohnJahr, MitarbeiterFehler> {
    let person = crate::auth::benutzer_holen(conn, benutzer_id).map_err(|e| MitarbeiterFehler::Sonstiges(e.to_string()))?;
    let stundenlohn = person.stundenlohn.ok_or(MitarbeiterFehler::KeinStundenlohn)?;
    let e = crate::einstellungen::einstellungen_lesen(conn).map_err(|e| MitarbeiterFehler::Sonstiges(e.to_string()))?;

    let mut monate = Vec::new();
    for monat in 1..=12u32 {
        let eintraege = crate::stunden::eigene_stunden(conn, benutzer_id, jahr, monat).map_err(|e| MitarbeiterFehler::Sonstiges(e.to_string()))?;
        if eintraege.is_empty() {
            continue;
        }
        let stunden: f64 = eintraege.iter().map(|x| x.stunden).sum();
        let l = lohn_berechnen(stunden, stundenlohn, e.lohn_ferienzuschlag_satz, e.lohn_ahv_satz, e.lohn_alv_satz);
        let ahv = rappen(l.ahv);
        let alv = rappen(l.alv);
        let bruttolohn = rappen(l.bruttolohn);
        monate.push(LohnMonat {
            monat,
            stunden,
            arbeitslohn: rappen(l.arbeitslohn),
            ferienzuschlag: rappen(l.ferienzuschlag),
            bruttolohn,
            ahv,
            alv,
            total_abzuege: rappen(ahv + alv),
            nettolohn: rappen(bruttolohn - ahv - alv),
        });
    }

    let summe = |f: fn(&LohnMonat) -> f64| rappen(monate.iter().map(f).sum());
    let total = LohnMonat {
        monat: 0,
        stunden: monate.iter().map(|m| m.stunden).sum(),
        arbeitslohn: summe(|m| m.arbeitslohn),
        ferienzuschlag: summe(|m| m.ferienzuschlag),
        bruttolohn: summe(|m| m.bruttolohn),
        ahv: summe(|m| m.ahv),
        alv: summe(|m| m.alv),
        total_abzuege: summe(|m| m.total_abzuege),
        nettolohn: summe(|m| m.nettolohn),
    };

    let (von, bis) = zeitraum(jahr, &person.eintritt, &person.austritt);
    Ok(LohnJahr { jahr, stundenlohn, monate, total, von, bis })
}

/// Beschaeftigungszeitraum innerhalb eines Jahres: Eintritt/Austritt, falls
/// sie in dieses Jahr fallen, sonst Jahresanfang/-ende.
fn zeitraum(jahr: i32, eintritt: &str, austritt: &str) -> (String, String) {
    let anfang = format!("{jahr}-01-01");
    let ende = format!("{jahr}-12-31");
    let von = if !eintritt.is_empty() && eintritt > anfang.as_str() { eintritt.to_string() } else { anfang };
    let bis = if !austritt.is_empty() && austritt < ende.as_str() { austritt.to_string() } else { ende };
    (von, bis)
}

fn datum_ch(iso: &str) -> String {
    match NaiveDate::parse_from_str(iso, "%Y-%m-%d") {
        Ok(d) => d.format("%d.%m.%Y").to_string(),
        Err(_) => iso.to_string(),
    }
}

const MONATE: [&str; 12] = [
    "Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember",
];

/// Jahresuebersicht als CSV (oeffnet sich in Excel) - mit den Zahlen in der
/// Reihenfolge der Ziffern auf dem Lohnausweis (Formular 11).
pub fn lohn_jahresuebersicht_exportieren(conn: &Connection, benutzer_id: i64, jahr: i32) -> Result<PathBuf, MitarbeiterFehler> {
    let person = crate::auth::benutzer_holen(conn, benutzer_id).map_err(|e| MitarbeiterFehler::Sonstiges(e.to_string()))?;
    let j = lohn_jahresuebersicht(conn, benutzer_id, jahr)?;
    let e = crate::einstellungen::einstellungen_lesen(conn).map_err(|e| MitarbeiterFehler::Sonstiges(e.to_string()))?;

    let mut csv = String::new();
    csv.push_str(&format!("Lohn-Jahresübersicht,{jahr}\n\n"));
    csv.push_str("Arbeitgeber\n");
    csv.push_str(&format!("{}\n", csv_feld(&e.geschaeft_name)));
    csv.push_str(&format!("{}\n\n", csv_feld(&e.geschaeft_adresse)));
    csv.push_str("Arbeitnehmerin\n");
    csv.push_str(&format!("{}\n", csv_feld(&person.anzeigename)));
    csv.push_str(&format!("{}\n", csv_feld(&person.strasse)));
    csv.push_str(&format!("{}\n", csv_feld(&person.plz_ort)));
    csv.push_str(&format!("AHV-Nummer,{}\n", csv_feld(&person.ahv_nummer)));
    csv.push_str(&format!("Geburtsdatum,{}\n", csv_feld(&datum_ch(&person.geburtsdatum))));
    csv.push_str(&format!("Zeitraum,{} - {}\n", datum_ch(&j.von), datum_ch(&j.bis)));
    csv.push_str(&format!("Stundenlohn,{:.2}\n\n", j.stundenlohn));

    csv.push_str("Monat,Stunden,Arbeitslohn,Ferienzuschlag,Bruttolohn,AHV/IV/EO,ALV,Total Abzüge,Nettolohn\n");
    for m in &j.monate {
        csv.push_str(&format!(
            "{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}\n",
            MONATE[(m.monat - 1) as usize], m.stunden, m.arbeitslohn, m.ferienzuschlag, m.bruttolohn, m.ahv, m.alv, m.total_abzuege, m.nettolohn
        ));
    }
    let t = &j.total;
    csv.push_str(&format!(
        "Total,{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}\n\n",
        t.stunden, t.arbeitslohn, t.ferienzuschlag, t.bruttolohn, t.ahv, t.alv, t.total_abzuege, t.nettolohn
    ));

    csv.push_str("Für den Lohnausweis (Formular 11),Betrag\n");
    csv.push_str(&format!("Ziffer 1 - Lohn,{:.2}\n", t.bruttolohn));
    csv.push_str(&format!("Ziffer 8 - Bruttolohn total,{:.2}\n", t.bruttolohn));
    csv.push_str(&format!("Ziffer 9 - Beiträge AHV/IV/EO/ALV,{:.2}\n", t.total_abzuege));
    csv.push_str(&format!("Ziffer 11 - Nettolohn,{:.2}\n\n", t.nettolohn));
    csv.push_str("Für die AHV-Lohnbescheinigung,Betrag\n");
    csv.push_str(&format!("AHV-pflichtiger Lohn {jahr},{:.2}\n", t.bruttolohn));

    let ordner = sicherungs_ordner().join("Lohnabrechnungen");
    std::fs::create_dir_all(&ordner).map_err(|e| MitarbeiterFehler::Sonstiges(e.to_string()))?;
    let pfad = ordner.join(format!("Lohn-Jahresuebersicht_{}_{jahr}.csv", person.anzeigename.replace(' ', "_")));
    schreiben(&pfad, &csv).map_err(MitarbeiterFehler::Sonstiges)?;
    Ok(pfad)
}

// ---------------------------------------------------------------------------
// Checkliste Formulare
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, PartialEq)]
pub struct FormularStatus {
    pub formular: String,
    pub erledigt_am: String,
}

/// Schluessel aus der Oberflaeche: Buchstaben, Ziffern, "_", "-" und ":"
/// (jaehrliche Meldungen z.B. "lohnausweis:2026").
fn formular_gueltig(formular: &str) -> bool {
    !formular.is_empty()
        && formular.len() <= 60
        && formular.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == ':')
}

pub fn formulare_lesen(conn: &Connection, benutzer_id: i64) -> Result<Vec<FormularStatus>, MitarbeiterFehler> {
    let mut stmt = conn.prepare("SELECT formular, erledigt_am FROM mitarbeiter_formulare WHERE benutzer_id = ?1 ORDER BY formular")?;
    let zeilen = stmt
        .query_map([benutzer_id], |z| Ok(FormularStatus { formular: z.get(0)?, erledigt_am: z.get(1)? }))?
        .collect::<Result<_, _>>()?;
    Ok(zeilen)
}

/// Abhaken (mit heutigem Datum) oder wieder zuruecksetzen.
pub fn formular_setzen(conn: &Connection, benutzer_id: i64, formular: &str, erledigt: bool) -> Result<(), MitarbeiterFehler> {
    if !formular_gueltig(formular) {
        return Err(MitarbeiterFehler::UnbekanntesFormular);
    }
    if erledigt {
        conn.execute(
            "INSERT INTO mitarbeiter_formulare (benutzer_id, formular, erledigt_am) VALUES (?1, ?2, date('now','localtime'))
             ON CONFLICT(benutzer_id, formular) DO NOTHING",
            params![benutzer_id, formular],
        )?;
    } else {
        conn.execute("DELETE FROM mitarbeiter_formulare WHERE benutzer_id = ?1 AND formular = ?2", params![benutzer_id, formular])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{mitarbeiterin_anlegen, NeueMitarbeiterin};
    use crate::stunden::{stunden_erfassen, NeuerStundenEintrag};

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    fn mitarbeiterin(conn: &Connection, stundenlohn: Option<f64>) -> i64 {
        mitarbeiterin_anlegen(
            conn,
            &NeueMitarbeiterin {
                anzeigename: "Erika Muster".into(),
                strasse: "Dorfstrasse 1".into(),
                plz_ort: "8808 Pfäffikon".into(),
                ahv_nummer: "756.1234.5678.97".into(),
                stundenlohn,
                geburtsdatum: "1980-05-17".into(),
                eintritt: "2026-03-01".into(),
                austritt: "".into(),
            },
        )
        .unwrap()
        .id
    }

    fn stunden(conn: &Connection, id: i64, datum: &str, beginn: &str, ende: &str) {
        stunden_erfassen(
            conn,
            id,
            &NeuerStundenEintrag {
                datum: datum.into(),
                vm_beginn: beginn.into(),
                vm_ende: ende.into(),
                nm_beginn: String::new(),
                nm_ende: String::new(),
                notiz: String::new(),
            },
        )
        .unwrap();
    }

    #[test]
    fn personalien_werden_geprueft_und_gespeichert() {
        let conn = test_db();
        let id = mitarbeiterin(&conn, Some(25.0));
        let p = crate::auth::benutzer_holen(&conn, id).unwrap();
        assert_eq!(p.geburtsdatum, "1980-05-17");
        assert_eq!(p.eintritt, "2026-03-01");

        personalien_setzen(&conn, id, "1980-05-17", "2026-03-01", "2026-09-30").unwrap();
        assert_eq!(crate::auth::benutzer_holen(&conn, id).unwrap().austritt, "2026-09-30");

        assert!(matches!(personalien_setzen(&conn, id, "17.05.1980", "", ""), Err(MitarbeiterFehler::UngueltigesDatum("Geburtsdatum"))));
        assert!(matches!(personalien_setzen(&conn, id, "", "2026-02-30", ""), Err(MitarbeiterFehler::UngueltigesDatum("Eintritt"))));
        assert!(matches!(personalien_setzen(&conn, id, "", "2026-03-01", "2026-01-31"), Err(MitarbeiterFehler::AustrittVorEintritt)));
        // Leer bleibt erlaubt.
        personalien_setzen(&conn, id, "", "", "").unwrap();
    }

    // Zwei Monate mit Stunden, einer im Vorjahr: die Jahresuebersicht
    // zaehlt nur das gewaehlte Jahr und die Totale sind die Summe der auf
    // den Rappen gerundeten Monatsabrechnungen.
    #[test]
    fn jahresuebersicht_summiert_die_monatsabrechnungen_des_jahres() {
        let conn = test_db();
        let id = mitarbeiterin(&conn, Some(24.66));
        stunden(&conn, id, "2026-03-02", "08:00", "11:30"); // 3.5 Std.
        stunden(&conn, id, "2026-03-09", "08:00", "10:00"); // 2 Std.
        stunden(&conn, id, "2026-05-04", "13:30", "16:00"); // 2.5 Std.
        stunden(&conn, id, "2025-12-01", "08:00", "12:00"); // anderes Jahr

        let j = lohn_jahresuebersicht(&conn, id, 2026).unwrap();
        assert_eq!(j.monate.iter().map(|m| m.monat).collect::<Vec<_>>(), vec![3, 5]);
        assert_eq!(j.total.stunden, 8.0);

        let e = crate::einstellungen::einstellungen_lesen(&conn).unwrap();
        let maerz = lohn_berechnen(5.5, 24.66, e.lohn_ferienzuschlag_satz, e.lohn_ahv_satz, e.lohn_alv_satz);
        assert_eq!(j.monate[0].bruttolohn, rappen(maerz.bruttolohn));
        assert_eq!(j.total.bruttolohn, rappen(j.monate[0].bruttolohn + j.monate[1].bruttolohn));
        assert_eq!(j.total.nettolohn, rappen(j.total.bruttolohn - j.total.total_abzuege));
        assert_eq!((j.von.as_str(), j.bis.as_str()), ("2026-03-01", "2026-12-31"), "Eintritt im Maerz");
    }

    #[test]
    fn jahresuebersicht_braucht_einen_stundenlohn() {
        let conn = test_db();
        let id = mitarbeiterin(&conn, None);
        assert!(matches!(lohn_jahresuebersicht(&conn, id, 2026), Err(MitarbeiterFehler::KeinStundenlohn)));
    }

    #[test]
    fn zeitraum_beruecksichtigt_eintritt_und_austritt_nur_im_selben_jahr() {
        assert_eq!(zeitraum(2026, "", ""), ("2026-01-01".into(), "2026-12-31".into()));
        assert_eq!(zeitraum(2026, "2019-04-01", ""), ("2026-01-01".into(), "2026-12-31".into()));
        assert_eq!(zeitraum(2026, "2026-04-01", "2026-08-31"), ("2026-04-01".into(), "2026-08-31".into()));
        assert_eq!(zeitraum(2026, "", "2027-01-31"), ("2026-01-01".into(), "2026-12-31".into()));
    }

    #[test]
    fn formulare_abhaken_und_zuruecksetzen() {
        let conn = test_db();
        let id = mitarbeiterin(&conn, Some(25.0));
        formular_setzen(&conn, id, "ahv_anmeldung", true).unwrap();
        formular_setzen(&conn, id, "lohnausweis:2026", true).unwrap();
        // Zweimal abhaken aendert nichts und bricht nicht ab.
        formular_setzen(&conn, id, "ahv_anmeldung", true).unwrap();

        let liste = formulare_lesen(&conn, id).unwrap();
        assert_eq!(liste.iter().map(|f| f.formular.as_str()).collect::<Vec<_>>(), vec!["ahv_anmeldung", "lohnausweis:2026"]);
        assert!(liste.iter().all(|f| f.erledigt_am.len() == 10));

        formular_setzen(&conn, id, "ahv_anmeldung", false).unwrap();
        assert_eq!(formulare_lesen(&conn, id).unwrap().len(), 1);

        assert!(matches!(formular_setzen(&conn, id, "", true), Err(MitarbeiterFehler::UnbekanntesFormular)));
        assert!(matches!(formular_setzen(&conn, id, "x'; DROP TABLE benutzer", true), Err(MitarbeiterFehler::UnbekanntesFormular)));
    }
}
