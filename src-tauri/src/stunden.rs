// Arbeitsstunden pro Person - bewusst getrennt von Kunden/Auftraegen
// (geschaeft.rs), damit das Erfassen der eigenen Stunden nie mit der
// Kundenverwaltung vermischt wird. Rechnet keinen Lohn aus - Stefan hat
// dafuer schon sein eigenes Excel, das Programm liefert nur die rohen
// Stunden (als Liste hier in der Oberflaeche und als stunden.csv in der
// Sicherung, siehe sicherung.rs).
//
// Erfassung wie in Stefans bisheriger Excel-Vorlage: pro Tag zwei
// Zeitbloecke (Vormittag und Nachmittag, je Beginn/Ende als "HH:MM"),
// die Stundenzahl wird daraus berechnet statt separat eingetippt. Ein
// Block darf komplett leer bleiben (z.B. nur nachmittags gearbeitet).

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum StundenFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Ungültige Uhrzeit - bitte im Format HH:MM eingeben")]
    UngueltigeZeit,
    #[error("Bei einem Zeitblock fehlt Beginn oder Ende")]
    UnvollstaendigerZeitblock,
    #[error("Bitte mindestens einen Zeitblock (Beginn und Ende) ausfüllen")]
    KeinZeitblock,
    #[error("Dieser Eintrag gehoert nicht dir")]
    NichtEigenerEintrag,
}

#[derive(Debug, Deserialize, Clone)]
pub struct NeuerStundenEintrag {
    pub datum: String,
    pub vm_beginn: String,
    pub vm_ende: String,
    pub nm_beginn: String,
    pub nm_ende: String,
    pub notiz: String,
}

#[derive(Debug, Serialize)]
pub struct StundenEintrag {
    pub id: i64,
    pub benutzer_id: i64,
    pub anzeigename: String,
    pub datum: String,
    pub vm_beginn: Option<String>,
    pub vm_ende: Option<String>,
    pub nm_beginn: Option<String>,
    pub nm_ende: Option<String>,
    pub notiz: String,
    pub stunden: f64,
}

/// "HH:MM" -> Minuten seit Mitternacht, oder None wenn kein gueltiges
/// Format (z.B. eine verirrte Kopfzeile aus einem Copy&Paste-Import).
fn minuten(zeit: &str) -> Option<i64> {
    let (h, m) = zeit.trim().split_once(':')?;
    let h: i64 = h.trim().parse().ok()?;
    let m: i64 = m.trim().parse().ok()?;
    if !(0..24).contains(&h) || !(0..60).contains(&m) {
        return None;
    }
    Some(h * 60 + m)
}

fn block_minuten(beginn: &Option<String>, ende: &Option<String>) -> i64 {
    match (beginn.as_deref(), ende.as_deref()) {
        (Some(b), Some(e)) => match (minuten(b), minuten(e)) {
            (Some(bm), Some(em)) if em > bm => em - bm,
            _ => 0,
        },
        _ => 0,
    }
}

/// Oeffentlich, weil sicherung.rs dieselbe Berechnung fuer die
/// stunden.csv-Spalte braucht - keine zweite Implementierung pflegen.
pub fn stunden_aus_bloecken(vm_beginn: &Option<String>, vm_ende: &Option<String>, nm_beginn: &Option<String>, nm_ende: &Option<String>) -> f64 {
    (block_minuten(vm_beginn, vm_ende) + block_minuten(nm_beginn, nm_ende)) as f64 / 60.0
}

/// Fuer die manuelle Erfassung (ein Eintrag): leer -> None, ein nicht
/// lesbares Format -> Fehler (soll dem Benutzer direkt auffallen).
fn normalisieren_streng(wert: &str) -> Result<Option<String>, StundenFehler> {
    let wert = wert.trim();
    if wert.is_empty() {
        return Ok(None);
    }
    if minuten(wert).is_some() {
        Ok(Some(wert.to_string()))
    } else {
        Err(StundenFehler::UngueltigeZeit)
    }
}

