// Login fuer Papa und Mama auf demselben PC. Passwoerter werden nie im
// Klartext gespeichert, sondern als Argon2-Hash - selbst wer die
// Datenbankdatei in die Finger bekommt, kann das Passwort nicht auslesen.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{Connection, ErrorCode, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Benutzer {
    pub id: i64,
    pub benutzername: String,
    pub anzeigename: String,
    pub rolle: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AuthFehler {
    #[error("Benutzername oder Passwort ist falsch")]
    UngueltigeAnmeldung,
    #[error("Dieser Benutzername ist schon vergeben. Bitte einen anderen wählen.")]
    BenutzernameBelegt,
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Passwort konnte nicht verarbeitet werden")]
    Hashing,
}

fn passwort_hashen(passwort: &str) -> Result<String, AuthFehler> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(passwort.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| AuthFehler::Hashing)
}

fn passwort_pruefen(passwort: &str, hash: &str) -> bool {
    let Ok(geparst) = PasswordHash::new(hash) else { return false };
    Argon2::default()
        .verify_password(passwort.as_bytes(), &geparst)
        .is_ok()
}

/// Noch niemand im Programm eingerichtet? Dann zeigt die Oberflaeche statt
/// des Login-Formulars die Ersteinrichtung. Kein Zufallspasswort mehr, das
/// irgendwo im Protokoll verschwinden koennte (genau das ist der ersten
/// Fassung passiert) - Papa vergibt sein Passwort selbst, im selben Zug.
pub fn ist_ersteinrichtung(conn: &Connection) -> Result<bool, AuthFehler> {
    let anzahl: i64 = conn.query_row("SELECT COUNT(*) FROM benutzer", [], |z| z.get(0))?;
    Ok(anzahl == 0)
}

pub fn ersteinrichtung_abschliessen(
    conn: &Connection,
    benutzername: &str,
    anzeigename: &str,
    passwort: &str,
) -> Result<Benutzer, AuthFehler> {
    // Nochmals pruefen statt blind einzufuegen: Jeder Tauri-Befehl haelt die
    // Datenbank-Sperre fuer seine gesamte Laufzeit (siehe verbindung_sperren
    // in commands.rs), zwei Anfragen laufen also nie wirklich gleichzeitig,
    // sondern strikt nacheinander. Trifft ein Doppelklick oder "Enter +
    // Klick" das Formular zweimal, legt die erste Anfrage das Konto an -
    // die zweite sieht hier bereits ist_ersteinrichtung() == false und
    // meldet stattdessen einfach mit denselben Daten an, statt einen
    // haesslichen "existiert schon"-Fehler zu zeigen.
    if !ist_ersteinrichtung(conn)? {
        return anmelden(conn, benutzername, passwort);
    }
    konto_anlegen(conn, benutzername, anzeigename, passwort, "inhaber")?;
    anmelden(conn, benutzername, passwort)
}

