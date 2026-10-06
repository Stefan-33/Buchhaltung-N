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

        -- Standardarbeiten mit Preis (z.B. "Hose kuerzen"), werden beim
        -- Erfassen eines Auftrags als Vorschlag angeboten. Bewusst ohne
        -- feste Verknuepfung zu auftrag_posten: der Preis wird nur
        -- uebernommen und bleibt im Auftrag frei aenderbar. Eintraege werden
        -- nie geloescht, nur deaktiviert (aktiv = 0).
        CREATE TABLE IF NOT EXISTS preisliste (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            bezeichnung TEXT NOT NULL,
            kategorie   TEXT NOT NULL DEFAULT '',
            preis       REAL NOT NULL DEFAULT 0,
            aktiv       INTEGER NOT NULL DEFAULT 1,
            erstellt_am TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Checkliste "Formulare" pro Mitarbeiterin (Reiter "Mitarbeiter"):
        -- welche Anmeldung/Meldung schon erledigt ist. Jaehrliche Meldungen
        -- tragen das Jahr im Schluessel (z.B. "lohnausweis:2026"). Nicht
        -- erledigt = keine Zeile.
        CREATE TABLE IF NOT EXISTS mitarbeiter_formulare (
            benutzer_id INTEGER NOT NULL REFERENCES benutzer(id),
            formular    TEXT NOT NULL,
            erledigt_am TEXT NOT NULL,
            PRIMARY KEY (benutzer_id, formular)
        );
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

/// Spalten, die erst nach dem allerersten Release zu einer Tabelle
/// dazugekommen sind. Bewusst einzeln per ALTER TABLE nachgezogen statt
/// die Tabelle neu anzulegen, damit bestehende Zeilen (Konten, Ausgaben,
/// ...) erhalten bleiben.
fn migrationen_anwenden(conn: &Connection) -> rusqlite::Result<()> {
    // Mitarbeiterinnen-Lohnprofil (Treuhand-Lohnabrechnung).
    let benutzer_spalten: &[(&str, &str)] = &[
        ("strasse", "TEXT NOT NULL DEFAULT ''"),
        ("plz_ort", "TEXT NOT NULL DEFAULT ''"),
        ("ahv_nummer", "TEXT NOT NULL DEFAULT ''"),
        ("stundenlohn", "REAL"),
        // 1 = kann sich selbst anmelden, 0 = reines Lohn-Profil ohne Zugang
        // (siehe Stefans Wunsch "kein Login gar nix" fuer eine
        // Mitarbeiterin, deren Stunden er selbst eintraegt).
        ("hat_login", "INTEGER NOT NULL DEFAULT 1"),
        // Personalien fuer Lohnausweis und AHV-Anmeldung (Reiter
        // "Mitarbeiter"), je "JJJJ-MM-TT" oder leer.
        ("geburtsdatum", "TEXT NOT NULL DEFAULT ''"),
        ("eintritt", "TEXT NOT NULL DEFAULT ''"),
        ("austritt", "TEXT NOT NULL DEFAULT ''"),
    ];
    // Beleg (Foto/PDF der Quittung) zu einer Ausgabe - siehe Stefans
    // Wunsch, beim Erfassen gleich eine Datei dazu ablegen zu koennen.
    let ausgaben_spalten: &[(&str, &str)] = &[("beleg_pfad", "TEXT")];
    // Auftrags-Ablauf: Annahme -> Abrechnen beim Abholen, offene Posten.
    // Die Standardwerte machen alle bereits bestehenden Auftraege zu
    // "abgeholt und bezahlt" - genau das, was sie bisher waren.
    let auftraege_spalten: &[(&str, &str)] = &[
        ("bezahlt", "INTEGER NOT NULL DEFAULT 1"),
        ("status", "TEXT NOT NULL DEFAULT 'Abgeholt'"),
        ("abholdatum", "TEXT"),
        ("angenommen_am", "TEXT"),
        ("bezahlt_am", "TEXT"),
    ];
    let bezahlt_am_neu = spalte_fehlt(conn, "auftraege", "bezahlt_am")?;

    for (tabelle, spalten) in [
        ("benutzer", benutzer_spalten),
        ("ausgaben", ausgaben_spalten),
        ("auftraege", auftraege_spalten),
    ] {
        for (spalte, definition) in spalten {
            if spalte_fehlt(conn, tabelle, spalte)? {
                conn.execute(&format!("ALTER TABLE {tabelle} ADD COLUMN {spalte} {definition}"), [])?;
            }
        }
    }

    // Einmalig beim Hinzufuegen der Spalte: bei allen schon bezahlten
    // Auftraegen gilt das Auftragsdatum als Bezahldatum.
    if bezahlt_am_neu {
        conn.execute("UPDATE auftraege SET bezahlt_am = datum WHERE bezahlt = 1 AND bezahlt_am IS NULL", [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Genau Stefans Fall: eine bestehende Datenbank aus einer frueheren
    // Version (Auftraege ohne Status/Abholdatum/Bezahldatum) - nach dem
    // Update muessen alle alten Auftraege als abgeholt und bezahlt gelten,
    // ohne dass eine Zeile verloren geht.
    #[test]
    fn alte_auftraege_werden_bei_der_migration_abgeholt_und_bezahlt() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE kunden (
                id INTEGER PRIMARY KEY AUTOINCREMENT, nummer INTEGER NOT NULL UNIQUE,
                name TEXT NOT NULL, vorname TEXT NOT NULL DEFAULT '', telefon TEXT NOT NULL DEFAULT '',
                ort TEXT NOT NULL DEFAULT '', adresse TEXT NOT NULL DEFAULT '', email TEXT NOT NULL DEFAULT '',
                kartensatz REAL, archiviert INTEGER NOT NULL DEFAULT 0, notiz TEXT NOT NULL DEFAULT '',
                erstellt_am TEXT NOT NULL DEFAULT (datetime('now')));
             CREATE TABLE auftraege (
                id INTEGER PRIMARY KEY AUTOINCREMENT, kunde_id INTEGER NOT NULL REFERENCES kunden(id),
                rechnungsnummer INTEGER NOT NULL UNIQUE, datum TEXT NOT NULL DEFAULT (date('now')),
                zahlart TEXT NOT NULL CHECK(zahlart IN ('Bar','Twint','Karte','Rechnung')),
                summe REAL NOT NULL, bezahlt INTEGER NOT NULL DEFAULT 1,
                erstellt_am TEXT NOT NULL DEFAULT (datetime('now')));
             INSERT INTO kunden (nummer, name) VALUES (101, 'Meier');
             INSERT INTO auftraege (kunde_id, rechnungsnummer, datum, zahlart, summe) VALUES (1, 1258, '2025-03-14', 'Bar', 42.0);
             INSERT INTO auftraege (kunde_id, rechnungsnummer, datum, zahlart, summe) VALUES (1, 1259, '2025-04-02', 'Rechnung', 30.0);",
        )
        .unwrap();

        schema_anlegen(&conn).unwrap();

        let mut stmt = conn
            .prepare("SELECT status, bezahlt, bezahlt_am, datum, abholdatum FROM auftraege ORDER BY id")
            .unwrap();
        let zeilen: Vec<(String, i64, Option<String>, String, Option<String>)> = stmt
            .query_map([], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?, z.get(3)?, z.get(4)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(zeilen.len(), 2, "keine Zeile darf verloren gehen");
        for (status, bezahlt, bezahlt_am, datum, abholdatum) in zeilen {
            assert_eq!(status, "Abgeholt");
            assert_eq!(bezahlt, 1);
            assert_eq!(bezahlt_am.as_deref(), Some(datum.as_str()));
            assert_eq!(abholdatum, None);
        }

        // Ein zweiter Start darf nichts mehr veraendern oder abbrechen.
        schema_anlegen(&conn).unwrap();
    }

    // Bestehende Konten aus einer frueheren Version (ohne Personalien-
    // Spalten) bleiben beim Update vollstaendig erhalten, die neuen Felder
    // sind einfach leer.
    #[test]
    fn bestehende_mitarbeiterin_behaelt_ihr_lohnprofil_bei_der_migration() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE benutzer (
                id INTEGER PRIMARY KEY AUTOINCREMENT, benutzername TEXT NOT NULL UNIQUE, anzeigename TEXT NOT NULL,
                passwort_hash TEXT NOT NULL, rolle TEXT NOT NULL DEFAULT 'inhaber',
                erstellt_am TEXT NOT NULL DEFAULT (datetime('now')),
                strasse TEXT NOT NULL DEFAULT '', plz_ort TEXT NOT NULL DEFAULT '', ahv_nummer TEXT NOT NULL DEFAULT '',
                stundenlohn REAL, hat_login INTEGER NOT NULL DEFAULT 1);
             INSERT INTO benutzer (benutzername, anzeigename, passwort_hash, rolle, ahv_nummer, stundenlohn, hat_login)
                VALUES ('m1', 'Erika Muster', 'x', 'mitarbeiterin', '756.1234.5678.97', 24.66, 0);",
        )
        .unwrap();

        schema_anlegen(&conn).unwrap();

        let b = crate::auth::benutzer_holen(&conn, 1).unwrap();
        assert_eq!((b.anzeigename.as_str(), b.ahv_nummer.as_str(), b.stundenlohn), ("Erika Muster", "756.1234.5678.97", Some(24.66)));
        assert_eq!((b.geburtsdatum.as_str(), b.eintritt.as_str(), b.austritt.as_str()), ("", "", ""));
        schema_anlegen(&conn).unwrap();
    }
}
