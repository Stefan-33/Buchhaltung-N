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

// Fuer Tests in anderen Modulen (z.B. geschaeft.rs) - dieselbe Tabellen-
// Definition wie im echten Betrieb, nur ohne Datei auf der Platte.
#[cfg(test)]
pub fn schema_fuer_tests_anlegen(conn: &Connection) {
    schema_anlegen(conn).expect("Test-Schema konnte nicht angelegt werden");
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

        -- Arbeitsstunden pro Person - bewusst eine eigene, von Kunden und
        -- Auftraegen komplett getrennte Tabelle (siehe Stefans Wunsch: beim
        -- Erfassen nicht mit den Kunden vermischen). Jede Person sieht und
        -- erfasst nur ihre eigenen Zeilen (benutzer_id), Papa/Mama sehen
        -- zusaetzlich alle zusammen fuer die Lohnabrechnung (die passiert
        -- weiterhin in Stefans eigenem Excel - das Programm liefert nur die
        -- rohen Stunden, siehe stunden.csv in der Sicherung).
        --
        -- Wie in Stefans bisheriger Excel-Vorlage: pro Tag zwei Zeitbloecke
        -- (Vormittag, Nachmittag), je als "HH:MM"-Text, ein Block darf leer
        -- (NULL) bleiben. Die Stundenzahl wird daraus berechnet, nicht
        -- separat gespeichert (stunden.rs).
        CREATE TABLE IF NOT EXISTS arbeitsstunden (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            benutzer_id     INTEGER NOT NULL REFERENCES benutzer(id),
            datum           TEXT NOT NULL,
            vm_beginn       TEXT,
            vm_ende         TEXT,
            nm_beginn       TEXT,
            nm_ende         TEXT,
            notiz           TEXT NOT NULL DEFAULT '',
            erstellt_am     TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_arbeitsstunden_benutzer ON arbeitsstunden(benutzer_id, datum);

        -- Geschaeftsausgaben fuer den jaehrlichen Treuhand-Bericht (Einnahmen
        -- kommen weiterhin aus "auftraege", werden nicht hier dupliziert).
        -- Feste Kategorie-Liste statt einer eigenen Tabelle dafuer - siehe
        -- treuhand.rs - deckt sich mit Stefans bisheriger Treuhand-Excel.
        CREATE TABLE IF NOT EXISTS ausgaben (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            datum       TEXT NOT NULL,
            kategorie   TEXT NOT NULL,
            betrag      REAL NOT NULL,
            notiz       TEXT NOT NULL DEFAULT '',
            erstellt_am TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_ausgaben_datum ON ausgaben(datum);
        "#,
    )?;
    migrationen_anwenden(conn)
}

/// Ob eine Spalte in einer bestehenden Tabelle noch fehlt - "CREATE TABLE IF
/// NOT EXISTS" allein reicht nicht, sobald eine Spalte zu einer Tabelle
/// dazukommt, die bei Stefan zuhause schon existiert (seine echte
/// atelierbuch.sqlite3 wird nie neu angelegt, nur weiterverwendet).
fn spalte_fehlt(conn: &Connection, tabelle: &str, spalte: &str) -> rusqlite::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({tabelle})"))?;
    let vorhandene_spalten: Vec<String> = stmt.query_map([], |z| z.get(1))?.collect::<Result<_, _>>()?;
    Ok(!vorhandene_spalten.iter().any(|s| s == spalte))
}

/// Spalten, die nach dem allerersten Release zu "benutzer" dazugekommen
/// sind - fuer das Mitarbeiterinnen-Lohnprofil (Treuhand-Lohnabrechnung).
/// Bewusst einzeln per ALTER TABLE nachgezogen statt die Tabelle neu
/// anzulegen, damit die bestehenden Konten (Papa, Mama, ...) erhalten
/// bleiben.
fn migrationen_anwenden(conn: &Connection) -> rusqlite::Result<()> {
    let neue_spalten: &[(&str, &str)] = &[
        ("strasse", "TEXT NOT NULL DEFAULT ''"),
        ("plz_ort", "TEXT NOT NULL DEFAULT ''"),
        ("ahv_nummer", "TEXT NOT NULL DEFAULT ''"),
        ("stundenlohn", "REAL"),
        // 1 = kann sich selbst anmelden, 0 = reines Lohn-Profil ohne Zugang
        // (siehe Stefans Wunsch "kein Login gar nix" fuer eine
        // Mitarbeiterin, deren Stunden er selbst eintraegt).
        ("hat_login", "INTEGER NOT NULL DEFAULT 1"),
    ];
    for (spalte, definition) in neue_spalten {
        if spalte_fehlt(conn, "benutzer", spalte)? {
            conn.execute(&format!("ALTER TABLE benutzer ADD COLUMN {spalte} {definition}"), [])?;
        }
    }
    Ok(())
}