pub fn konto_anlegen(
    conn: &Connection,
    benutzername: &str,
    anzeigename: &str,
    passwort: &str,
    rolle: &str,
) -> Result<(), AuthFehler> {
    let hash = passwort_hashen(passwort)?;
    let ergebnis = conn.execute(
        "INSERT INTO benutzer (benutzername, anzeigename, passwort_hash, rolle) VALUES (?1, ?2, ?3, ?4)",
        (benutzername, anzeigename, hash, rolle),
    );
    match ergebnis {
        Ok(_) => Ok(()),
        // "benutzername" ist UNIQUE in der Tabelle - statt der rohen
        // SQLite-Fehlermeldung eine verstaendliche Meldung zeigen.
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::ConstraintViolation => {
            Err(AuthFehler::BenutzernameBelegt)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn anmelden(conn: &Connection, benutzername: &str, passwort: &str) -> Result<Benutzer, AuthFehler> {
    let treffer: Option<(i64, String, String, String, String)> = conn
        .query_row(
            "SELECT id, benutzername, anzeigename, rolle, passwort_hash FROM benutzer WHERE benutzername = ?1",
            [benutzername],
            |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?, z.get(3)?, z.get(4)?)),
        )
        .optional()?;

    match treffer {
        Some((id, benutzername, anzeigename, rolle, hash)) if passwort_pruefen(passwort, &hash) => {
            Ok(Benutzer { id, benutzername, anzeigename, rolle })
        }
        _ => Err(AuthFehler::UngueltigeAnmeldung),
    }
}

/// Fuer den "Fuer wen?"-Auswahl beim Stunden-Import: Papa/Mama koennen
/// damit Stunden fuer eine bestimmte Mitarbeiterin nachtragen, statt nur
/// fuer sich selbst.
pub fn alle_benutzer(conn: &Connection) -> Result<Vec<Benutzer>, AuthFehler> {
    let mut stmt = conn.prepare("SELECT id, benutzername, anzeigename, rolle FROM benutzer ORDER BY anzeigename")?;
    let zeilen = stmt
        .query_map([], |z| {
            Ok(Benutzer { id: z.get(0)?, benutzername: z.get(1)?, anzeigename: z.get(2)?, rolle: z.get(3)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

pub fn passwort_aendern(conn: &Connection, benutzer_id: i64, neues_passwort: &str) -> Result<(), AuthFehler> {
    let hash = passwort_hashen(neues_passwort)?;
    conn.execute("UPDATE benutzer SET passwort_hash = ?1 WHERE id = ?2", (hash, benutzer_id))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    // Simuliert exakt den Ablauf, den Stefan am Bildschirm hatte: leere
    // Datenbank, Ersteinrichtung mit denselben Werten. Soll in deutlich
    // unter einer Sekunde durchlaufen, ohne Panik und ohne zu haengen.
    #[test]
    fn ersteinrichtung_laeuft_schnell_und_fehlerfrei_durch() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE benutzer (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                benutzername TEXT NOT NULL UNIQUE,
                anzeigename TEXT NOT NULL,
                passwort_hash TEXT NOT NULL,
                rolle TEXT NOT NULL DEFAULT 'inhaber',
                erstellt_am TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();

        assert!(ist_ersteinrichtung(&conn).unwrap(), "Datenbank sollte leer sein");

        let start = Instant::now();
        let benutzer = ersteinrichtung_abschliessen(&conn, "papa", "hansueli", "12345678").unwrap();
        let dauer = start.elapsed();

        assert_eq!(benutzer.benutzername, "papa");
        assert_eq!(benutzer.anzeigename, "hansueli");
        assert!(!ist_ersteinrichtung(&conn).unwrap(), "Konto sollte jetzt existieren");
        assert!(dauer.as_secs() < 2, "Dauerte verdaechtig lang: {:?}", dauer);

        // Direkt danach normal einloggen - muss ebenfalls klappen.
        let eingeloggt = anmelden(&conn, "papa", "12345678").unwrap();
        assert_eq!(eingeloggt.id, benutzer.id);

        // Falsches Passwort muss sauber abgelehnt werden, nicht haengen.
        assert!(anmelden(&conn, "papa", "falsch").is_err());
    }

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE benutzer (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                benutzername TEXT NOT NULL UNIQUE,
                anzeigename TEXT NOT NULL,
                passwort_hash TEXT NOT NULL,
                rolle TEXT NOT NULL DEFAULT 'inhaber',
                erstellt_am TEXT NOT NULL DEFAULT (datetime('now'))
            );",
        )
        .unwrap();
        conn
    }

    // Der Knopf "+ Mitarbeiterin anlegen" ruft genau das auf: ein zweites
    // Konto mit Rolle "mitarbeiterin" statt "inhaber". Wird derselbe
    // Benutzername zweimal vergeben (z.B. aus Versehen "mitarbeiterin1"
    // nochmal angelegt), soll eine verstaendliche Meldung kommen statt der
    // rohen SQLite-Fehlermeldung.
    #[test]
    fn mitarbeiterin_konto_mit_eigener_rolle_und_klare_meldung_bei_doppeltem_benutzernamen() {
        let conn = test_db();

        konto_anlegen(&conn, "mitarbeiterin1", "Mitarbeiterin 1", "geheim123", "mitarbeiterin").unwrap();
        let eingeloggt = anmelden(&conn, "mitarbeiterin1", "geheim123").unwrap();
        assert_eq!(eingeloggt.rolle, "mitarbeiterin");

        let zweiter_versuch = konto_anlegen(&conn, "mitarbeiterin1", "Mitarbeiterin X", "andres123", "mitarbeiterin");
        match zweiter_versuch {
            Err(AuthFehler::BenutzernameBelegt) => {}
            andere => panic!("erwartet BenutzernameBelegt, bekam: {:?}", andere),
        }
    }
}
