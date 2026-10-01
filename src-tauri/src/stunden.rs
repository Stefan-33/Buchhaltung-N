// Arbeitsstunden pro Person - bewusst getrennt von Kunden/Auftraegen
// (geschaeft.rs), damit das Erfassen der eigenen Stunden nie mit der
// Kundenverwaltung vermischt wird. Rechnet keinen Lohn aus - Stefan hat
// dafuer schon sein eigenes Excel, das Programm liefert nur die rohen
// Stunden (als Liste hier in der Oberflaeche und als stunden.csv in der
// Sicherung, siehe sicherung.rs).

use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum StundenFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Bitte eine Stundenzahl groesser als 0 eingeben")]
    UngueltigeStunden,
    #[error("Dieser Eintrag gehoert nicht dir")]
    NichtEigenerEintrag,
}

#[derive(Debug, Serialize)]
pub struct StundenEintrag {
    pub id: i64,
    pub benutzer_id: i64,
    pub anzeigename: String,
    pub datum: String,
    pub stunden: f64,
    pub notiz: String,
}

fn zeile_zu_eintrag(z: &rusqlite::Row) -> rusqlite::Result<StundenEintrag> {
    Ok(StundenEintrag {
        id: z.get(0)?,
        benutzer_id: z.get(1)?,
        anzeigename: z.get(2)?,
        datum: z.get(3)?,
        stunden: z.get(4)?,
        notiz: z.get(5)?,
    })
}

const EINTRAG_SQL: &str = "
    SELECT a.id, a.benutzer_id, b.anzeigename, a.datum, a.stunden, a.notiz
    FROM arbeitsstunden a
    JOIN benutzer b ON b.id = a.benutzer_id
";

pub fn stunden_erfassen(
    conn: &Connection,
    benutzer_id: i64,
    datum: &str,
    stunden: f64,
    notiz: &str,
) -> Result<StundenEintrag, StundenFehler> {
    if !(stunden > 0.0) {
        return Err(StundenFehler::UngueltigeStunden);
    }
    conn.execute(
        "INSERT INTO arbeitsstunden (benutzer_id, datum, stunden, notiz) VALUES (?1, ?2, ?3, ?4)",
        params![benutzer_id, datum, stunden, notiz.trim()],
    )?;
    let id = conn.last_insert_rowid();
    let sql = format!("{EINTRAG_SQL} WHERE a.id = ?1");
    Ok(conn.query_row(&sql, [id], zeile_zu_eintrag)?)
}

/// Nur die eigenen Eintraege eines Monats - das ist die Ansicht der
/// Mitarbeiterin selbst.
pub fn eigene_stunden(
    conn: &Connection,
    benutzer_id: i64,
    jahr: i32,
    monat: u32,
) -> Result<Vec<StundenEintrag>, StundenFehler> {
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

    // Genau der Stefan-Wunsch: die Mitarbeiterin erfasst ihre Stunden fuer
    // sich selbst, sieht nur ihre eigenen - Papa sieht in der Uebersicht
    // alle zusammen, fuer die Abrechnung in seinem Excel.
    #[test]
    fn eigene_und_gemeinsame_sicht_trennen_korrekt() {
        let conn = test_db();
        stunden_erfassen(&conn, 2, "2026-10-05", 6.5, "").unwrap();
        stunden_erfassen(&conn, 2, "2026-10-06", 4.0, "Kurzer Tag").unwrap();
        stunden_erfassen(&conn, 1, "2026-10-05", 8.0, "").unwrap();
        // Anderer Monat - darf in der Oktober-Abfrage nicht auftauchen.
        stunden_erfassen(&conn, 2, "2026-11-01", 3.0, "").unwrap();

        let eigene = eigene_stunden(&conn, 2, 2026, 10).unwrap();
        assert_eq!(eigene.len(), 2);
        assert_eq!(eigene[0].datum, "2026-10-05");
        assert_eq!(eigene[1].notiz, "Kurzer Tag");

        let alle = alle_stunden(&conn, 2026, 10).unwrap();
        assert_eq!(alle.len(), 3, "Papas und der Mitarbeiterin Eintraege zusammen");
    }

    #[test]
    fn negative_oder_null_stunden_werden_abgelehnt() {
        let conn = test_db();
        assert!(matches!(
            stunden_erfassen(&conn, 2, "2026-10-05", 0.0, ""),
            Err(StundenFehler::UngueltigeStunden)
        ));
        assert!(matches!(
            stunden_erfassen(&conn, 2, "2026-10-05", -2.0, ""),
            Err(StundenFehler::UngueltigeStunden)
        ));
    }

    #[test]
    fn loeschen_funktioniert_nur_am_eigenen_eintrag() {
        let conn = test_db();
        let eintrag = stunden_erfassen(&conn, 2, "2026-10-05", 6.5, "").unwrap();

        // Papa (benutzer_id 1) darf den Eintrag der Mitarbeiterin nicht loeschen.
        assert!(matches!(
            stunden_loeschen(&conn, eintrag.id, 1),
            Err(StundenFehler::NichtEigenerEintrag)
        ));
        assert_eq!(eigene_stunden(&conn, 2, 2026, 10).unwrap().len(), 1);

        // Sie selbst darf es.
        stunden_loeschen(&conn, eintrag.id, 2).unwrap();
        assert_eq!(eigene_stunden(&conn, 2, 2026, 10).unwrap().len(), 0);
    }
}
