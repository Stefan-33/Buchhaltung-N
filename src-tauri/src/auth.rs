// Login fuer Papa und Mama auf demselben PC. Passwoerter werden nie im
// Klartext gespeichert, sondern als Argon2-Hash - selbst wer die
// Datenbankdatei in die Finger bekommt, kann das Passwort nicht auslesen.

use argon2::password_hash::{
    rand_core::{OsRng, RngCore},
    PasswordHash, PasswordHasher, PasswordVerifier, SaltString,
};
use argon2::Argon2;
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Benutzer {
    pub id: i64,
    pub benutzername: String,
    pub anzeigename: String,
    pub rolle: String,
    // Lohnprofil fuer die Treuhand-Lohnabrechnung (treuhand.rs) - bei Papa/
    // Mama immer leer/NULL, nur bei einer Mitarbeiterin ausgefuellt.
    pub strasse: String,
    pub plz_ort: String,
    pub ahv_nummer: String,
    pub stundenlohn: Option<f64>,
    // false = reines Lohn-Profil, kann sich nicht anmelden (siehe
    // mitarbeiterin_anlegen) - Stefan traegt deren Stunden selbst ein.
    pub hat_login: bool,
}

/// Eingabe fuer "Mitarbeiterin anlegen" - deckt gleich das ganze
/// Lohnprofil mit ab (Stefans Wunsch: "direkt auch mit allem anlegen"),
/// statt nur Benutzername/Anzeigename wie bisher. Bewusst kein Login
/// dabei - braucht es laut Stefan nicht, sie bekommt ein reines
/// Lohn-Profil ohne Zugang zum Programm.
#[derive(Debug, Deserialize)]
pub struct NeueMitarbeiterin {
    pub anzeigename: String,
    #[serde(default)]
    pub strasse: String,
    #[serde(default)]
    pub plz_ort: String,
    #[serde(default)]
    pub ahv_nummer: String,
    #[serde(default)]
    pub stundenlohn: Option<f64>,
}

const BENUTZER_SPALTEN: &str = "id, benutzername, anzeigename, rolle, strasse, plz_ort, ahv_nummer, stundenlohn, hat_login";

fn zeile_zu_benutzer(z: &Row) -> rusqlite::Result<Benutzer> {
    Ok(Benutzer {
        id: z.get(0)?,
        benutzername: z.get(1)?,
        anzeigename: z.get(2)?,
        rolle: z.get(3)?,
        strasse: z.get(4)?,
        plz_ort: z.get(5)?,
        ahv_nummer: z.get(6)?,
        stundenlohn: z.get(7)?,
        hat_login: z.get::<_, i64>(8)? != 0,
    })
}