/// Fuer den Massen-Import aus Excel/LibreOffice: ein nicht lesbares Format
/// (z.B. eine Kopfzeile "Beginn") wird stillschweigend als "kein Wert"
/// behandelt statt den ganzen Import abzubrechen.
fn normalisieren_nachsichtig(wert: &str) -> Option<String> {
    let wert = wert.trim();
    if minuten(wert).is_some() {
        Some(wert.to_string())
    } else {
        None
    }
}

fn zeitbloecke_pruefen(vm_beginn: &Option<String>, vm_ende: &Option<String>, nm_beginn: &Option<String>, nm_ende: &Option<String>) -> Result<(), StundenFehler> {
    if vm_beginn.is_some() != vm_ende.is_some() || nm_beginn.is_some() != nm_ende.is_some() {
        return Err(StundenFehler::UnvollstaendigerZeitblock);
    }
    if vm_beginn.is_none() && nm_beginn.is_none() {
        return Err(StundenFehler::KeinZeitblock);
    }
    Ok(())
}

fn zeile_zu_eintrag(z: &rusqlite::Row) -> rusqlite::Result<StundenEintrag> {
    let vm_beginn: Option<String> = z.get(4)?;
    let vm_ende: Option<String> = z.get(5)?;
    let nm_beginn: Option<String> = z.get(6)?;
    let nm_ende: Option<String> = z.get(7)?;
    let stunden = stunden_aus_bloecken(&vm_beginn, &vm_ende, &nm_beginn, &nm_ende);
    Ok(StundenEintrag {
        id: z.get(0)?,
        benutzer_id: z.get(1)?,
        anzeigename: z.get(2)?,
        datum: z.get(3)?,
        vm_beginn,
        vm_ende,
        nm_beginn,
        nm_ende,
        notiz: z.get(8)?,
        stunden,
    })
}

const EINTRAG_SQL: &str = "
    SELECT a.id, a.benutzer_id, b.anzeigename, a.datum, a.vm_beginn, a.vm_ende, a.nm_beginn, a.nm_ende, a.notiz
    FROM arbeitsstunden a
    JOIN benutzer b ON b.id = a.benutzer_id
";

pub fn stunden_erfassen(conn: &Connection, benutzer_id: i64, eingabe: &NeuerStundenEintrag) -> Result<StundenEintrag, StundenFehler> {
    let vm_beginn = normalisieren_streng(&eingabe.vm_beginn)?;
    let vm_ende = normalisieren_streng(&eingabe.vm_ende)?;
    let nm_beginn = normalisieren_streng(&eingabe.nm_beginn)?;
    let nm_ende = normalisieren_streng(&eingabe.nm_ende)?;
    zeitbloecke_pruefen(&vm_beginn, &vm_ende, &nm_beginn, &nm_ende)?;

    conn.execute(
        "INSERT INTO arbeitsstunden (benutzer_id, datum, vm_beginn, vm_ende, nm_beginn, nm_ende, notiz)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![benutzer_id, eingabe.datum.trim(), vm_beginn, vm_ende, nm_beginn, nm_ende, eingabe.notiz.trim()],
    )?;
    let id = conn.last_insert_rowid();
    let sql = format!("{EINTRAG_SQL} WHERE a.id = ?1");
    Ok(conn.query_row(&sql, [id], zeile_zu_eintrag)?)
}

