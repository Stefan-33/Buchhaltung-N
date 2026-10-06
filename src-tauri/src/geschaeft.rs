// Kunden, Auftraege und die Monatsauswertung. Absichtlich keine
// gespeicherten Summenfelder wie "Jahresumsatz" oder "Total" - die werden
// bei jeder Abfrage aus den auftraege-Zeilen frisch zusammengezaehlt.
// Genau das ersetzt die fehleranfaelligen Datei-zu-Datei-Verknuepfungen
// aus dem bisherigen LibreOffice-Ablauf.

use rusqlite::{params, Connection, ErrorCode, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum GeschaeftFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Kunde wurde nicht gefunden")]
    KundeNichtGefunden,
    #[error("Ein Auftrag braucht mindestens eine Position")]
    KeinePosten,
    #[error("Dieser Kunde hat bereits Aufträge und kann darum nicht gelöscht werden - ohne neuen Besuch wird er automatisch archiviert")]
    KundeHatAuftraege,
    #[error("Keine Kundin mit dieser Nummer gefunden")]
    KundenNummerUnbekannt,
    #[error("Auftrag wurde nicht gefunden")]
    AuftragNichtGefunden,
    #[error("Dieser Auftrag ist bereits abgerechnet")]
    BereitsAbgerechnet,
    #[error("Dieser Auftrag ist noch nicht abgerechnet - bitte zuerst im Kundenblatt abrechnen")]
    NochNichtAbgerechnet,
    #[error("Unbekannter Status")]
    UngueltigerStatus,
    #[error("Unbekannte Zahlart")]
    UngueltigeZahlart,
    #[error("Ungültiges Abholdatum")]
    UngueltigesDatum,
}

pub const ZAHLARTEN: &[&str] = &["Bar", "Twint", "Karte", "Rechnung"];

/// Ablauf eines Auftrags von der Annahme bis zur Abholung. "Abgeholt"
/// heisst gleichzeitig: abgerechnet (Beleg erstellt) - erst ab dann
/// zaehlt der Betrag als Umsatz.
pub const STATUS_ABGEHOLT: &str = "Abgeholt";
pub const AUFTRAG_STATUS: &[&str] = &["Angenommen", "In Arbeit", "Abholbereit", STATUS_ABGEHOLT];

fn status_abgeholt() -> String {
    STATUS_ABGEHOLT.to_string()
}

