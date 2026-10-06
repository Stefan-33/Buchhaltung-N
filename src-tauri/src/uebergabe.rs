// Daten-Uebergabe auf einen anderen PC: Stefan richtet auf seinem PC alles
// ein (Konten, Kunden, Excel-Importe) und gibt die Daten seinem Vater -
// als eine einzige Datei ("Daten-Paket"), nicht ueber die oeffentliche
// Download-Seite. Der Vater installiert das Programm und spielt das Paket
// mit einem Klick ein (auch direkt im Ersteinrichtungs-Bildschirm).
//
// Das Paket ist eine vollstaendige, in sich stimmige Kopie der Datenbank
// (SQLite "VACUUM INTO", schliesst auch noch nicht zurueckgeschriebene
// Aenderungen aus der WAL-Datei mit ein). Einspielen ersetzt die Daten auf
// diesem PC komplett - vorher wird automatisch eine Sicherung angelegt.

use chrono::Local;
use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use std::path::{Path, PathBuf};

fn uebergabe_ordner() -> PathBuf {
    crate::sicherung::sicherungs_ordner().parent().map(Path::to_path_buf).unwrap_or_default().join("Übergabe")
}

/// Schreibt eine vollstaendige Kopie der Datenbank nach `ziel` (darf noch
/// nicht existieren).
pub fn datenbank_kopieren(conn: &Connection, ziel: &Path) -> Result<(), String> {
    if ziel.exists() {
        std::fs::remove_file(ziel).map_err(|e| e.to_string())?;
    }
    conn.execute("VACUUM INTO ?1", [ziel.to_string_lossy()])
        .map_err(|e| format!("Daten konnten nicht kopiert werden: {e}"))?;
    Ok(())
}

/// Legt das Daten-Paket unter "Dokumente \ Atelierbuch Straub \ Übergabe"
/// ab und gibt den Pfad zurueck.
pub fn datenpaket_erstellen(conn: &Connection) -> Result<PathBuf, String> {
    let ordner = uebergabe_ordner();
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let pfad = ordner.join(format!("Atelierbuch-Daten_{}.atelierbuch", Local::now().format("%Y-%m-%d_%H-%M")));
    datenbank_kopieren(conn, &pfad)?;
    Ok(pfad)
}

/// Was in einem Daten-Paket steckt - zum Bestaetigen vor dem Einspielen.
#[derive(Debug, Serialize, PartialEq)]
pub struct PaketInfo {
    pub kunden: i64,
    pub auftraege: i64,
    pub ausgaben: i64,
    pub einnahmen_excel: i64,
    pub stunden: i64,
    /// Anzeigenamen der Konten, mit denen man sich danach anmeldet.
    pub konten: Vec<String>,
}

fn zaehlen(conn: &Connection, tabelle: &str) -> i64 {
    // Aeltere Pakete haben evtl. noch nicht jede Tabelle - dann 0.
    conn.query_row(&format!("SELECT COUNT(*) FROM {tabelle}"), [], |z| z.get(0)).unwrap_or(0)
}

fn paket_oeffnen(pfad: &Path) -> Result<Connection, String> {
    let conn = Connection::open_with_flags(pfad, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|_| "Die Datei konnte nicht geöffnet werden.".to_string())?;
    // Ein echtes Atelierbuch-Paket hat mindestens diese Tabellen.
    let ok: bool = conn
        .query_row(
            "SELECT COUNT(*) = 3 FROM sqlite_master WHERE type = 'table' AND name IN ('benutzer', 'kunden', 'auftraege')",
            [],
            |z| z.get(0),
        )
        .unwrap_or(false);
    if !ok {
        return Err("Diese Datei ist kein Daten-Paket des Atelierbuchs.".to_string());
    }
    Ok(conn)
}

