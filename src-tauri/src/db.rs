// Datenbank-Grundlage: eine einzige SQLite-Datei auf dem PC, keine
// Server-Verbindung, kein Internet noetig. Genau das war die Entscheidung
// gegen die Cloud-Loesung - alles bleibt physisch bei Straubs.
//
// Warum SQLite statt vieler einzelner Dateien wie bisher: es gibt keine
// Verknuepfungen zwischen Dateien mehr, die reissen koennten (siehe Papas
// Beschreibung "er nimmt zum Teil Sachen heraus und uebernimmt nichts").
// Eine Rechnung ist eine Zeile in einer Tabelle, der Jahresumsatz wird
// beim Anschauen aus diesen Zeilen zusammengezaehlt statt in einer
// zweiten Datei mitgefuehrt.

use rusqlite::Connection;
use std::path::PathBuf;

pub fn datenbank_pfad() -> PathBuf {
    // %APPDATA%\Atelierbuch Straub\atelierbuch.sqlite3 unter Windows.
    // dirs::data_dir() liefert dort automatisch den richtigen Ordner -
    // wir muessen den Pfad nicht selbst raten.
    let mut pfad = dirs::data_dir().expect("Kein Datenverzeichnis gefunden");
    pfad.push("Atelierbuch Straub");
    std::fs::create_dir_all(&pfad).expect("Datenverzeichnis konnte nicht angelegt werden");
    pfad.push("atelierbuch.sqlite3");
    pfad
}

pub fn verbinden() -> rusqlite::Result<Connection> {
    let conn = Connection::open(datenbank_pfad())?;
    // WAL-Modus: robuster gegen einen Stromausfall oder ein abgestuerztes
    // Programm mitten im Schreiben - die Datei bleibt konsistent.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    schema_anlegen(&conn)?;
    Ok(conn)
}

fn schema_anlegen(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS benutzer (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            benutzername    TEXT NOT NULL UNIQUE,
            anzeigename     TEXT NOT NULL,
            passwort_hash   TEXT NOT NULL,
            rolle           TEXT NOT NULL DEFAULT 'inhaber',
            erstellt_am     TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS kunden (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            -- vom Programm vergeben, fortlaufend - loest den Fall aus
            -- Papas Beschreibung, wo er beim ABC manuell nachzaehlen musste.
            nummer          INTEGER NOT NULL UNIQUE,
            name            TEXT NOT NULL,
            vorname         TEXT NOT NULL DEFAULT '',
            telefon         TEXT NOT NULL DEFAULT '',
            ort             TEXT NOT NULL DEFAULT '',
            adresse         TEXT NOT NULL DEFAULT '',
            email           TEXT NOT NULL DEFAULT '',
            -- 1.5 / 2.5 oder NULL = noch nie mit Karte bezahlt.
            -- Haengt am Kunden, nicht an der Rechnung - siehe Kartengebuehr-
            -- Absprache: einmal einstellen, rechnet sich danach von selbst.
            kartensatz      REAL,
            archiviert      INTEGER NOT NULL DEFAULT 0,
            notiz           TEXT NOT NULL DEFAULT '',
            erstellt_am     TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_kunden_name ON kunden(name, vorname);

        CREATE TABLE IF NOT EXISTS auftraege (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            kunde_id            INTEGER NOT NULL REFERENCES kunden(id),
            -- fortlaufend ueber alle Kunden hinweg, wie Papas "Nr. 1258".
            rechnungsnummer     INTEGER NOT NULL UNIQUE,
            datum               TEXT NOT NULL DEFAULT (date('now')),
            zahlart             TEXT NOT NULL CHECK(zahlart IN ('Bar','Twint','Karte','Rechnung')),
            -- Bruttobetrag, das was die Kundin tatsaechlich zahlt - siehe
            -- Absprache: dieser Betrag bleibt Umsatz, unveraendert von der
            -- Kartengebuehr. Die Gebuehr wird spaeter separat als Ausgabe
            -- gebucht, nicht hier von der Rechnung abgezogen.
            summe               REAL NOT NULL,
            bezahlt             INTEGER NOT NULL DEFAULT 1,
            erstellt_am         TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_auftraege_kunde ON auftraege(kunde_id);
        CREATE INDEX IF NOT EXISTS idx_auftraege_datum ON auftraege(datum);

        CREATE TABLE IF NOT EXISTS auftrag_posten (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            auftrag_id  INTEGER NOT NULL REFERENCES auftraege(id) ON DELETE CASCADE,
            bezeichnung TEXT NOT NULL,
            stueck      REAL NOT NULL DEFAULT 1,
            preis       REAL NOT NULL DEFAULT 0
        );

        -- Laufende Zaehler (naechste Kundennummer, naechste Rechnungsnummer)
        -- und sonstige Einstellungen wie der Sicherungs-Ordner.
        CREATE TABLE IF NOT EXISTS einstellungen (
            schluessel  TEXT PRIMARY KEY,
            wert        TEXT NOT NULL
        );
        "#,
    )
}