pub(crate) fn benutzer_holen(conn: &Connection, id: i64) -> Result<Benutzer, AuthFehler> {
    let sql = format!("SELECT {BENUTZER_SPALTEN} FROM benutzer WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], zeile_zu_benutzer)?)
}

/// Zufaelliger, technischer Benutzername/Passwort fuer eine Mitarbeiterin
/// ohne eigenen Login - wird nie angezeigt oder gebraucht, muss nur die
/// UNIQUE/NOT NULL-Vorgaben der Tabelle erfuellen.
fn zufallstext(praefix: &str) -> String {
    format!("{praefix}_{:016x}{:016x}", OsRng.next_u64(), OsRng.next_u64())
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
    let sql = format!("SELECT {BENUTZER_SPALTEN}, passwort_hash FROM benutzer WHERE benutzername = ?1");
    let treffer: Option<(Benutzer, String)> = conn
        .query_row(&sql, [benutzername], |z| Ok((zeile_zu_benutzer(z)?, z.get(9)?)))
        .optional()?;

    match treffer {
        Some((benutzer, hash)) if benutzer.hat_login && passwort_pruefen(passwort, &hash) => Ok(benutzer),
        _ => Err(AuthFehler::UngueltigeAnmeldung),
    }
}

/// Fuer den "Fuer wen?"-Auswahl beim Stunden-Import: Papa/Mama koennen
/// damit Stunden fuer eine bestimmte Mitarbeiterin nachtragen, statt nur
/// fuer sich selbst - schliesst auch eine Mitarbeiterin ohne eigenen
/// Login mit ein, genau die soll hier ja erscheinen.
pub fn alle_benutzer(conn: &Connection) -> Result<Vec<Benutzer>, AuthFehler> {
    let sql = format!("SELECT {BENUTZER_SPALTEN} FROM benutzer ORDER BY anzeigename");
    let mut stmt = conn.prepare(&sql)?;
    let zeilen = stmt.query_map([], zeile_zu_benutzer)?.collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

pub fn passwort_aendern(conn: &Connection, benutzer_id: i64, neues_passwort: &str) -> Result<(), AuthFehler> {
    let hash = passwort_hashen(neues_passwort)?;
    conn.execute("UPDATE benutzer SET passwort_hash = ?1 WHERE id = ?2", (hash, benutzer_id))?;
    Ok(())
}

/// "+ Mitarbeiterin anlegen": nimmt gleich das ganze Lohnprofil entgegen,
/// aber nie einen Login - Stefan braucht das nicht, er traegt ihre Stunden
/// selbst ein (ueber "Fuer wen?" beim Stunden-Import/-Erfassen). Benutzer-
/// name+Passwort werden rein intern generiert, nur um die UNIQUE/NOT
/// NULL-Vorgaben der Tabelle zu erfuellen - niemand bekommt sie je zu sehen.
pub fn mitarbeiterin_anlegen(conn: &Connection, eingabe: &NeueMitarbeiterin) -> Result<Benutzer, AuthFehler> {
    let anzeigename = eingabe.anzeigename.trim();
    let benutzername = zufallstext("mitarbeiterin");
    let hash = passwort_hashen(&zufallstext("pw"))?;

    let ergebnis = conn.execute(
        "INSERT INTO benutzer (benutzername, anzeigename, passwort_hash, rolle, strasse, plz_ort, ahv_nummer, stundenlohn, hat_login)
         VALUES (?1, ?2, ?3, 'mitarbeiterin', ?4, ?5, ?6, ?7, 0)",
        rusqlite::params![
            benutzername,
            anzeigename,
            hash,
            eingabe.strasse.trim(),
            eingabe.plz_ort.trim(),
            eingabe.ahv_nummer.trim(),
            eingabe.stundenlohn,
        ],
    );
    match ergebnis {
        Ok(_) => benutzer_holen(conn, conn.last_insert_rowid()),
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::ConstraintViolation => {
            Err(AuthFehler::BenutzernameBelegt)
        }
        Err(e) => Err(e.into()),
    }
}

/// Lohnprofil einer bestehenden Mitarbeiterin nachtraeglich anpassen (z.B.
/// Stundenlohn-Aenderung, Adresse korrigieren) - ohne das Konto neu
/// anlegen zu muessen.
pub fn mitarbeiterin_profil_aktualisieren(
    conn: &Connection,
    benutzer_id: i64,
    anzeigename: &str,
    strasse: &str,
    plz_ort: &str,
    ahv_nummer: &str,
    stundenlohn: Option<f64>,
) -> Result<Benutzer, AuthFehler> {
    conn.execute(
        "UPDATE benutzer SET anzeigename = ?1, strasse = ?2, plz_ort = ?3, ahv_nummer = ?4, stundenlohn = ?5 WHERE id = ?6",
        rusqlite::params![anzeigename.trim(), strasse.trim(), plz_ort.trim(), ahv_nummer.trim(), stundenlohn, benutzer_id],
    )?;
    benutzer_holen(conn, benutzer_id)
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
        crate::db::schema_fuer_tests_anlegen(&conn);

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
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    fn neue_mitarbeiterin(anzeigename: &str) -> NeueMitarbeiterin {
        NeueMitarbeiterin {
            anzeigename: anzeigename.into(),
            strasse: "".into(),
            plz_ort: "".into(),
            ahv_nummer: "".into(),
            stundenlohn: None,
        }
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

    // Stefans eigentlicher Wunsch: eine Mitarbeiterin anlegen, die sich nie
    // selbst einloggen soll ("kein Login gar nix") - bekommt trotzdem ein
    // vollstaendiges Lohnprofil und taucht in "alle_benutzer" auf (fuer
    // "Fuer wen?" bei Stunden), kann sich aber mit keinem Passwort anmelden.
    #[test]
    fn mitarbeiterin_ohne_login_bekommt_trotzdem_volles_lohnprofil() {
        let conn = test_db();
        let mut eingabe = neue_mitarbeiterin("Derensiya Kishokumar");
        eingabe.strasse = "Schützenstrasse 20".into();
        eingabe.plz_ort = "8808 Pfäffikon".into();
        eingabe.ahv_nummer = "756.1014.7360.86".into();
        eingabe.stundenlohn = Some(24.66);

        let angelegt = mitarbeiterin_anlegen(&conn, &eingabe).unwrap();
        assert!(!angelegt.hat_login);
        assert_eq!(angelegt.stundenlohn, Some(24.66));
        assert_eq!(angelegt.ahv_nummer, "756.1014.7360.86");

        // Mit irgendeinem Passwort gegen den intern generierten (unbekannten)
        // Benutzernamen anmelden ist unmoeglich - und selbst wenn jemand den
        // technischen Benutzernamen erraten wuerde, verhindert hat_login=0
        // den Login trotzdem.
        assert!(anmelden(&conn, &angelegt.benutzername, "irgendwas").is_err());

        let alle = alle_benutzer(&conn).unwrap();
        assert!(alle.iter().any(|b| b.id == angelegt.id), "muss fuer 'Fuer wen?' auftauchen");
    }

    #[test]
    fn lohnprofil_aktualisieren_aendert_nur_die_lohn_felder() {
        let conn = test_db();
        let angelegt = mitarbeiterin_anlegen(&conn, &neue_mitarbeiterin("Erika Muster")).unwrap();

        let aktualisiert =
            mitarbeiterin_profil_aktualisieren(&conn, angelegt.id, "Erika Muster", "Dorfstrasse 1", "8808 Pfäffikon", "756.1.2.3", Some(25.0))
                .unwrap();
        assert_eq!(aktualisiert.stundenlohn, Some(25.0));
        assert_eq!(aktualisiert.strasse, "Dorfstrasse 1");
        assert_eq!(aktualisiert.hat_login, angelegt.hat_login, "Login-Status bleibt unberuehrt");
    }
}
