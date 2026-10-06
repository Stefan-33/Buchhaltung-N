// Treuhand: der jaehrliche Einnahmen/Ausgaben-Bericht fuer den Treuhaender
// (ersetzt Stefans bisherige "Treuhand - Umsatz"-Excel) und die monatliche
// Lohnabrechnung einer Mitarbeiterin (ersetzt "Lohnabrechnung Stundenlohn
// laufend"-Excel). Die Einnahmen-Seite gibt es durch die Auftraege
// (geschaeft.rs) schon - hier kommt nur das bisher fehlende Stueck dazu:
// Geschaeftsausgaben erfassen und beides zu einem Bericht zusammenfuehren.

use crate::sicherung::{csv_feld, schreiben, sicherungs_ordner};
use rusqlite::{params, Connection, Row};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum TreuhandFehler {
    #[error("Datenbankfehler: {0}")]
    Datenbank(#[from] rusqlite::Error),
    #[error("Bitte einen Betrag grösser als 0 eingeben")]
    UngueltigerBetrag,
    #[error("Unbekannte Kategorie")]
    UnbekannteKategorie,
    #[error("Beleg konnte nicht gespeichert werden: {0}")]
    Beleg(std::io::Error),
}

// Feste Kategorie-Liste, 1:1 aus Stefans bisheriger Treuhand-Excel
// uebernommen (siehe Rueckfrage dazu: "genau diese Liste fest
// übernehmen"). "Mitarbeiterin" ist hier bewusst eine ganz normale,
// manuell erfasste Ausgabe - KEINE automatische Berechnung aus den
// Stunden (ebenfalls Stefans Entscheid).
pub const AUSGABEN_KATEGORIEN: &[&str] = &[
    "Kleinmaterial / Atelier",
    "Büromaterialien",
    "Werbung",
    "Einrichten / Investition",
    "Telefon",
    "Versicherung",
    "Reparaturen / Service Arbeitsgeräte",
    "Miete / Strom",
    "Auto",
    "AHV",
    "Mitarbeiterin",
];

#[derive(Debug, Deserialize)]
pub struct NeueAusgabe {
    pub datum: String,
    pub kategorie: String,
    pub betrag: f64,
    #[serde(default)]
    pub notiz: String,
    // Pfad der vom nativen Dateidialog ausgewaehlten Beleg-Datei (Foto/
    // Scan/PDF der Quittung) - wird beim Erfassen in den Sicherungsordner
    // kopiert, damit sie nicht verloren geht, falls Stefan die
    // Original-Datei spaeter verschiebt oder loescht.
    #[serde(default)]
    pub beleg_quelle: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Ausgabe {
    pub id: i64,
    pub datum: String,
    pub kategorie: String,
    pub betrag: f64,
    pub notiz: String,
    pub beleg_pfad: Option<String>,
}

fn zeile_zu_ausgabe(z: &Row) -> rusqlite::Result<Ausgabe> {
    Ok(Ausgabe {
        id: z.get(0)?,
        datum: z.get(1)?,
        kategorie: z.get(2)?,
        betrag: z.get(3)?,
        notiz: z.get(4)?,
        beleg_pfad: z.get(5)?,
    })
}

const AUSGABE_SPALTEN: &str = "id, datum, kategorie, betrag, notiz, beleg_pfad";

/// Kopiert eine Beleg-Datei (vom nativen Dateidialog ausgewaehlt) in den
/// Sicherungsordner, benannt nach der Ausgabe-Id - damit zwei Belege nie
/// denselben Dateinamen bekommen, auch wenn beide Originale z.B.
/// "foto.jpg" heissen.
fn beleg_kopieren(quelle: &str, ausgabe_id: i64) -> Result<String, TreuhandFehler> {
    let quelle_pfad = Path::new(quelle);
    let endung = quelle_pfad.extension().and_then(|e| e.to_str()).unwrap_or("dat");
    let ordner = sicherungs_ordner().join("Belege");
    std::fs::create_dir_all(&ordner).map_err(TreuhandFehler::Beleg)?;
    let ziel = ordner.join(format!("Beleg_{ausgabe_id}.{endung}"));
    std::fs::copy(quelle_pfad, &ziel).map_err(TreuhandFehler::Beleg)?;
    Ok(ziel.display().to_string())
}

pub fn ausgabe_erfassen(conn: &Connection, eingabe: &NeueAusgabe) -> Result<Ausgabe, TreuhandFehler> {
    if !AUSGABEN_KATEGORIEN.contains(&eingabe.kategorie.as_str()) {
        return Err(TreuhandFehler::UnbekannteKategorie);
    }
    if !(eingabe.betrag > 0.0) {
        return Err(TreuhandFehler::UngueltigerBetrag);
    }
    conn.execute(
        "INSERT INTO ausgaben (datum, kategorie, betrag, notiz) VALUES (?1, ?2, ?3, ?4)",
        params![eingabe.datum.trim(), eingabe.kategorie, eingabe.betrag, eingabe.notiz.trim()],
    )?;
    let id = conn.last_insert_rowid();

    if let Some(quelle) = &eingabe.beleg_quelle {
        let beleg_pfad = beleg_kopieren(quelle, id)?;
        conn.execute("UPDATE ausgaben SET beleg_pfad = ?1 WHERE id = ?2", params![beleg_pfad, id])?;
    }

    let sql = format!("SELECT {AUSGABE_SPALTEN} FROM ausgaben WHERE id = ?1");
    Ok(conn.query_row(&sql, [id], zeile_zu_ausgabe)?)
}

/// Fuer den Excel-/CSV-Import (Stefans bisherige Treuhand-Tabelle). Eine
/// Zeile ohne Datum, mit unbekannter Kategorie oder ungueltigem Betrag
/// wird uebersprungen statt den ganzen Import abzubrechen - das Frontend
/// zeigt solche Zeilen schon in der Vorschau als uebersprungen an, hier
/// nochmals zur Sicherheit, falls doch eine durchrutscht. Beleg-Dateien
/// gehoeren nicht zu einem Tabellen-Import (kein Dateipfad in Excel) und
/// werden hier bewusst nicht beruecksichtigt.
///
/// Eine Ausgabe mit gleichem Datum, gleicher Kategorie, gleichem Betrag und
/// gleicher Notiz gibt es schon -> uebersprungen und als "doppelt"
/// gezaehlt. So kann dieselbe Excel (z.B. jeden Monat um neue Zeilen
/// ergaenzt) wieder importiert werden, ohne dass etwas doppelt zaehlt.
pub fn ausgaben_importieren(conn: &mut Connection, eingaben: Vec<NeueAusgabe>) -> Result<ImportErgebnis, TreuhandFehler> {
    let tx = conn.transaction()?;
    let mut ergebnis = ImportErgebnis::default();
    let mut schon_da = SchonDa::default();
    for eingabe in eingaben {
        let datum = eingabe.datum.trim();
        if !datum_gueltig(datum) || !AUSGABEN_KATEGORIEN.contains(&eingabe.kategorie.as_str()) || !(eingabe.betrag > 0.0) {
            continue;
        }
        let betrag = rappen(eingabe.betrag);
        let schluessel = format!("{datum}|{}|{betrag:.2}|{}", eingabe.kategorie, eingabe.notiz.trim());
        let vorhanden = schon_da.verbrauchen(&schluessel, || {
            tx.query_row(
                "SELECT COUNT(*) FROM ausgaben WHERE datum = ?1 AND kategorie = ?2 AND ROUND(betrag, 2) = ?3 AND notiz = ?4",
                params![datum, eingabe.kategorie, betrag, eingabe.notiz.trim()],
                |z| z.get(0),
            )
        })?;
        if vorhanden {
            ergebnis.doppelt += 1;
            continue;
        }
        tx.execute(
            "INSERT INTO ausgaben (datum, kategorie, betrag, notiz) VALUES (?1, ?2, ?3, ?4)",
            params![datum, eingabe.kategorie, betrag, eingabe.notiz.trim()],
        )?;
        ergebnis.neu += 1;
    }
    tx.commit()?;
    Ok(ergebnis)
}

#[derive(Debug, Serialize, PartialEq, Default)]
pub struct ImportErgebnis {
    pub neu: usize,
    pub doppelt: usize,
}

/// Erkennt schon importierte Zeilen auch dann richtig, wenn derselbe
/// Betrag im selben Monat mehrmals vorkommt (z.B. zweimal "Bar 50.00" im
/// Juni): pro Schluessel zaehlt, wie viele gleiche Zeilen schon in der
/// Datenbank stehen - erst die darueber hinaus werden neu eingetragen.
#[derive(Default)]
struct SchonDa {
    rest: std::collections::HashMap<String, i64>,
}

impl SchonDa {
    /// true = diese Zeile gibt es schon (und ist damit "verbraucht").
    fn verbrauchen(&mut self, schluessel: &str, anzahl_in_db: impl FnOnce() -> rusqlite::Result<i64>) -> rusqlite::Result<bool> {
        if !self.rest.contains_key(schluessel) {
            let n = anzahl_in_db()?;
            self.rest.insert(schluessel.to_string(), n);
        }
        let rest = self.rest.get_mut(schluessel).expect("eben eingefuegt");
        if *rest > 0 {
            *rest -= 1;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn datum_gueltig(datum: &str) -> bool {
    chrono::NaiveDate::parse_from_str(datum, "%Y-%m-%d").is_ok()
}

fn rappen(betrag: f64) -> f64 {
    (betrag * 100.0).round() / 100.0
}

// ---------------------------------------------------------------------------
// Einnahmen aus Excel: was vor dem Programm (laufendes Jahr) noch in Stefans
// Excel erfasst wurde. Zaehlt im Treuhand-Bericht zusammen mit den
// Auftraegen - sonst fehlten die Monate vor dem Umstieg.
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct NeueEinnahme {
    pub datum: String,
    pub betrag: f64,
    #[serde(default)]
    pub notiz: String,
    /// "Bar", "Karte", "Twint", "Rechnung" oder leer (unbekannt).
    #[serde(default)]
    pub zahlart: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct Einnahme {
    pub id: i64,
    pub datum: String,
    pub betrag: f64,
    pub notiz: String,
    pub zahlart: String,
}

/// Gleiche Regeln wie bei den Ausgaben: ungueltige Zeilen werden
/// uebersprungen, schon vorhandene (Datum + Betrag + Notiz) nicht doppelt
/// eingetragen.
pub fn einnahmen_importieren(conn: &mut Connection, eingaben: Vec<NeueEinnahme>) -> Result<ImportErgebnis, TreuhandFehler> {
    let tx = conn.transaction()?;
    let mut ergebnis = ImportErgebnis::default();
    let mut schon_da = SchonDa::default();
    for eingabe in eingaben {
        let datum = eingabe.datum.trim();
        if !datum_gueltig(datum) || !(eingabe.betrag > 0.0) {
            continue;
        }
        let betrag = rappen(eingabe.betrag);
        let zahlart = match eingabe.zahlart.trim() {
            z @ ("Bar" | "Karte" | "Twint" | "Rechnung") => z,
            _ => "",
        };
        let notiz = eingabe.notiz.trim();
        let schluessel = format!("{datum}|{betrag:.2}|{notiz}|{zahlart}");
        let vorhanden = schon_da.verbrauchen(&schluessel, || {
            tx.query_row(
                "SELECT COUNT(*) FROM einnahmen_extern WHERE datum = ?1 AND ROUND(betrag, 2) = ?2 AND notiz = ?3 AND zahlart = ?4",
                params![datum, betrag, notiz, zahlart],
                |z| z.get(0),
            )
        })?;
        if vorhanden {
            ergebnis.doppelt += 1;
            continue;
        }
        tx.execute(
            "INSERT INTO einnahmen_extern (datum, betrag, notiz, zahlart) VALUES (?1, ?2, ?3, ?4)",
            params![datum, betrag, notiz, zahlart],
        )?;
        ergebnis.neu += 1;
    }
    tx.commit()?;
    Ok(ergebnis)
}

pub fn einnahmen_extern_eines_jahres(conn: &Connection, jahr: i32) -> Result<Vec<Einnahme>, TreuhandFehler> {
    let mut stmt = conn.prepare(
        "SELECT id, datum, betrag, notiz, zahlart FROM einnahmen_extern WHERE strftime('%Y', datum) = ?1 ORDER BY datum, id",
    )?;
    let zeilen = stmt
        .query_map([jahr.to_string()], |z| {
            Ok(Einnahme { id: z.get(0)?, datum: z.get(1)?, betrag: z.get(2)?, notiz: z.get(3)?, zahlart: z.get(4)? })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

pub fn einnahme_extern_loeschen(conn: &Connection, id: i64) -> Result<(), TreuhandFehler> {
    conn.execute("DELETE FROM einnahmen_extern WHERE id = ?1", [id])?;
    Ok(())
}

pub fn ausgaben_eines_jahres(conn: &Connection, jahr: i32) -> Result<Vec<Ausgabe>, TreuhandFehler> {
    let sql = format!("SELECT {AUSGABE_SPALTEN} FROM ausgaben WHERE strftime('%Y', datum) = ?1 ORDER BY datum, id");
    let mut stmt = conn.prepare(&sql)?;
    let zeilen = stmt.query_map([jahr.to_string()], zeile_zu_ausgabe)?.collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

pub fn ausgabe_loeschen(conn: &Connection, id: i64) -> Result<(), TreuhandFehler> {
    conn.execute("DELETE FROM ausgaben WHERE id = ?1", [id])?;
    Ok(())
}

#[derive(Debug, Serialize, PartialEq)]
pub struct TreuhandZusammenfassung {
    pub einnahmen: f64,
    pub ausgaben_nach_kategorie: Vec<(String, f64)>,
    pub ausgaben_gesamt: f64,
    pub netto: f64,
}

/// Reine Rechenlogik, getrennt von der Datenbank-Abfrage - so laesst sich
/// die Zusammenfassung ohne Datei-/DB-Zugriff testen.
fn zusammenfassen(einnahmen: f64, ausgaben: &[Ausgabe]) -> TreuhandZusammenfassung {
    let ausgaben_nach_kategorie: Vec<(String, f64)> = AUSGABEN_KATEGORIEN
        .iter()
        .map(|k| ((*k).to_string(), ausgaben.iter().filter(|a| a.kategorie == *k).map(|a| a.betrag).sum()))
        .collect();
    let ausgaben_gesamt: f64 = ausgaben_nach_kategorie.iter().map(|(_, s)| s).sum();
    TreuhandZusammenfassung { einnahmen, ausgaben_gesamt, netto: einnahmen - ausgaben_gesamt, ausgaben_nach_kategorie }
}

fn einnahmen_eines_jahres(conn: &Connection, jahr: i32) -> rusqlite::Result<f64> {
    conn.query_row(
        "SELECT COALESCE(SUM(summe), 0) FROM auftraege WHERE strftime('%Y', datum) = ?1 AND status = 'Abgeholt'",
        [jahr.to_string()],
        |z| z.get(0),
    )
}

fn einnahmen_extern_summe(conn: &Connection, jahr: i32) -> rusqlite::Result<f64> {
    conn.query_row(
        "SELECT COALESCE(SUM(betrag), 0) FROM einnahmen_extern WHERE strftime('%Y', datum) = ?1",
        [jahr.to_string()],
        |z| z.get(0),
    )
}

/// Rechnungen aus den alten Kundenordnern (kundenordner.rs) - aber nur in
/// Monaten, fuer die es keine Zahlen aus der Treuhand-Excel gibt: deren
/// Monatsblaetter enthalten dieselben Einnahmen schon, sonst zaehlte alles
/// doppelt.
pub(crate) const ALTE_RECHNUNGEN_OHNE_EXCEL_MONATE: &str = "
    strftime('%Y', datum) = ?1
    AND strftime('%m', datum) NOT IN (
        SELECT strftime('%m', e.datum) FROM einnahmen_extern e WHERE strftime('%Y', e.datum) = ?1
    )";

fn alte_rechnungen_summe(conn: &Connection, jahr: i32) -> rusqlite::Result<f64> {
    conn.query_row(
        &format!("SELECT COALESCE(SUM(summe), 0) FROM alte_rechnungen WHERE {ALTE_RECHNUNGEN_OHNE_EXCEL_MONATE}"),
        [jahr.to_string()],
        |z| z.get(0),
    )
}

/// Umsatz der abgerechneten Auftraege pro Monat (nur aus dem Programm) -
/// fuer die Doppelt-Warnung beim Excel-Import.
#[derive(Debug, Serialize)]
pub struct MonatsUmsatz {
    pub monat: u32,
    pub summe: f64,
}

pub fn auftraege_monatsumsatz(conn: &Connection, jahr: i32) -> Result<Vec<MonatsUmsatz>, TreuhandFehler> {
    let mut stmt = conn.prepare(
        "SELECT CAST(strftime('%m', datum) AS INTEGER), SUM(summe) FROM auftraege
         WHERE strftime('%Y', datum) = ?1 AND status = 'Abgeholt' GROUP BY 1 ORDER BY 1",
    )?;
    let zeilen = stmt
        .query_map([jahr.to_string()], |z| Ok(MonatsUmsatz { monat: z.get(0)?, summe: z.get(1)? }))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

/// Die Zahlen des Treuhand-Berichts, fuer die Anzeige im Reiter (zum
/// Vergleichen mit der eigenen Excel, bevor der Bericht rausgeht).
#[derive(Debug, Serialize)]
pub struct TreuhandUebersicht {
    pub einnahmen_auftraege: f64,
    pub einnahmen_excel: f64,
    /// Aus den alten Kundenordnern, nur Monate ohne Excel-Zahlen.
    pub einnahmen_alte_rechnungen: f64,
    pub einnahmen: f64,
    pub ausgaben_nach_kategorie: Vec<(String, f64)>,
    pub ausgaben_gesamt: f64,
    pub netto: f64,
}

pub fn treuhand_uebersicht(conn: &Connection, jahr: i32) -> Result<TreuhandUebersicht, TreuhandFehler> {
    let einnahmen_auftraege = einnahmen_eines_jahres(conn, jahr)?;
    let einnahmen_excel = einnahmen_extern_summe(conn, jahr)?;
    let einnahmen_alte_rechnungen = alte_rechnungen_summe(conn, jahr)?;
    let z = zusammenfassen(einnahmen_auftraege + einnahmen_excel + einnahmen_alte_rechnungen, &ausgaben_eines_jahres(conn, jahr)?);
    Ok(TreuhandUebersicht {
        einnahmen_auftraege,
        einnahmen_excel,
        einnahmen_alte_rechnungen,
        einnahmen: z.einnahmen,
        ausgaben_nach_kategorie: z.ausgaben_nach_kategorie,
        ausgaben_gesamt: z.ausgaben_gesamt,
        netto: z.netto,
    })
}

/// Jahresbericht fuer den Treuhaender: Einnahmen (aus den Auftraegen) +
/// Ausgaben (nach Kategorie summiert) + Netto, als CSV mit Geschaefts-Kopf
/// - entspricht inhaltlich Stefans bisherigem "Treuhand"-Tabellenblatt.
pub fn treuhand_bericht_exportieren(conn: &Connection, jahr: i32) -> Result<PathBuf, String> {
    let einstellungen = crate::einstellungen::einstellungen_lesen(conn).map_err(|e| e.to_string())?;
    let einnahmen_auftraege = einnahmen_eines_jahres(conn, jahr).map_err(|e| e.to_string())?;
    let einnahmen_excel = einnahmen_extern_summe(conn, jahr).map_err(|e| e.to_string())?;
    let einnahmen_alte = alte_rechnungen_summe(conn, jahr).map_err(|e| e.to_string())?;
    let ausgaben = ausgaben_eines_jahres(conn, jahr).map_err(|e| e.to_string())?;
    let z = zusammenfassen(einnahmen_auftraege + einnahmen_excel + einnahmen_alte, &ausgaben);

    let mut csv = String::new();
    csv.push_str(&format!("{}\n", csv_feld(&einstellungen.geschaeft_name)));
    csv.push_str(&format!("{}\n", csv_feld(&einstellungen.geschaeft_zeile2)));
    csv.push_str(&format!("{}\n\n", csv_feld(&einstellungen.geschaeft_adresse)));
    csv.push_str(&format!("Treuhand-Bericht,Januar - Dezember {jahr}\n\n"));
    csv.push_str(&format!("Einnahmen gemäss Kundenrechnungen,,{:.2}\n", einnahmen_auftraege));
    if einnahmen_excel > 0.0 {
        csv.push_str(&format!("Einnahmen übernommen aus Excel,,{einnahmen_excel:.2}\n"));
    }
    if einnahmen_alte > 0.0 {
        csv.push_str(&format!("Einnahmen gemäss früheren Kundenrechnungen (Excel-Ordner),,{einnahmen_alte:.2}\n"));
    }
    csv.push_str(&format!("Total Einnahmen,,{:.2}\n\n", z.einnahmen));
    csv.push_str("Ausgaben\n");
    for (kategorie, summe) in &z.ausgaben_nach_kategorie {
        csv.push_str(&format!("{},,{summe:.2}\n", csv_feld(kategorie)));
    }
    csv.push_str(&format!("Total Ausgaben,,{:.2}\n\n", z.ausgaben_gesamt));
    csv.push_str(&format!("Netto Einnahmen,,{:.2}\n", z.netto));

    let ordner = sicherungs_ordner().join("Treuhand");
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let pfad = ordner.join(format!("Treuhand-Bericht_{jahr}.csv"));
    schreiben(&pfad, &csv)?;
    Ok(pfad)
}

#[derive(Debug, Serialize, PartialEq)]
pub struct LohnBerechnung {
    pub stunden: f64,
    pub arbeitslohn: f64,
    pub ferienzuschlag: f64,
    pub bruttolohn: f64,
    pub ahv: f64,
    pub alv: f64,
    pub total_abzuege: f64,
    pub nettolohn: f64,
}

/// Reine Lohn-Rechenlogik, getrennt von Datenbank/Datei - genau die Formel
/// aus Stefans "Lohnabrechnung Stundenlohn laufend"-Excel: Arbeitslohn =
/// Stunden × Stundenlohn, Ferienzuschlag und die Abzuege (AHV/IV/EO, ALV)
/// je ein Prozentsatz vom Bruttolohn. KTV/NBU bewusst nicht automatisiert -
/// bei der bisherigen Mitarbeiterin ohne Abzug, muesste sonst zusaetzlich
/// pro Person hinterlegt werden.
pub fn lohn_berechnen(stunden: f64, stundenlohn: f64, ferienzuschlag_satz: f64, ahv_satz: f64, alv_satz: f64) -> LohnBerechnung {
    let arbeitslohn = stunden * stundenlohn;
    let ferienzuschlag = arbeitslohn * ferienzuschlag_satz / 100.0;
    let bruttolohn = arbeitslohn + ferienzuschlag;
    let ahv = bruttolohn * ahv_satz / 100.0;
    let alv = bruttolohn * alv_satz / 100.0;
    let total_abzuege = ahv + alv;
    LohnBerechnung { stunden, arbeitslohn, ferienzuschlag, bruttolohn, ahv, alv, total_abzuege, nettolohn: bruttolohn - total_abzuege }
}

/// Monatliche Lohnabrechnung einer Mitarbeiterin als CSV, aus den bereits
/// erfassten Stunden berechnet - braucht einen hinterlegten Stundenlohn
/// (siehe Mitarbeiterin anlegen/bearbeiten).
pub fn lohnabrechnung_exportieren(conn: &Connection, benutzer_id: i64, jahr: i32, monat: u32) -> Result<PathBuf, String> {
    let mitarbeiterin = crate::auth::benutzer_holen(conn, benutzer_id).map_err(|e| e.to_string())?;
    let stundenlohn = mitarbeiterin
        .stundenlohn
        .ok_or_else(|| "Für diese Person ist kein Stundenlohn hinterlegt (siehe Mitarbeiterin bearbeiten).".to_string())?;
    let einstellungen = crate::einstellungen::einstellungen_lesen(conn).map_err(|e| e.to_string())?;
    let eintraege = crate::stunden::eigene_stunden(conn, benutzer_id, jahr, monat).map_err(|e| e.to_string())?;
    let stunden: f64 = eintraege.iter().map(|e| e.stunden).sum();
    let l = lohn_berechnen(stunden, stundenlohn, einstellungen.lohn_ferienzuschlag_satz, einstellungen.lohn_ahv_satz, einstellungen.lohn_alv_satz);

    let mut csv = String::new();
    csv.push_str(&format!("Lohnabrechnung,{monat:02}.{jahr}\n\n"));
    csv.push_str("Arbeitgeber\n");
    csv.push_str(&format!("{}\n", csv_feld(&einstellungen.geschaeft_name)));
    csv.push_str(&format!("{}\n\n", csv_feld(&einstellungen.geschaeft_adresse)));
    csv.push_str("Arbeitnehmer\n");
    csv.push_str(&format!("{}\n", csv_feld(&mitarbeiterin.anzeigename)));
    csv.push_str(&format!("{}\n", csv_feld(&mitarbeiterin.strasse)));
    csv.push_str(&format!("{}\n", csv_feld(&mitarbeiterin.plz_ort)));
    csv.push_str(&format!("AHV-Nummer,{}\n\n", csv_feld(&mitarbeiterin.ahv_nummer)));
    csv.push_str(&format!("Stundenlohn,{stundenlohn:.2}\n"));
    csv.push_str(&format!("Anzahl Stunden,{:.2}\n\n", l.stunden));
    csv.push_str("Lohn,Betrag\n");
    csv.push_str(&format!("Arbeitslohn,{:.2}\n", l.arbeitslohn));
    csv.push_str(&format!("Ferienzuschlag ({}%),{:.2}\n", einstellungen.lohn_ferienzuschlag_satz, l.ferienzuschlag));
    csv.push_str(&format!("Bruttolohn,{:.2}\n\n", l.bruttolohn));
    csv.push_str("Abzüge,Betrag\n");
    csv.push_str(&format!("AHV/IV/EO ({}%),{:.2}\n", einstellungen.lohn_ahv_satz, l.ahv));
    csv.push_str(&format!("ALV ({}%),{:.2}\n", einstellungen.lohn_alv_satz, l.alv));
    csv.push_str(&format!("Total Abzüge,{:.2}\n\n", l.total_abzuege));
    csv.push_str(&format!("Nettolohn,{:.2}\n", l.nettolohn));

    let ordner = sicherungs_ordner().join("Lohnabrechnungen");
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let dateiname = format!("Lohnabrechnung_{}_{jahr}-{monat:02}.csv", mitarbeiterin.anzeigename.replace(' ', "_"));
    let pfad = ordner.join(dateiname);
    schreiben(&pfad, &csv)?;
    Ok(pfad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    fn ausgabe(datum: &str, kategorie: &str, betrag: f64) -> NeueAusgabe {
        NeueAusgabe { datum: datum.into(), kategorie: kategorie.into(), betrag, notiz: "".into(), beleg_quelle: None }
    }

    #[test]
    fn unbekannte_kategorie_wird_abgelehnt() {
        let conn = test_db();
        let ergebnis = ausgabe_erfassen(&conn, &ausgabe("2026-01-05", "Urlaub", 100.0));
        assert!(matches!(ergebnis, Err(TreuhandFehler::UnbekannteKategorie)));
    }

    #[test]
    fn negativer_oder_null_betrag_wird_abgelehnt() {
        let conn = test_db();
        let ergebnis = ausgabe_erfassen(&conn, &ausgabe("2026-01-05", "Telefon", 0.0));
        assert!(matches!(ergebnis, Err(TreuhandFehler::UngueltigerBetrag)));
    }

    #[test]
    fn ausgaben_importieren_ueberspringt_ungueltige_zeilen_statt_abzubrechen() {
        let mut conn = test_db();
        let eingaben = vec![
            ausgabe("2026-01-05", "Telefon", 50.0),
            ausgabe("2026-01-06", "Urlaub", 10.0), // unbekannte Kategorie
            ausgabe("", "Telefon", 10.0),           // kein Datum
            ausgabe("2026-01-07", "Auto", 0.0),     // ungueltiger Betrag
            ausgabe("2026-01-08", "AHV", 30.0),
        ];
        let ergebnis = ausgaben_importieren(&mut conn, eingaben).unwrap();
        assert_eq!(ergebnis, ImportErgebnis { neu: 2, doppelt: 0 });
        assert_eq!(ausgaben_eines_jahres(&conn, 2026).unwrap().len(), 2);
    }

    // Dieselbe Treuhand-Excel ein zweites Mal importiert (jetzt um einen
    // Monat laenger): nichts zaehlt doppelt.
    #[test]
    fn erneuter_ausgaben_import_zaehlt_nichts_doppelt() {
        let mut conn = test_db();
        let erste = || vec![ausgabe("2026-01-05", "Telefon", 49.90), ausgabe("2026-02-05", "Telefon", 49.90)];
        assert_eq!(ausgaben_importieren(&mut conn, erste()).unwrap(), ImportErgebnis { neu: 2, doppelt: 0 });
        let mut zweite = erste();
        zweite.push(ausgabe("2026-03-05", "Telefon", 49.90));
        assert_eq!(ausgaben_importieren(&mut conn, zweite).unwrap(), ImportErgebnis { neu: 1, doppelt: 2 });
        assert_eq!(ausgaben_eines_jahres(&conn, 2026).unwrap().len(), 3);
    }

    // Einnahmen aus Excel (Monate vor dem Programm) zaehlen in der Treuhand-
    // Uebersicht zusammen mit den Auftraegen, ein zweiter Import nicht
    // doppelt, und loeschen nimmt sie wieder raus.
    #[test]
    fn einnahmen_aus_excel_zaehlen_im_treuhand_total() {
        let mut conn = test_db();
        conn.execute_batch(
            "INSERT INTO kunden (nummer, name) VALUES (101, 'Meier');
             INSERT INTO auftraege (kunde_id, rechnungsnummer, datum, zahlart, summe) VALUES (1, 1300, '2026-10-02', 'Bar', 100.0);",
        )
        .unwrap();
        let excel = || {
            vec![
                NeueEinnahme { datum: "2026-01-31".into(), betrag: 1500.0, notiz: "Januar".into(), zahlart: "".into() },
                NeueEinnahme { datum: "2026-02-28".into(), betrag: 1250.5, notiz: "Februar".into(), zahlart: "".into() },
                NeueEinnahme { datum: "2026-02-30".into(), betrag: 10.0, notiz: "".into(), zahlart: "".into() }, // kein echtes Datum
            ]
        };
        assert_eq!(einnahmen_importieren(&mut conn, excel()).unwrap(), ImportErgebnis { neu: 2, doppelt: 0 });
        assert_eq!(einnahmen_importieren(&mut conn, excel()).unwrap(), ImportErgebnis { neu: 0, doppelt: 2 });
        ausgaben_importieren(&mut conn, vec![ausgabe("2026-01-05", "Telefon", 50.0)]).unwrap();

        let u = treuhand_uebersicht(&conn, 2026).unwrap();
        assert_eq!((u.einnahmen_auftraege, u.einnahmen_excel, u.einnahmen), (100.0, 2750.5, 2850.5));
        assert_eq!((u.ausgaben_gesamt, u.netto), (50.0, 2800.5));

        let id = einnahmen_extern_eines_jahres(&conn, 2026).unwrap()[0].id;
        einnahme_extern_loeschen(&conn, id).unwrap();
        assert_eq!(treuhand_uebersicht(&conn, 2026).unwrap().einnahmen_excel, 1250.5);
    }

    #[test]
    fn ausgaben_werden_nach_jahr_gefiltert_und_geloescht() {
        let conn = test_db();
        ausgabe_erfassen(&conn, &ausgabe("2026-03-01", "Telefon", 50.0)).unwrap();
        ausgabe_erfassen(&conn, &ausgabe("2025-03-01", "Telefon", 40.0)).unwrap();
        let eingetragen = ausgabe_erfassen(&conn, &ausgabe("2026-05-01", "Auto", 30.0)).unwrap();

        let eines_jahres = ausgaben_eines_jahres(&conn, 2026).unwrap();
        assert_eq!(eines_jahres.len(), 2, "2025er-Ausgabe darf nicht mitgezaehlt werden");

        ausgabe_loeschen(&conn, eingetragen.id).unwrap();
        assert_eq!(ausgaben_eines_jahres(&conn, 2026).unwrap().len(), 1);
    }

    // Rechnet exakt Stefans Treuhand-Beispiel nach: Einnahmen 18975.29,
    // Ausgaben nach Kategorie (siehe hochgeladene Treuhand-Excel), Total
    // Ausgaben 18050.81, Netto 924.48.
    #[test]
    fn zusammenfassung_rechnet_wie_in_stefans_treuhand_beispiel() {
        let ausgaben = vec![
            Ausgabe { id: 1, datum: "2026-01-01".into(), kategorie: "Kleinmaterial / Atelier".into(), betrag: 604.27, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 2, datum: "2026-01-01".into(), kategorie: "Einrichten / Investition".into(), betrag: 995.2, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 3, datum: "2026-01-01".into(), kategorie: "Telefon".into(), betrag: 1090.6, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 4, datum: "2026-01-01".into(), kategorie: "Versicherung".into(), betrag: 30.0, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 5, datum: "2026-01-01".into(), kategorie: "Reparaturen / Service Arbeitsgeräte".into(), betrag: 99.9, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 6, datum: "2026-01-01".into(), kategorie: "Miete / Strom".into(), betrag: 12505.0, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 7, datum: "2026-01-01".into(), kategorie: "AHV".into(), betrag: 332.9, notiz: "".into(), beleg_pfad: None },
            Ausgabe { id: 8, datum: "2026-01-01".into(), kategorie: "Mitarbeiterin".into(), betrag: 2392.94, notiz: "".into(), beleg_pfad: None },
        ];
        let z = zusammenfassen(18975.29, &ausgaben);
        assert!((z.ausgaben_gesamt - 18050.81).abs() < 0.01, "Total Ausgaben: {}", z.ausgaben_gesamt);
        assert!((z.netto - 924.48).abs() < 0.01, "Netto: {}", z.netto);
    }

    #[test]
    fn einnahmen_eines_jahres_summiert_nur_auftraege_dieses_jahres() {
        let mut conn = test_db();
        let kunde = crate::geschaeft::kunde_anlegen(
            &conn,
            crate::geschaeft::NeuerKunde {
                nummer: None,
                name: "Meier".into(),
                vorname: "".into(),
                telefon: "".into(),
                ort: "".into(),
                adresse: "".into(),
                email: "".into(),
                notiz: "".into(),
            },
        )
        .unwrap();
        crate::geschaeft::auftrag_anlegen(
            &mut conn,
            crate::geschaeft::NeuerAuftrag {
                kunde_id: kunde.id,
                zahlart: "Bar".into(),
                posten: vec![crate::geschaeft::Posten { bezeichnung: "Kürzen".into(), stueck: 1.0, preis: 42.0 }],
            },
        )
        .unwrap();
        // auftrag_anlegen setzt das Datum selbst auf heute - die
        // Einnahmen-Summe dieses (heutigen) Jahres muss die 42.- enthalten.
        let jahr: i32 = chrono::Local::now().format("%Y").to_string().parse().unwrap();
        let einnahmen = einnahmen_eines_jahres(&conn, jahr).unwrap();
        assert!((einnahmen - 42.0).abs() < 0.001);
        assert_eq!(einnahmen_eines_jahres(&conn, jahr - 1).unwrap(), 0.0);
    }

    // Rechnet exakt Stefans Lohnabrechnung-Beispiel (Januar) nach:
    // Stundenlohn 24.66, 15 Stunden -> Nettolohn ca. 375.07.
    #[test]
    fn lohn_berechnen_stimmt_mit_stefans_lohnabrechnung_ueberein() {
        let l = lohn_berechnen(15.0, 24.66, 8.33, 5.3, 1.1);
        assert!((l.arbeitslohn - 369.9).abs() < 0.01);
        assert!((l.ferienzuschlag - 30.81267).abs() < 0.01);
        assert!((l.bruttolohn - 400.71267).abs() < 0.01);
        assert!((l.ahv - 21.23777151).abs() < 0.01);
        assert!((l.alv - 4.40783937).abs() < 0.01);
        assert!((l.nettolohn - 375.06705912).abs() < 0.01);
    }

    // Stefans Monatsblatt: derselbe Betrag kommt im Monat mehrmals vor
    // (zweimal Bar 50.00). Beide muessen rein - und ein zweiter Import der
    // gleichen Datei darf trotzdem nichts doppelt eintragen.
    #[test]
    fn gleiche_betraege_im_selben_monat_zaehlen_einzeln() {
        let mut conn = test_db();
        let e = |betrag: f64, zahlart: &str| NeueEinnahme { datum: "2026-06-30".into(), betrag, notiz: zahlart.into(), zahlart: zahlart.into() };
        let juni = || vec![e(50.0, "Bar"), e(50.0, "Bar"), e(24.38, "Karte"), e(24.38, "Karte"), e(65.0, "Twint")];
        assert_eq!(einnahmen_importieren(&mut conn, juni()).unwrap(), ImportErgebnis { neu: 5, doppelt: 0 });
        let mut mehr = juni();
        mehr.push(e(50.0, "Bar")); // ein drittes Mal Bar 50.00 kam dazu
        assert_eq!(einnahmen_importieren(&mut conn, mehr).unwrap(), ImportErgebnis { neu: 1, doppelt: 5 });
        let liste = einnahmen_extern_eines_jahres(&conn, 2026).unwrap();
        assert_eq!(liste.len(), 6);
        assert_eq!(liste.iter().filter(|x| x.zahlart == "Bar").count(), 3);

        let a = |betrag: f64| ausgabe("2026-06-30", "Telefon", betrag);
        assert_eq!(ausgaben_importieren(&mut conn, vec![a(49.9), a(49.9)]).unwrap(), ImportErgebnis { neu: 2, doppelt: 0 });
        assert_eq!(ausgaben_importieren(&mut conn, vec![a(49.9), a(49.9)]).unwrap(), ImportErgebnis { neu: 0, doppelt: 2 });
    }

    // Fruehere Rechnungen aus den Kundenordnern zaehlen mit - aber nicht in
    // Monaten, die schon aus der Treuhand-Excel kommen (doppelt).
    #[test]
    fn alte_kundenrechnungen_zaehlen_nur_in_monaten_ohne_excel() {
        let conn = test_db();
        conn.execute_batch(
            "INSERT INTO kunden (nummer, name) VALUES (251, 'Bona');
             INSERT INTO alte_rechnungen (kunde_id, datum, summe, zahlart) VALUES
                (1, '2026-04-08', 30.0, 'Bar'), (1, '2026-06-02', 44.0, 'Twint'), (1, '2025-12-01', 99.0, 'Bar'), (1, '', 12.0, '');
             INSERT INTO einnahmen_extern (datum, betrag, zahlart) VALUES ('2026-06-30', 2449.23, 'Bar');",
        )
        .unwrap();
        let u = treuhand_uebersicht(&conn, 2026).unwrap();
        assert_eq!(u.einnahmen_alte_rechnungen, 30.0, "April zaehlt, Juni kommt schon aus der Excel");
        assert_eq!(u.einnahmen, 2479.23);
    }
}