/// Massen-Import (Excel/LibreOffice-Copy&Paste, bereits in Zeilen zerlegt
/// von der Oberflaeche) fuer eine bestimmte Person - der Besitzer kann
/// damit auch die Stunden einer Mitarbeiterin nachtragen (z.B. aus einer
/// bisher separat gefuehrten Excel-Liste). Zeilen ohne gueltigen
/// Zeitblock werden uebersprungen statt den ganzen Import abzubrechen.
pub fn stunden_importieren(conn: &mut Connection, benutzer_id: i64, eingaben: Vec<NeuerStundenEintrag>) -> Result<usize, StundenFehler> {
    let tx = conn.transaction()?;
    let mut angelegt = 0usize;
    for eingabe in eingaben {
        let datum = eingabe.datum.trim();
        if datum.is_empty() {
            continue;
        }
        let vm_beginn = normalisieren_nachsichtig(&eingabe.vm_beginn);
        let vm_ende = normalisieren_nachsichtig(&eingabe.vm_ende);
        let nm_beginn = normalisieren_nachsichtig(&eingabe.nm_beginn);
        let nm_ende = normalisieren_nachsichtig(&eingabe.nm_ende);
        // Unvollstaendiger Block (nur Beginn oder nur Ende) -> diesen
        // Block verwerfen statt die ganze Zeile zu verwerfen.
        let vm_beginn = if vm_beginn.is_some() && vm_ende.is_some() { vm_beginn } else { None };
        let vm_ende = if vm_beginn.is_some() { vm_ende } else { None };
        let nm_beginn = if nm_beginn.is_some() && nm_ende.is_some() { nm_beginn } else { None };
        let nm_ende = if nm_beginn.is_some() { nm_ende } else { None };

        if stunden_aus_bloecken(&vm_beginn, &vm_ende, &nm_beginn, &nm_ende) <= 0.0 {
            continue;
        }

        tx.execute(
            "INSERT INTO arbeitsstunden (benutzer_id, datum, vm_beginn, vm_ende, nm_beginn, nm_ende, notiz)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![benutzer_id, datum, vm_beginn, vm_ende, nm_beginn, nm_ende, eingabe.notiz.trim()],
        )?;
        angelegt += 1;
    }
    tx.commit()?;
    Ok(angelegt)
}