fn ja() -> bool {
    true
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Kunde {
    pub id: i64,
    pub nummer: i64,
    pub name: String,
    pub vorname: String,
    pub telefon: String,
    pub ort: String,
    pub adresse: String,
    pub email: String,
    pub kartensatz: Option<f64>,
    pub archiviert: bool,
    pub notiz: String,
    // wird mitgeliefert, nicht gespeichert:
    pub jahresumsatz: f64,
    pub anzahl_auftraege: i64,
    pub letzter_besuch: Option<String>,
    // Summe aller noch nicht bezahlten Auftraege dieser Kundin.
    #[serde(default)]
    pub offen_summe: f64,
}

#[derive(Debug, Deserialize)]
pub struct NeuerKunde {
    // Nur beim Import gesetzt (z.B. um eine bestehende Kundennummer aus
    // einer alten Excel-Liste zu uebernehmen) - beim normalen "+ Neuer
    // Kunde"-Dialog nie mitgeschickt, daher "serde(default)" noetig.
    #[serde(default)]
    pub nummer: Option<i64>,
    pub name: String,
    pub vorname: String,
    pub telefon: String,
    pub ort: String,
    pub adresse: String,
    pub email: String,
    #[serde(default)]
    pub notiz: String,
}

/// Eingabe fuer "Kunde bearbeiten" - bewusst ohne "nummer" (bleibt fix,
/// einmal vergeben) und ohne "notiz" (hat hier kein eigenes Feld in der
/// Oberflaeche - wuerde sonst beim Speichern versehentlich geleert, z.B.
/// bei einem per Datei importierten Kunden mit Zusatz-Telefonnummer in
/// der Notiz).
#[derive(Debug, Deserialize)]
pub struct KundeBearbeiten {
    pub name: String,
    pub vorname: String,
    pub telefon: String,
    pub ort: String,
    pub adresse: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Posten {
    pub bezeichnung: String,
    pub stueck: f64,
    pub preis: f64,
}

#[derive(Debug, Deserialize)]
pub struct NeuerAuftrag {
    pub kunde_id: i64,
    pub zahlart: String,
    pub posten: Vec<Posten>,
}

// Auch Deserialize: die Oberflaeche schickt einen bereits geladenen
// Auftrag zurueck an Rust, um daraus die Quittung als PDF zu erzeugen
// (siehe quittung.rs) - ohne ihn ein zweites Mal aus der Datenbank holen
// zu muessen.
#[derive(Debug, Serialize, Deserialize)]
pub struct Auftrag {
    pub id: i64,
    pub rechnungsnummer: i64,
    pub datum: String,
    pub zahlart: String,
    pub summe: f64,
    pub posten: Vec<Posten>,
    #[serde(default = "status_abgeholt")]
    pub status: String,
    #[serde(default)]
    pub abholdatum: Option<String>,
    #[serde(default)]
    pub angenommen_am: Option<String>,
    #[serde(default = "ja")]
    pub bezahlt: bool,
    #[serde(default)]
    pub bezahlt_am: Option<String>,
}

const AUFTRAG_SPALTEN: &str =
    "id, rechnungsnummer, datum, zahlart, summe, status, abholdatum, angenommen_am, bezahlt, bezahlt_am";

/// Liest einen Auftrag ohne Posten (Spaltenreihenfolge wie AUFTRAG_SPALTEN).
fn zeile_zu_auftrag(z: &rusqlite::Row) -> rusqlite::Result<Auftrag> {
    Ok(Auftrag {
        id: z.get(0)?,
        rechnungsnummer: z.get(1)?,
        datum: z.get(2)?,
        zahlart: z.get(3)?,
        summe: z.get(4)?,
        posten: Vec::new(),
        status: z.get(5)?,
        abholdatum: z.get(6)?,
        angenommen_am: z.get(7)?,
        bezahlt: z.get::<_, i64>(8)? != 0,
        bezahlt_am: z.get(9)?,
    })
}

pub(crate) fn posten_holen(conn: &Connection, auftrag_id: i64) -> rusqlite::Result<Vec<Posten>> {
    let mut stmt = conn.prepare("SELECT bezeichnung, stueck, preis FROM auftrag_posten WHERE auftrag_id = ?1 ORDER BY id")?;
    let posten = stmt
        .query_map([auftrag_id], |z| Ok(Posten { bezeichnung: z.get(0)?, stueck: z.get(1)?, preis: z.get(2)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(posten)
}

pub(crate) fn auftrag_holen(conn: &Connection, auftrag_id: i64) -> Result<Auftrag, GeschaeftFehler> {
    let sql = format!("SELECT {AUFTRAG_SPALTEN} FROM auftraege WHERE id = ?1");
    let mut auftrag = conn
        .query_row(&sql, [auftrag_id], zeile_zu_auftrag)
        .optional()?
        .ok_or(GeschaeftFehler::AuftragNichtGefunden)?;
    auftrag.posten = posten_holen(conn, auftrag_id)?;
    Ok(auftrag)
}

pub(crate) fn naechster_zaehler(conn: &Connection, schluessel: &str, start: i64) -> rusqlite::Result<i64> {
    // Ein Zaehler in einer eigenen kleinen Tabelle statt MAX(nummer)+1 auf
    // der Kunden-/Auftragstabelle - so bleibt die Nummer stabil, auch wenn
    // irgendwann mal ein Testkunde geloescht wird und eine Luecke entsteht.
    conn.execute(
        "INSERT INTO einstellungen (schluessel, wert) VALUES (?1, ?2)
         ON CONFLICT(schluessel) DO UPDATE SET wert = CAST(wert AS INTEGER) + 1",
        params![schluessel, start.to_string()],
    )?;
    let wert: String = conn.query_row(
        "SELECT wert FROM einstellungen WHERE schluessel = ?1",
        [schluessel],
        |z| z.get(0),
    )?;
    Ok(wert.parse().unwrap_or(start))
}

pub fn kunde_anlegen(conn: &Connection, eingabe: NeuerKunde) -> Result<Kunde, GeschaeftFehler> {
    let nummer = naechster_zaehler(conn, "naechste_kundennummer", 101)?;
    conn.execute(
        "INSERT INTO kunden (nummer, name, vorname, telefon, ort, adresse, email, notiz)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![nummer, eingabe.name, eingabe.vorname, eingabe.telefon, eingabe.ort, eingabe.adresse, eingabe.email, eingabe.notiz],
    )?;
    let id = conn.last_insert_rowid();
    kunde_holen(conn, id)
}

/// Masseneinspielung aus Excel/LibreOffice - die Oberflaeche hat die
/// eingefuegte Tabelle bereits in NeuerKunde-Zeilen zerlegt. Laeuft in
/// einer einzigen Transaktion: entweder kommt alles rein, oder bei einem
/// echten Fehler nichts, statt einer halbfertigen Liste.
///
/// Ist bei einer Zeile eine Nummer mitgegeben (z.B. aus einer bestehenden
/// Telefonliste mit eigenen Kundennummern), wird genau diese verwendet
/// statt automatisch eine neue zu vergeben - praktisch fuer eine einmalige
/// Uebernahme, bei der die alten Nummern erhalten bleiben sollen. Der
/// laufende Zaehler fuer kuenftige, manuell angelegte Kunden wird danach
/// auf die hoechste uebernommene Nummer + 1 angehoben, damit es keine
/// Kollision gibt.
pub fn kunden_importieren(conn: &mut Connection, eingaben: Vec<NeuerKunde>) -> Result<KundenImport, GeschaeftFehler> {
    let tx = conn.transaction()?;
    let mut ergebnis_zaehler = KundenImport { neu: 0, doppelt: 0 };
    let mut hoechste_uebernommene_nummer: Option<i64> = None;

    // Wer schon da ist (gleicher Name, Vorname und Telefon), wird nicht ein
    // zweites Mal angelegt - so kann dieselbe Liste gefahrlos nochmals
    // importiert werden, z.B. nachdem sie um neue Kundinnen ergaenzt wurde.
    let mut bekannt: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let mut stmt = tx.prepare("SELECT name, vorname, telefon FROM kunden")?;
        let zeilen = stmt.query_map([], |z| Ok((z.get::<_, String>(0)?, z.get::<_, String>(1)?, z.get::<_, String>(2)?)))?;
        for z in zeilen {
            let (n, v, t) = z?;
            bekannt.insert(kunden_schluessel(&n, &v, &t));
        }
    }

    for eingabe in eingaben {
        let name = eingabe.name.trim();
        if name.is_empty() {
            continue; // Zeile ohne Namen ueberspringen statt den ganzen Import abzubrechen
        }
        let schluessel = kunden_schluessel(name, &eingabe.vorname, &eingabe.telefon);
        if bekannt.contains(&schluessel) {
            ergebnis_zaehler.doppelt += 1;
            continue;
        }
        let nummer = match eingabe.nummer {
            Some(n) => n,
            None => naechster_zaehler(&tx, "naechste_kundennummer", 101)?,
        };
        let ergebnis = tx.execute(
            "INSERT INTO kunden (nummer, name, vorname, telefon, ort, adresse, email, notiz)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                nummer,
                name,
                eingabe.vorname.trim(),
                eingabe.telefon.trim(),
                eingabe.ort.trim(),
                eingabe.adresse.trim(),
                eingabe.email.trim(),
                eingabe.notiz.trim(),
            ],
        );
        match ergebnis {
            Ok(_) => {
                ergebnis_zaehler.neu += 1;
                bekannt.insert(schluessel);
                if eingabe.nummer.is_some() {
                    hoechste_uebernommene_nummer =
                        Some(hoechste_uebernommene_nummer.map_or(nummer, |bisher| bisher.max(nummer)));
                }
            }
            // "nummer" ist UNIQUE - eine doppelt vorkommende oder bereits
            // vergebene Nummer soll diese eine Zeile ueberspringen statt
            // den ganzen Import abzubrechen.
            Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::ConstraintViolation => {
                ergebnis_zaehler.doppelt += 1;
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }

    if let Some(hoechste) = hoechste_uebernommene_nummer {
        tx.execute(
            "INSERT INTO einstellungen (schluessel, wert) VALUES ('naechste_kundennummer', ?1)
             ON CONFLICT(schluessel) DO UPDATE SET wert = ?1 WHERE CAST(wert AS INTEGER) < ?1",
            params![hoechste.to_string()],
        )?;
    }

    tx.commit()?;
    Ok(ergebnis_zaehler)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct KundenImport {
    pub neu: usize,
    /// Schon vorhanden (gleiche Person oder Kundennummer schon vergeben).
    pub doppelt: usize,
}

/// Name + Vorname (ohne Gross/Klein, Leerzeichen) + nur die Ziffern der
/// Telefonnummer - "079 123 45 67" und "0791234567" gelten als gleich.
fn kunden_schluessel(name: &str, vorname: &str, telefon: &str) -> String {
    let ziffern: String = telefon.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("{}|{}|{}", name.trim().to_lowercase(), vorname.trim().to_lowercase(), ziffern)
}

const KUNDE_MIT_KENNZAHLEN_SQL: &str = r#"
    SELECT
        k.id, k.nummer, k.name, k.vorname, k.telefon, k.ort, k.adresse,
        k.email, k.kartensatz, k.archiviert, k.notiz,
        COALESCE((
            SELECT SUM(a.summe) FROM auftraege a
            WHERE a.kunde_id = k.id AND a.status = 'Abgeholt'
              AND strftime('%Y', a.datum) = strftime('%Y', 'now')
        ), 0) AS jahresumsatz,
        (SELECT COUNT(*) FROM auftraege a WHERE a.kunde_id = k.id) AS anzahl_auftraege,
        (SELECT MAX(a.datum) FROM auftraege a WHERE a.kunde_id = k.id) AS letzter_besuch,
        COALESCE((
            SELECT SUM(a.summe) FROM auftraege a WHERE a.kunde_id = k.id AND a.bezahlt = 0
        ), 0) AS offen_summe
    FROM kunden k
"#;

fn zeile_zu_kunde(z: &rusqlite::Row) -> rusqlite::Result<Kunde> {
    Ok(Kunde {
        id: z.get(0)?,
        nummer: z.get(1)?,
        name: z.get(2)?,
        vorname: z.get(3)?,
        telefon: z.get(4)?,
        ort: z.get(5)?,
        adresse: z.get(6)?,
        email: z.get(7)?,
        kartensatz: z.get(8)?,
        archiviert: z.get::<_, i64>(9)? != 0,
        notiz: z.get(10)?,
        jahresumsatz: z.get(11)?,
        anzahl_auftraege: z.get(12)?,
        letzter_besuch: z.get(13)?,
        offen_summe: z.get(14)?,
    })
}

pub fn kunde_holen(conn: &Connection, id: i64) -> Result<Kunde, GeschaeftFehler> {
    let sql = format!("{KUNDE_MIT_KENNZAHLEN_SQL} WHERE k.id = ?1");
    conn.query_row(&sql, [id], zeile_zu_kunde)
        .optional()?
        .ok_or(GeschaeftFehler::KundeNichtGefunden)
}

/// Fuer "Neuer Auftrag" im Reiter Auftraege: die Kundin direkt ueber ihre
/// Kundennummer finden (die Nummer, die auch auf Auftrag und Rechnung steht).
pub fn kunde_nach_nummer(conn: &Connection, nummer: i64) -> Result<Kunde, GeschaeftFehler> {
    let sql = format!("{KUNDE_MIT_KENNZAHLEN_SQL} WHERE k.nummer = ?1");
    conn.query_row(&sql, [nummer], zeile_zu_kunde)
        .optional()?
        .ok_or(GeschaeftFehler::KundenNummerUnbekannt)
}

/// Suche wie in der Skizze: Name, Ort und Telefon gleichzeitig, ein
/// Jahr ohne Besuch faellt automatisch ins Archiv, bleibt dort aber
/// jederzeit ueber den Haken "Archiv mitzeigen" auffindbar.
pub fn kunden_suchen(conn: &Connection, suchtext: &str, archiv_zeigen: bool) -> Result<Vec<Kunde>, GeschaeftFehler> {
    let muster = format!("%{}%", suchtext.trim());
    let sql = format!(
        "{KUNDE_MIT_KENNZAHLEN_SQL}
         WHERE (?1 = '' OR k.name LIKE ?2 OR k.vorname LIKE ?2 OR k.ort LIKE ?2 OR k.telefon LIKE ?2)
           AND (?3 = 1 OR k.archiviert = 0)
         ORDER BY k.nummer"
    );
    let mut stmt = conn.prepare(&sql)?;
    let zeilen = stmt
        .query_map(params![suchtext.trim(), muster, archiv_zeigen as i64], zeile_zu_kunde)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

pub fn kartensatz_setzen(conn: &Connection, kunde_id: i64, kartensatz: Option<f64>) -> Result<(), GeschaeftFehler> {
    conn.execute("UPDATE kunden SET kartensatz = ?1 WHERE id = ?2", params![kartensatz, kunde_id])?;
    Ok(())
}

/// Stammdaten eines bestehenden Kunden korrigieren - z.B. wenn beim
/// Datei-Import (siehe kunden_importieren) im Vorname-Feld statt eines
/// echten Vornamens "Herr"/"Frau" aus der Ursprungs-Excel gelandet ist.
pub fn kunde_aktualisieren(conn: &Connection, kunde_id: i64, eingabe: KundeBearbeiten) -> Result<Kunde, GeschaeftFehler> {
    conn.execute(
        "UPDATE kunden SET name = ?1, vorname = ?2, telefon = ?3, ort = ?4, adresse = ?5, email = ?6 WHERE id = ?7",
        params![
            eingabe.name.trim(),
            eingabe.vorname.trim(),
            eingabe.telefon.trim(),
            eingabe.ort.trim(),
            eingabe.adresse.trim(),
            eingabe.email.trim(),
            kunde_id,
        ],
    )?;
    kunde_holen(conn, kunde_id)
}

/// Loescht einen Kunden endgueltig - aber nur, wenn er noch keine
/// Auftraege hat (die Fremdschluessel-Vorgabe in db.rs verhindert das
/// sonst ohnehin, hier nur mit einer verstaendlichen Meldung statt der
/// rohen SQLite-Fehlermeldung). Gedacht fuer Karteileichen, z.B. ein aus
/// Versehen zweimal angelegter oder beim Telefonlisten-Import falsch
/// erkannter Kunde - sobald wirklich ein Auftrag dabei ist, kommt
/// stattdessen die automatische Archivierung (archiv_aktualisieren) zum
/// Zug, damit die Buchhaltung lueckenlos bleibt.
pub fn kunde_loeschen(conn: &Connection, kunde_id: i64) -> Result<(), GeschaeftFehler> {
    let betroffen = conn.execute("DELETE FROM kunden WHERE id = ?1", [kunde_id]);
    match betroffen {
        Ok(0) => Err(GeschaeftFehler::KundeNichtGefunden),
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == ErrorCode::ConstraintViolation => {
            Err(GeschaeftFehler::KundeHatAuftraege)
        }
        Err(e) => Err(e.into()),
    }
}

/// Ein Jahr ohne Besuch -> automatisch archiviert. Wird beim Programmstart
/// und nach jedem neuen Auftrag aufgerufen, statt dass jemand das von Hand
/// pflegen muesste.
pub fn archiv_aktualisieren(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE kunden SET archiviert = CASE
            WHEN (SELECT MAX(a.datum) FROM auftraege a WHERE a.kunde_id = kunden.id) IS NULL
                THEN archiviert
            WHEN julianday('now') - julianday((SELECT MAX(a.datum) FROM auftraege a WHERE a.kunde_id = kunden.id)) > 365
                THEN 1
            ELSE 0
         END",
        [],
    )?;
    Ok(())
}

pub(crate) fn zahlart_pruefen(zahlart: &str) -> Result<(), GeschaeftFehler> {
    if ZAHLARTEN.contains(&zahlart) {
        Ok(())
    } else {
        Err(GeschaeftFehler::UngueltigeZahlart)
    }
}

pub(crate) fn posten_einfuegen(conn: &Connection, auftrag_id: i64, posten: &[Posten]) -> rusqlite::Result<()> {
    for p in posten {
        conn.execute(
            "INSERT INTO auftrag_posten (auftrag_id, bezeichnung, stueck, preis) VALUES (?1, ?2, ?3, ?4)",
            params![auftrag_id, p.bezeichnung.trim(), p.stueck, p.preis],
        )?;
    }
    Ok(())
}

/// Sofort-Ablauf im Kundenblatt: Auftrag erfassen und gleich abrechnen
/// (Kundin zahlt sofort bzw. bekommt eine Rechnung). Mit Zahlart
/// "Rechnung" bleibt er als offener Posten stehen, bis er als bezahlt
/// markiert wird.
pub fn auftrag_anlegen(conn: &mut Connection, eingabe: NeuerAuftrag) -> Result<Auftrag, GeschaeftFehler> {
    if eingabe.posten.is_empty() {
        return Err(GeschaeftFehler::KeinePosten);
    }
    zahlart_pruefen(&eingabe.zahlart)?;
    let summe: f64 = eingabe.posten.iter().map(|p| p.stueck * p.preis).sum();
    let rechnungsnummer = naechster_zaehler(conn, "naechste_rechnungsnummer", 1259)?;
    let bezahlt = eingabe.zahlart != "Rechnung";

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO auftraege (kunde_id, rechnungsnummer, zahlart, summe, status, bezahlt, bezahlt_am)
         VALUES (?1, ?2, ?3, ?4, 'Abgeholt', ?5, CASE WHEN ?5 = 1 THEN date('now') END)",
        params![eingabe.kunde_id, rechnungsnummer, eingabe.zahlart, summe, bezahlt as i64],
    )?;
    let auftrag_id = tx.last_insert_rowid();
    posten_einfuegen(&tx, auftrag_id, &eingabe.posten)?;
    tx.commit()?;
    archiv_aktualisieren(conn)?;

    auftrag_holen(conn, auftrag_id)
}

pub fn auftraege_von_kunde(conn: &Connection, kunde_id: i64) -> Result<Vec<Auftrag>, GeschaeftFehler> {
    let sql = format!("SELECT {AUFTRAG_SPALTEN} FROM auftraege WHERE kunde_id = ?1 ORDER BY datum DESC, id DESC");
    let mut stmt = conn.prepare(&sql)?;
    let auftraege = stmt.query_map([kunde_id], zeile_zu_auftrag)?.collect::<Result<Vec<_>, _>>()?;

    let mut ergebnis = Vec::with_capacity(auftraege.len());
    for mut auftrag in auftraege {
        auftrag.posten = posten_holen(conn, auftrag.id)?;
        ergebnis.push(auftrag);
    }
    Ok(ergebnis)
}

#[derive(Debug, Serialize)]
pub struct MonatsZeile {
    pub monat: u32,
    pub bar: f64,
    pub twint: f64,
    pub karte: f64,
    pub rechnung: f64,
    pub anzahl_kunden: i64,
}

/// Genau die Auswertung, die Papa jeden Monat von Hand zusammensucht:
/// Bar/Twint/Karte/Rechnung und wie viele Kunden es waren.
pub fn monatsstatistik(conn: &Connection, jahr: i32) -> Result<Vec<MonatsZeile>, GeschaeftFehler> {
    let mut stmt = conn.prepare(
        "SELECT
            CAST(strftime('%m', datum) AS INTEGER) AS monat,
            COALESCE(SUM(CASE WHEN zahlart = 'Bar' THEN summe END), 0),
            COALESCE(SUM(CASE WHEN zahlart = 'Twint' THEN summe END), 0),
            COALESCE(SUM(CASE WHEN zahlart = 'Karte' THEN summe END), 0),
            COALESCE(SUM(CASE WHEN zahlart = 'Rechnung' THEN summe END), 0),
            COUNT(DISTINCT kunde_id)
         FROM auftraege
         WHERE strftime('%Y', datum) = ?1 AND status = 'Abgeholt'
         GROUP BY monat
         ORDER BY monat",
    )?;
    let zeilen = stmt
        .query_map([jahr.to_string()], |z| {
            Ok(MonatsZeile {
                monat: z.get(0)?,
                bar: z.get(1)?,
                twint: z.get(2)?,
                karte: z.get(3)?,
                rechnung: z.get(4)?,
                anzahl_kunden: z.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        // db::verbinden() braucht einen echten Dateipfad (%APPDATA%) - fuer
        // einen Test reicht eine In-Memory-Datenbank mit demselben Schema.
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    fn neuer_kunde(name: &str, vorname: &str) -> NeuerKunde {
        NeuerKunde {
            nummer: None,
            name: name.into(),
            vorname: vorname.into(),
            telefon: "".into(),
            ort: "".into(),
            adresse: "".into(),
            email: "".into(),
            notiz: "".into(),
        }
    }

    // Stefans Anfrage nach dem Telefonlisten-Import: ein Vorname-Feld wie
    // "Herr"/"Frau" (so stand es in seiner Original-Excel) muss sich im
    // Nachhinein korrigieren lassen - und darf dabei die per Import gesetzte
    // Notiz (hier: eine zweite Telefonnummer) nicht versehentlich leeren,
    // weil der Bearbeiten-Dialog dafuer kein eigenes Feld hat.
    #[test]
    fn kunde_aktualisieren_aendert_stammdaten_ohne_notiz_zu_loeschen() {
        let conn = test_db();
        let mut eingabe = neuer_kunde("Muster", "Herr");
        eingabe.notiz = "Privat: 044 123 45 67".into();
        let angelegt = kunde_anlegen(&conn, eingabe).unwrap();

        let aktualisiert = kunde_aktualisieren(
            &conn,
            angelegt.id,
            KundeBearbeiten {
                name: "Muster".into(),
                vorname: "Hans".into(),
                telefon: angelegt.telefon.clone(),
                ort: "Pfäffikon".into(),
                adresse: angelegt.adresse.clone(),
                email: angelegt.email.clone(),
            },
        )
        .unwrap();

        assert_eq!(aktualisiert.vorname, "Hans");
        assert_eq!(aktualisiert.ort, "Pfäffikon");
        assert_eq!(aktualisiert.nummer, angelegt.nummer, "Kundennummer bleibt unveraendert");
        assert_eq!(aktualisiert.notiz, "Privat: 044 123 45 67", "Notiz darf beim Bearbeiten nicht verloren gehen");
    }

    // Stefans Wunsch nach dem Telefonlisten-Import: eine Karteileiche (z.B.
    // versehentlich doppelt angelegt) soll sich wieder loeschen lassen.
    #[test]
    fn kunde_ohne_auftraege_laesst_sich_loeschen() {
        let conn = test_db();
        let angelegt = kunde_anlegen(&conn, neuer_kunde("Testweise", "")).unwrap();

        kunde_loeschen(&conn, angelegt.id).unwrap();

        assert!(matches!(kunde_holen(&conn, angelegt.id), Err(GeschaeftFehler::KundeNichtGefunden)));
    }

    // Ein Kunde mit echten Auftraegen darf nicht einfach verschwinden -
    // das waere ein Loch in der Buchhaltung. Stattdessen die bestehende
    // automatische Archivierung nutzen.
    #[test]
    fn kunde_mit_auftraegen_kann_nicht_geloescht_werden() {
        let mut conn = test_db();
        let angelegt = kunde_anlegen(&conn, neuer_kunde("Kundin", "Mit Auftrag")).unwrap();
        auftrag_anlegen(
            &mut conn,
            NeuerAuftrag { kunde_id: angelegt.id, zahlart: "Bar".into(), posten: vec![Posten { bezeichnung: "Kürzen".into(), stueck: 1.0, preis: 20.0 }] },
        )
        .unwrap();

        let ergebnis = kunde_loeschen(&conn, angelegt.id);
        assert!(matches!(ergebnis, Err(GeschaeftFehler::KundeHatAuftraege)));
        // Kunde muss unangetastet weiterbestehen.
        assert!(kunde_holen(&conn, angelegt.id).is_ok());
    }

    #[test]
    fn nicht_vorhandenen_kunden_loeschen_gibt_klare_meldung() {
        let conn = test_db();
        assert!(matches!(kunde_loeschen(&conn, 99999), Err(GeschaeftFehler::KundeNichtGefunden)));
    }

    // Genau der Fall aus Stefans Anfrage: eine aus Excel/LibreOffice
    // eingefuegte Tabelle, in der Oberflaeche bereits in NeuerKunde-Zeilen
    // zerlegt. Eine Zeile ohne Namen (z.B. eine leere Excel-Zeile) muss
    // uebersprungen werden statt den ganzen Import abzubrechen, und die
    // fortlaufende Kundennummer muss trotzdem bei 101 beginnen.
    #[test]
    fn kunden_import_ueberspringt_namenlose_zeilen_und_vergibt_nummern() {
        let mut conn = test_db();
        let eingaben = vec![
            NeuerKunde { telefon: "0791234567".into(), ort: "Wollerau".into(), adresse: "Seestrasse 1".into(), email: "hans@meier.ch".into(), ..neuer_kunde("Meier", "Hans") },
            // Leere Excel-Zeile - darf nicht als Kunde "Niemand" landen.
            neuer_kunde("", ""),
            NeuerKunde { ort: "Freienbach".into(), ..neuer_kunde("  Keller  ", "Anna") },
        ];

        let anzahl = kunden_importieren(&mut conn, eingaben).unwrap().neu;
        assert_eq!(anzahl, 2, "die namenlose Zeile darf nicht mitgezaehlt werden");

        let kunden = kunden_suchen(&conn, "", false).unwrap();
        assert_eq!(kunden.len(), 2);
        assert_eq!(kunden[0].nummer, 101);
        assert_eq!(kunden[0].name, "Meier");
        assert_eq!(kunden[1].nummer, 102);
        assert_eq!(kunden[1].name, "Keller", "fuehrende/folgende Leerzeichen muessen getrimmt sein");
    }

    // Stefans Telefonliste hat eigene Kundennummern, die uebernommen werden
    // sollen (siehe Chat) - und ein danach manuell angelegter Kunde darf
    // nicht mit einer dieser Nummern kollidieren.
    #[test]
    fn kunden_import_uebernimmt_vorgegebene_nummern_und_hebt_zaehler_an() {
        let mut conn = test_db();
        let eingaben = vec![
            NeuerKunde { nummer: Some(234), ..neuer_kunde("Arnold", "Yvonne") },
            NeuerKunde { nummer: Some(987), ..neuer_kunde("Zbinden", "Peter") },
            // Kein nummer -> automatisch vergeben, unabhaengig von den obigen.
            neuer_kunde("Ohne Nummer", ""),
        ];
        let anzahl = kunden_importieren(&mut conn, eingaben).unwrap().neu;
        assert_eq!(anzahl, 3);

        let kunden = kunden_suchen(&conn, "", false).unwrap();
        let nummern: Vec<i64> = kunden.iter().map(|k| k.nummer).collect();
        assert!(nummern.contains(&234));
        assert!(nummern.contains(&987));

        // Naechster manuell angelegter Kunde darf nicht mit 987 kollidieren.
        let manuell = kunde_anlegen(&conn, neuer_kunde("Neu", "")).unwrap();
        assert!(manuell.nummer > 987, "Zaehler haette auf > 987 angehoben werden muessen, war aber {}", manuell.nummer);
    }

    // Zwei Zeilen mit derselben (bereits vergebenen) Nummer duerfen den
    // Import nicht komplett abbrechen - nur die kollidierende Zeile faellt
    // raus.
    #[test]
    fn kunden_import_ueberspringt_doppelte_nummer_statt_abzubrechen() {
        let mut conn = test_db();
        kunden_importieren(&mut conn, vec![NeuerKunde { nummer: Some(500), ..neuer_kunde("Erste", "") }]).unwrap();

        let anzahl = kunden_importieren(
            &mut conn,
            vec![
                NeuerKunde { nummer: Some(500), ..neuer_kunde("Kollidiert", "") },
                NeuerKunde { nummer: Some(501), ..neuer_kunde("Geht durch", "") },
            ],
        )
        .unwrap();
        assert_eq!(anzahl, KundenImport { neu: 1, doppelt: 1 }, "nur die Zeile mit der neuen Nummer 501 zaehlt");

        let kunden = kunden_suchen(&conn, "", false).unwrap();
        assert_eq!(kunden.len(), 2); // "Erste" (500) + "Geht durch" (501), nicht "Kollidiert"
    }

    // Dieselbe Kundenliste nochmals importiert (inzwischen mit einer neuen
    // Kundin): niemand wird doppelt angelegt, auch wenn die Telefonnummer
    // anders geschrieben ist.
    #[test]
    fn erneuter_kunden_import_legt_niemanden_doppelt_an() {
        let mut conn = test_db();
        let liste = || vec![NeuerKunde { telefon: "079 123 45 67".into(), ..neuer_kunde("Meier", "Anna") }, neuer_kunde("Keller", "Beat")];
        assert_eq!(kunden_importieren(&mut conn, liste()).unwrap(), KundenImport { neu: 2, doppelt: 0 });
        let mut zweite = liste();
        zweite[0].telefon = "0791234567".into();
        zweite[1].name = "keller".into();
        zweite.push(neuer_kunde("Neu", "Nina"));
        assert_eq!(kunden_importieren(&mut conn, zweite).unwrap(), KundenImport { neu: 1, doppelt: 2 });
        // Gleicher Name, andere Telefonnummer = andere Person.
        let andere = vec![NeuerKunde { telefon: "055 000 00 00".into(), ..neuer_kunde("Meier", "Anna") }];
        assert_eq!(kunden_importieren(&mut conn, andere).unwrap().neu, 1);
        assert_eq!(kunden_suchen(&conn, "", false).unwrap().len(), 4);
    }
}
