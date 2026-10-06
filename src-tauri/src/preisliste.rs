// Preisliste (Standardarbeiten): startet leer und wird per Excel-/CSV-
// Import oder einzeln befuellt. Beim Erfassen eines Auftrags werden die
// aktiven Eintraege als Vorschlag angeboten, der Preis wird uebernommen
// und bleibt im Auftrag frei aenderbar. Eintraege werden nie geloescht,
// nur deaktiviert.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum PreislisteFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Bitte eine Bezeichnung eingeben")]
    BezeichnungLeer,
    #[error("Bitte einen gültigen Preis (0 oder mehr) eingeben")]
    UngueltigerPreis,
    #[error("Eintrag wurde nicht gefunden")]
    NichtGefunden,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PreisEintrag {
    pub id: i64,
    pub bezeichnung: String,
    pub kategorie: String,
    pub preis: f64,
    pub aktiv: bool,
}

#[derive(Debug, Deserialize)]
pub struct NeuerPreisEintrag {
    pub bezeichnung: String,
    #[serde(default)]
    pub kategorie: String,
    pub preis: f64,
}

#[derive(Debug, Serialize, PartialEq)]
pub struct PreislisteImport {
    pub neu: usize,
    pub aktualisiert: usize,
}

fn zeile_zu_eintrag(z: &rusqlite::Row) -> rusqlite::Result<PreisEintrag> {
    Ok(PreisEintrag {
        id: z.get(0)?,
        bezeichnung: z.get(1)?,
        kategorie: z.get(2)?,
        preis: z.get(3)?,
        aktiv: z.get::<_, i64>(4)? != 0,
    })
}

fn pruefen(e: &NeuerPreisEintrag) -> Result<(), PreislisteFehler> {
    if e.bezeichnung.trim().is_empty() {
        return Err(PreislisteFehler::BezeichnungLeer);
    }
    if !e.preis.is_finite() || e.preis < 0.0 {
        return Err(PreislisteFehler::UngueltigerPreis);
    }
    Ok(())
}

fn eintrag_holen(conn: &Connection, id: i64) -> Result<PreisEintrag, PreislisteFehler> {
    conn.query_row("SELECT id, bezeichnung, kategorie, preis, aktiv FROM preisliste WHERE id = ?1", [id], zeile_zu_eintrag)
        .optional()?
        .ok_or(PreislisteFehler::NichtGefunden)
}

pub fn preisliste_lesen(conn: &Connection, inaktive_zeigen: bool) -> Result<Vec<PreisEintrag>, PreislisteFehler> {
    let mut stmt = conn.prepare(
        "SELECT id, bezeichnung, kategorie, preis, aktiv FROM preisliste
         WHERE ?1 = 1 OR aktiv = 1
         ORDER BY kategorie COLLATE NOCASE, bezeichnung COLLATE NOCASE",
    )?;
    let eintraege = stmt.query_map([inaktive_zeigen as i64], zeile_zu_eintrag)?.collect::<Result<Vec<_>, _>>()?;
    Ok(eintraege)
}

/// Neuer Eintrag (id = None) oder bestehenden bearbeiten.
pub fn preis_eintrag_speichern(
    conn: &Connection,
    id: Option<i64>,
    e: &NeuerPreisEintrag,
) -> Result<PreisEintrag, PreislisteFehler> {
    pruefen(e)?;
    let id = match id {
        Some(id) => {
            let geaendert = conn.execute(
                "UPDATE preisliste SET bezeichnung = ?1, kategorie = ?2, preis = ?3 WHERE id = ?4",
                params![e.bezeichnung.trim(), e.kategorie.trim(), e.preis, id],
            )?;
            if geaendert == 0 {
                return Err(PreislisteFehler::NichtGefunden);
            }
            id
        }
        None => {
            conn.execute(
                "INSERT INTO preisliste (bezeichnung, kategorie, preis) VALUES (?1, ?2, ?3)",
                params![e.bezeichnung.trim(), e.kategorie.trim(), e.preis],
            )?;
            conn.last_insert_rowid()
        }
    };
    eintrag_holen(conn, id)
}

pub fn preis_eintrag_aktiv_setzen(conn: &Connection, id: i64, aktiv: bool) -> Result<(), PreislisteFehler> {
    let geaendert = conn.execute("UPDATE preisliste SET aktiv = ?1 WHERE id = ?2", params![aktiv as i64, id])?;
    if geaendert == 0 {
        return Err(PreislisteFehler::NichtGefunden);
    }
    Ok(())
}