/// Nur die eigenen Eintraege eines Monats - das ist die Ansicht der
/// Mitarbeiterin selbst.
pub fn eigene_stunden(conn: &Connection, benutzer_id: i64, jahr: i32, monat: u32) -> Result<Vec<StundenEintrag>, StundenFehler> {
    let monat_text = format!("{jahr:04}-{monat:02}");
    let sql = format!("{EINTRAG_SQL} WHERE a.benutzer_id = ?1 AND strftime('%Y-%m', a.datum) = ?2 ORDER BY a.datum, a.id");
    let mut stmt = conn.prepare(&sql)?;
    let zeilen = stmt
        .query_map(params![benutzer_id, monat_text], zeile_zu_eintrag)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

/// Alle Eintraege aller Personen eines Monats - die Uebersicht, die Papa/
/// Mama fuer die Lohnabrechnung brauchen.
pub fn alle_stunden(conn: &Connection, jahr: i32, monat: u32) -> Result<Vec<StundenEintrag>, StundenFehler> {
    let monat_text = format!("{jahr:04}-{monat:02}");
    let sql = format!("{EINTRAG_SQL} WHERE strftime('%Y-%m', a.datum) = ?1 ORDER BY b.anzeigename, a.datum, a.id");
    let mut stmt = conn.prepare(&sql)?;
    let zeilen = stmt.query_map([monat_text], zeile_zu_eintrag)?.collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

/// Loescht einen Eintrag - aber nur, wenn er wirklich der anfragenden
/// Person gehoert (z.B. ein Tippfehler-Eintrag von sich selbst). Verhindert,
/// dass jemand per erratener ID fremde Eintraege loescht.
pub fn stunden_loeschen(conn: &Connection, id: i64, benutzer_id: i64) -> Result<(), StundenFehler> {
    let betroffen = conn.execute(
        "DELETE FROM arbeitsstunden WHERE id = ?1 AND benutzer_id = ?2",
        params![id, benutzer_id],
    )?;
    if betroffen == 0 {
        return Err(StundenFehler::NichtEigenerEintrag);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn.execute(
            "INSERT INTO benutzer (id, benutzername, anzeigename, passwort_hash, rolle) VALUES
             (1, 'papa', 'Papa', 'x', 'inhaber'),
             (2, 'mitarbeiterin1', 'Mitarbeiterin 1', 'x', 'mitarbeiterin')",
            [],
        )
        .unwrap();
        conn
    }

    fn eintrag(datum: &str, vm_b: &str, vm_e: &str, nm_b: &str, nm_e: &str) -> NeuerStundenEintrag {
        NeuerStundenEintrag {
            datum: datum.into(),
            vm_beginn: vm_b.into(),
            vm_ende: vm_e.into(),
            nm_beginn: nm_b.into(),
            nm_ende: nm_e.into(),
            notiz: "".into(),
        }
    }

    // Genau Stefans bisheriges Excel-Muster: nur der Nachmittagsblock
    // ausgefuellt (13:30-16:00), Vormittag bleibt leer.
    #[test]
    fn nur_ein_zeitblock_wird_korrekt_berechnet() {
        let conn = test_db();
        let e = stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "", "", "13:30", "16:00")).unwrap();
        assert_eq!(e.stunden, 2.5);
        assert!(e.vm_beginn.is_none());
        assert_eq!(e.nm_beginn.as_deref(), Some("13:30"));
    }

    #[test]
    fn beide_zeitbloecke_werden_addiert() {
        let conn = test_db();
        let e = stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "08:30", "11:00", "13:30", "16:00")).unwrap();
        assert_eq!(e.stunden, 5.0);
    }

    #[test]
    fn unvollstaendiger_block_wird_abgelehnt() {
        let conn = test_db();
        assert!(matches!(
            stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "08:30", "", "", "")),
            Err(StundenFehler::UnvollstaendigerZeitblock)
        ));
    }

    #[test]
    fn kein_zeitblock_wird_abgelehnt() {
        let conn = test_db();
        assert!(matches!(
            stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "", "", "", "")),
            Err(StundenFehler::KeinZeitblock)
        ));
    }

    #[test]
    fn ungueltige_uhrzeit_wird_abgelehnt() {
        let conn = test_db();
        assert!(matches!(
            stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "", "", "nachmittags", "16:00")),
            Err(StundenFehler::UngueltigeZeit)
        ));
    }

    #[test]
    fn eigene_und_gemeinsame_sicht_trennen_korrekt() {
        let conn = test_db();
        stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "", "", "13:30", "16:00")).unwrap();
        stunden_erfassen(&conn, 2, &eintrag("2026-10-06", "", "", "13:30", "16:00")).unwrap();
        stunden_erfassen(&conn, 1, &eintrag("2026-10-05", "08:00", "12:00", "", "")).unwrap();
        // Anderer Monat - darf in der Oktober-Abfrage nicht auftauchen.
        stunden_erfassen(&conn, 2, &eintrag("2026-11-01", "", "", "13:30", "16:00")).unwrap();

        let eigene = eigene_stunden(&conn, 2, 2026, 10).unwrap();
        assert_eq!(eigene.len(), 2);

        let alle = alle_stunden(&conn, 2026, 10).unwrap();
        assert_eq!(alle.len(), 3, "Papas und der Mitarbeiterin Eintraege zusammen");
    }

    #[test]
    fn loeschen_funktioniert_nur_am_eigenen_eintrag() {
        let conn = test_db();
        let e = stunden_erfassen(&conn, 2, &eintrag("2026-10-05", "", "", "13:30", "16:00")).unwrap();

        assert!(matches!(stunden_loeschen(&conn, e.id, 1), Err(StundenFehler::NichtEigenerEintrag)));
        assert_eq!(eigene_stunden(&conn, 2, 2026, 10).unwrap().len(), 1);

        stunden_loeschen(&conn, e.id, 2).unwrap();
        assert_eq!(eigene_stunden(&conn, 2, 2026, 10).unwrap().len(), 0);
    }

    // Simuliert genau den Fall aus Stefans alter Excel-Liste: eine Kopfzeile
    // ("Datum"/"Beginn"/...) landet versehentlich mit im eingefuegten Text -
    // darf nicht als kaputter Eintrag durchrutschen, der ganze Rest muss
    // trotzdem sauber importiert werden.
    #[test]
    fn import_ueberspringt_kopfzeile_und_unvollstaendige_bloecke_ohne_abzubrechen() {
        let mut conn = test_db();
        let eingaben = vec![
            eintrag("Datum", "Beginn", "Ende", "Beginn", "Ende"), // Kopfzeile
            eintrag("2026-01-12", "", "", "13:30", "16:00"),
            eintrag("2026-01-13", "08:30", "", "", ""), // Ende fehlt -> dieser Block verworfen, Zeile ohne Zeit -> uebersprungen
            eintrag("2026-01-19", "", "", "13:30", "16:00"),
        ];
        let anzahl = stunden_importieren(&mut conn, 2, eingaben).unwrap();
        assert_eq!(anzahl, 2);
        assert_eq!(eigene_stunden(&conn, 2, 2026, 1).unwrap().len(), 2);
    }
}