pub fn datenpaket_pruefen(pfad: &Path) -> Result<PaketInfo, String> {
    let conn = paket_oeffnen(pfad)?;
    let mut stmt = conn.prepare("SELECT anzeigename FROM benutzer ORDER BY id").map_err(|e| e.to_string())?;
    let konten = stmt
        .query_map([], |z| z.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(PaketInfo {
        kunden: zaehlen(&conn, "kunden"),
        auftraege: zaehlen(&conn, "auftraege"),
        ausgaben: zaehlen(&conn, "ausgaben"),
        einnahmen_excel: zaehlen(&conn, "einnahmen_extern"),
        stunden: zaehlen(&conn, "arbeitsstunden"),
        konten,
    })
}

/// Ersetzt die Daten dieses PCs durch das Paket (SQLite-Backup in die
/// offene Verbindung) und bringt sie auf den Stand dieser Programmversion.
/// Die Sicherung vorher macht commands.rs, nur wenn schon Daten da sind.
pub fn datenpaket_einspielen(conn: &mut Connection, pfad: &Path) -> Result<(), String> {
    let quelle = paket_oeffnen(pfad)?;
    {
        let backup = rusqlite::backup::Backup::new(&quelle, conn).map_err(|e| e.to_string())?;
        backup
            .run_to_completion(500, std::time::Duration::ZERO, None)
            .map_err(|e| format!("Daten konnten nicht eingespielt werden: {e}"))?;
    }
    crate::db::schema_anlegen(conn).map_err(|e| e.to_string())?;
    Ok(())
}

/// Zeigt die Datei im Windows-Explorer (zum Kopieren auf einen USB-Stick).
pub fn im_explorer_zeigen(pfad: &Path) {
    let _ = std::process::Command::new("explorer").arg(format!("/select,{}", pfad.display())).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_pfad(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("atelierbuch-test-{}-{name}", std::process::id()))
    }

    // Stefans Fall: auf seinem PC eingerichtet (Konto, Kunden, Auftrag,
    // Excel-Einnahmen), als Paket kopiert, beim Vater in ein frisches
    // Programm eingespielt - alles ist da, auch die Anmeldung.
    #[test]
    fn paket_von_einem_pc_auf_den_anderen() {
        let quelle_pfad = temp_pfad("quelle.sqlite3");
        let paket = temp_pfad("paket.atelierbuch");
        let _ = std::fs::remove_file(&quelle_pfad);
        {
            let conn = Connection::open(&quelle_pfad).unwrap();
            conn.pragma_update(None, "journal_mode", "WAL").unwrap();
            crate::db::schema_fuer_tests_anlegen(&conn);
            crate::auth::ersteinrichtung_abschliessen(&conn, "papa", "Beat", "geheim123").unwrap();
            conn.execute_batch(
                "INSERT INTO kunden (nummer, name) VALUES (101, 'Meier'), (102, 'Keller');
                 INSERT INTO einnahmen_extern (datum, betrag, zahlart) VALUES ('2026-06-30', 50.0, 'Bar');",
            )
            .unwrap();
            datenbank_kopieren(&conn, &paket).unwrap();
        }

        let info = datenpaket_pruefen(&paket).unwrap();
        assert_eq!((info.kunden, info.einnahmen_excel), (2, 1));
        assert_eq!(info.konten, vec!["Beat".to_string()]);

        // Beim Vater wie im echten Programm: Datei im WAL-Modus, offen.
        let vater_pfad = temp_pfad("vater.sqlite3");
        let _ = std::fs::remove_file(&vater_pfad);
        {
            let mut beim_vater = Connection::open(&vater_pfad).unwrap();
            beim_vater.pragma_update(None, "journal_mode", "WAL").unwrap();
            crate::db::schema_fuer_tests_anlegen(&beim_vater);
            datenpaket_einspielen(&mut beim_vater, &paket).unwrap();
            assert_eq!(zaehlen(&beim_vater, "kunden"), 2);
            assert!(crate::auth::anmelden(&beim_vater, "papa", "geheim123").is_ok(), "Anmeldung wie auf Stefans PC");
        }
        // Nach einem Neustart (neue Verbindung) ist alles noch da.
        let neu = Connection::open(&vater_pfad).unwrap();
        assert_eq!(zaehlen(&neu, "kunden"), 2);
        assert_eq!(zaehlen(&neu, "einnahmen_extern"), 1);
        drop(neu);

        for p in [&quelle_pfad, &paket, &vater_pfad] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn fremde_datei_wird_abgelehnt() {
        let pfad = temp_pfad("fremd.sqlite3");
        let _ = std::fs::remove_file(&pfad);
        Connection::open(&pfad).unwrap().execute_batch("CREATE TABLE irgendwas (x INTEGER);").unwrap();
        assert!(datenpaket_pruefen(&pfad).is_err());
        let mut ziel = Connection::open_in_memory().unwrap();
        assert!(datenpaket_einspielen(&mut ziel, &pfad).is_err());
        let _ = std::fs::remove_file(&pfad);

        let kein_sqlite = temp_pfad("text.atelierbuch");
        std::fs::write(&kein_sqlite, "Hallo").unwrap();
        assert!(datenpaket_pruefen(&kein_sqlite).is_err());
        let _ = std::fs::remove_file(&kein_sqlite);
    }
}