/// Import aus Excel/CSV. Gibt es eine Arbeit mit derselben Bezeichnung
/// schon (Gross-/Kleinschreibung egal), wird ihr Preis (und, falls
/// angegeben, die Kategorie) aktualisiert und sie wieder aktiviert - so
/// kann dieselbe Liste spaeter mit neuen Preisen einfach nochmals
/// importiert werden, ohne doppelte Eintraege. Ungueltige Zeilen werden
/// uebersprungen statt den ganzen Import abzubrechen.
pub fn preisliste_importieren(
    conn: &mut Connection,
    eingaben: Vec<NeuerPreisEintrag>,
) -> Result<PreislisteImport, PreislisteFehler> {
    let tx = conn.transaction()?;
    let mut ergebnis = PreislisteImport { neu: 0, aktualisiert: 0 };
    for e in eingaben {
        if pruefen(&e).is_err() {
            continue;
        }
        let vorhanden: Option<i64> = tx
            .query_row(
                "SELECT id FROM preisliste WHERE bezeichnung = ?1 COLLATE NOCASE ORDER BY id LIMIT 1",
                [e.bezeichnung.trim()],
                |z| z.get(0),
            )
            .optional()?;
        match vorhanden {
            Some(id) => {
                tx.execute(
                    "UPDATE preisliste SET preis = ?1, aktiv = 1,
                            kategorie = CASE WHEN ?2 = '' THEN kategorie ELSE ?2 END
                     WHERE id = ?3",
                    params![e.preis, e.kategorie.trim(), id],
                )?;
                ergebnis.aktualisiert += 1;
            }
            None => {
                tx.execute(
                    "INSERT INTO preisliste (bezeichnung, kategorie, preis) VALUES (?1, ?2, ?3)",
                    params![e.bezeichnung.trim(), e.kategorie.trim(), e.preis],
                )?;
                ergebnis.neu += 1;
            }
        }
    }
    tx.commit()?;
    Ok(ergebnis)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    fn eintrag(bezeichnung: &str, kategorie: &str, preis: f64) -> NeuerPreisEintrag {
        NeuerPreisEintrag { bezeichnung: bezeichnung.into(), kategorie: kategorie.into(), preis }
    }

    #[test]
    fn preisliste_startet_leer() {
        let conn = test_db();
        assert!(preisliste_lesen(&conn, true).unwrap().is_empty());
    }

    #[test]
    fn eintrag_anlegen_bearbeiten_und_deaktivieren() {
        let conn = test_db();
        let e = preis_eintrag_speichern(&conn, None, &eintrag("Hose kürzen", "Hosen", 25.0)).unwrap();
        assert!(e.aktiv);

        let geaendert = preis_eintrag_speichern(&conn, Some(e.id), &eintrag("Hose kürzen", "Hosen", 28.5)).unwrap();
        assert_eq!(geaendert.preis, 28.5);

        preis_eintrag_aktiv_setzen(&conn, e.id, false).unwrap();
        assert!(preisliste_lesen(&conn, false).unwrap().is_empty(), "inaktive werden standardmaessig ausgeblendet");
        assert_eq!(preisliste_lesen(&conn, true).unwrap().len(), 1, "aber nicht geloescht");
    }

    #[test]
    fn leere_bezeichnung_und_negativer_preis_werden_abgelehnt() {
        let conn = test_db();
        assert!(matches!(
            preis_eintrag_speichern(&conn, None, &eintrag("  ", "", 10.0)),
            Err(PreislisteFehler::BezeichnungLeer)
        ));
        assert!(matches!(
            preis_eintrag_speichern(&conn, None, &eintrag("Saum", "", -1.0)),
            Err(PreislisteFehler::UngueltigerPreis)
        ));
    }

    #[test]
    fn import_legt_neue_an_aktualisiert_bestehende_und_ueberspringt_ungueltige() {
        let mut conn = test_db();
        let alt = preis_eintrag_speichern(&conn, None, &eintrag("Hose kürzen", "Hosen", 25.0)).unwrap();
        preis_eintrag_aktiv_setzen(&conn, alt.id, false).unwrap();

        let ergebnis = preisliste_importieren(
            &mut conn,
            vec![
                eintrag("hose kürzen", "", 27.0), // gleiche Arbeit, andere Schreibweise
                eintrag("Reissverschluss ersetzen", "Reissverschluss", 35.0),
                eintrag("", "Jacken", 10.0),   // ohne Bezeichnung -> uebersprungen
                eintrag("Ärmel kürzen", "Jacken", -5.0), // ungueltiger Preis -> uebersprungen
            ],
        )
        .unwrap();
        assert_eq!(ergebnis, PreislisteImport { neu: 1, aktualisiert: 1 });

        let liste = preisliste_lesen(&conn, false).unwrap();
        assert_eq!(liste.len(), 2);
        let hose = liste.iter().find(|e| e.id == alt.id).unwrap();
        assert_eq!(hose.preis, 27.0);
        assert!(hose.aktiv, "durch den Import wieder aktiviert");
        assert_eq!(hose.kategorie, "Hosen", "leere Kategorie im Import ueberschreibt nichts");
    }
}
