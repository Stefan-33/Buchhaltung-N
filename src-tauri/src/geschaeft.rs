// Kunden, Auftraege und die Monatsauswertung. Absichtlich keine
// gespeicherten Summenfelder wie "Jahresumsatz" oder "Total" - die werden
// bei jeder Abfrage aus den auftraege-Zeilen frisch zusammengezaehlt.
// Genau das ersetzt die fehleranfaelligen Datei-zu-Datei-Verknuepfungen
// aus dem bisherigen LibreOffice-Ablauf.

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum GeschaeftFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Kunde wurde nicht gefunden")]
    KundeNichtGefunden,
    #[error("Ein Auftrag braucht mindestens eine Position")]
    KeinePosten,
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
}

#[derive(Debug, Deserialize)]
pub struct NeuerKunde {
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

#[derive(Debug, Serialize)]
pub struct Auftrag {
    pub id: i64,
    pub rechnungsnummer: i64,
    pub datum: String,
    pub zahlart: String,
    pub summe: f64,
    pub posten: Vec<Posten>,
}

fn naechster_zaehler(conn: &Connection, schluessel: &str, start: i64) -> rusqlite::Result<i64> {
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
        "INSERT INTO kunden (nummer, name, vorname, telefon, ort, adresse, email)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![nummer, eingabe.name, eingabe.vorname, eingabe.telefon, eingabe.ort, eingabe.adresse, eingabe.email],
    )?;
    let id = conn.last_insert_rowid();
    kunde_holen(conn, id)
}

const KUNDE_MIT_KENNZAHLEN_SQL: &str = r#"
    SELECT
        k.id, k.nummer, k.name, k.vorname, k.telefon, k.ort, k.adresse,
        k.email, k.kartensatz, k.archiviert, k.notiz,
        COALESCE((
            SELECT SUM(a.summe) FROM auftraege a
            WHERE a.kunde_id = k.id AND strftime('%Y', a.datum) = strftime('%Y', 'now')
        ), 0) AS jahresumsatz,
        (SELECT COUNT(*) FROM auftraege a WHERE a.kunde_id = k.id) AS anzahl_auftraege,
        (SELECT MAX(a.datum) FROM auftraege a WHERE a.kunde_id = k.id) AS letzter_besuch
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
    })
}

pub fn kunde_holen(conn: &Connection, id: i64) -> Result<Kunde, GeschaeftFehler> {
    let sql = format!("{KUNDE_MIT_KENNZAHLEN_SQL} WHERE k.id = ?1");
    conn.query_row(&sql, [id], zeile_zu_kunde)
        .optional()?
        .ok_or(GeschaeftFehler::KundeNichtGefunden)
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

pub fn auftrag_anlegen(conn: &mut Connection, eingabe: NeuerAuftrag) -> Result<Auftrag, GeschaeftFehler> {
    if eingabe.posten.is_empty() {
        return Err(GeschaeftFehler::KeinePosten);
    }
    let summe: f64 = eingabe.posten.iter().map(|p| p.stueck * p.preis).sum();
    let rechnungsnummer = naechster_zaehler(conn, "naechste_rechnungsnummer", 1259)?;

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO auftraege (kunde_id, rechnungsnummer, zahlart, summe) VALUES (?1, ?2, ?3, ?4)",
        params![eingabe.kunde_id, rechnungsnummer, eingabe.zahlart, summe],
    )?;
    let auftrag_id = tx.last_insert_rowid();
    for posten in &eingabe.posten {
        tx.execute(
            "INSERT INTO auftrag_posten (auftrag_id, bezeichnung, stueck, preis) VALUES (?1, ?2, ?3, ?4)",
            params![auftrag_id, posten.bezeichnung, posten.stueck, posten.preis],
        )?;
    }
    tx.commit()?;
    archiv_aktualisieren(conn)?;

    let datum: String = conn.query_row("SELECT datum FROM auftraege WHERE id = ?1", [auftrag_id], |z| z.get(0))?;
    Ok(Auftrag { id: auftrag_id, rechnungsnummer, datum, zahlart: eingabe.zahlart, summe, posten: eingabe.posten })
}

pub fn auftraege_von_kunde(conn: &Connection, kunde_id: i64) -> Result<Vec<Auftrag>, GeschaeftFehler> {
    let mut stmt = conn.prepare(
        "SELECT id, rechnungsnummer, datum, zahlart, summe FROM auftraege
         WHERE kunde_id = ?1 ORDER BY datum DESC, id DESC",
    )?;
    let auftraege = stmt
        .query_map([kunde_id], |z| {
            Ok(Auftrag {
                id: z.get(0)?,
                rechnungsnummer: z.get(1)?,
                datum: z.get(2)?,
                zahlart: z.get(3)?,
                summe: z.get(4)?,
                posten: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut ergebnis = Vec::with_capacity(auftraege.len());
    for mut auftrag in auftraege {
        let mut stmt = conn.prepare("SELECT bezeichnung, stueck, preis FROM auftrag_posten WHERE auftrag_id = ?1")?;
        auftrag.posten = stmt
            .query_map([auftrag.id], |z| Ok(Posten { bezeichnung: z.get(0)?, stueck: z.get(1)?, preis: z.get(2)? }))?
            .collect::<Result<Vec<_>, _>>()?;
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
         WHERE strftime('%Y', datum) = ?1
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
