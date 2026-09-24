// Login fuer Papa und Mama auf demselben PC. Passwoerter werden nie im
// Klartext gespeichert, sondern als Argon2-Hash - selbst wer die
// Datenbankdatei in die Finger bekommt, kann das Passwort nicht auslesen.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rusqlite::{Connection, OptionalExtension};
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

/// Legt beim allerersten Start automatisch das Konto fuer Papa an, falls
/// noch niemand existiert - er muss also nicht selbst einen SQL-Befehl
/// eintippen. Das Startpasswort steht danach einmalig in der Konsole /
/// einer Textdatei, damit er es sofort aendern kann.
pub fn erstkonto_sicherstellen(conn: &Connection) -> Result<Option<(String, String)>, AuthFehler> {
    let anzahl: i64 = conn.query_row("SELECT COUNT(*) FROM benutzer", [], |z| z.get(0))?;
    if anzahl > 0 {
        return Ok(None);
    }
    let start_passwort = zufalls_passwort();
    konto_anlegen(conn, "papa", "Rosmarie / Beat Straub", &start_passwort, "inhaber")?;
    Ok(Some(("papa".to_string(), start_passwort)))
}

fn zufalls_passwort() -> String {
    use rand::Rng;
    const ZEICHEN: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    let mut rng = rand::thread_rng();
    (0..10).map(|_| ZEICHEN[rng.gen_range(0..ZEICHEN.len())] as char).collect()
}

pub fn konto_anlegen(
    conn: &Connection,
    benutzername: &str,
    anzeigename: &str,
    passwort: &str,
    rolle: &str,
) -> Result<(), AuthFehler> {
    let hash = passwort_hashen(passwort)?;
    conn.execute(
        "INSERT INTO benutzer (benutzername, anzeigename, passwort_hash, rolle) VALUES (?1, ?2, ?3, ?4)",
        (benutzername, anzeigename, hash, rolle),
    )?;
    Ok(())
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

pub fn passwort_aendern(conn: &Connection, benutzer_id: i64, neues_passwort: &str) -> Result<(), AuthFehler> {
    let hash = passwort_hashen(neues_passwort)?;
    conn.execute("UPDATE benutzer SET passwort_hash = ?1 WHERE id = ?2", (hash, benutzer_id))?;
    Ok(())
}
