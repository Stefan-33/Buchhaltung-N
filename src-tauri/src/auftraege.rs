// Auftrags-Ablauf von der Annahme bis zur Abholung (Reiter "Auftraege"
// und "Uebersicht"):
//
//   Annahme (Reiter Auftraege)  ->  Angenommen / In Arbeit / Abholbereit
//   Abrechnen (Kundenblatt)     ->  Abgeholt, mit Zahlart und Beleg
//
// Auftrag und Rechnung sind dieselbe Zeile in "auftraege" und tragen
// dieselbe Nummer - die Kundin bekommt bei der Annahme die Nummer, unter
// der spaeter auch abgerechnet wird. Erst ein abgerechneter (abgeholter)
// Auftrag zaehlt als Umsatz.

use crate::geschaeft::{
    archiv_aktualisieren, auftrag_holen, naechster_zaehler, posten_einfuegen, zahlart_pruefen, Auftrag,
    GeschaeftFehler, Posten, AUFTRAG_STATUS, STATUS_ABGEHOLT,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
pub struct NeueAnnahme {
    pub kunde_id: i64,
    pub posten: Vec<Posten>,
    #[serde(default)]
    pub abholdatum: Option<String>,
}

/// Eine Zeile fuer die Auftragsliste und die Uebersicht - mit Kundin und
/// einer Kurzbeschreibung der Arbeit, damit die Liste ohne weitere
/// Abfragen auskommt.
#[derive(Debug, Serialize, PartialEq)]
pub struct AuftragZeile {
    pub id: i64,
    pub rechnungsnummer: i64,
    pub kunde_id: i64,
    pub kunde_nummer: i64,
    pub kunde_name: String,
    pub kunde_telefon: String,
    pub datum: String,
    pub angenommen_am: Option<String>,
    pub abholdatum: Option<String>,
    pub status: String,
    pub zahlart: String,
    pub summe: f64,
    pub bezahlt: bool,
    pub arbeit: String,
    pub alter_tage: i64,
}

#[derive(Debug, Serialize)]
pub struct Uebersicht {
    pub heute: Vec<AuftragZeile>,
    pub ueberfaellig: Vec<AuftragZeile>,
    pub laufend_anzahl: i64,
    pub abholbereit_anzahl: i64,
    pub unbezahlt_anzahl: i64,
    pub unbezahlt_summe: f64,
    pub umsatz_monat: f64,
}

/// Leeres Feld -> kein Abholdatum; sonst muss es ein echtes Datum
/// (JJJJ-MM-TT, so liefert es das Datumsfeld der Oberflaeche) sein.
fn abholdatum_pruefen(abholdatum: Option<String>) -> Result<Option<String>, GeschaeftFehler> {
    match abholdatum.map(|d| d.trim().to_string()).filter(|d| !d.is_empty()) {
        None => Ok(None),
        Some(d) => chrono::NaiveDate::parse_from_str(&d, "%Y-%m-%d")
            .map(|_| Some(d))
            .map_err(|_| GeschaeftFehler::UngueltigesDatum),
    }
}

/// Auftrag bei der Annahme erfassen (Kleidungsstueck wird abgegeben) -
/// noch nicht bezahlt, noch kein Umsatz. Die Zahlart steht hier technisch
/// auf "Rechnung" (die Tabelle verlangt eine), die echte Zahlart wird beim
/// Abrechnen gesetzt.
pub fn auftrag_annehmen(conn: &mut Connection, eingabe: NeueAnnahme) -> Result<Auftrag, GeschaeftFehler> {
    let posten: Vec<Posten> = eingabe.posten.into_iter().filter(|p| !p.bezeichnung.trim().is_empty()).collect();
    if posten.is_empty() {
        return Err(GeschaeftFehler::KeinePosten);
    }
    let abholdatum = abholdatum_pruefen(eingabe.abholdatum)?;
    let vorhanden: Option<i64> =
        conn.query_row("SELECT id FROM kunden WHERE id = ?1", [eingabe.kunde_id], |z| z.get(0)).optional()?;
    if vorhanden.is_none() {
        return Err(GeschaeftFehler::KundeNichtGefunden);
    }

    let summe: f64 = posten.iter().map(|p| p.stueck * p.preis).sum();
    let rechnungsnummer = naechster_zaehler(conn, "naechste_rechnungsnummer", 1259)?;

    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO auftraege (kunde_id, rechnungsnummer, zahlart, summe, status, bezahlt, abholdatum, angenommen_am)
         VALUES (?1, ?2, 'Rechnung', ?3, 'Angenommen', 0, ?4, date('now'))",
        params![eingabe.kunde_id, rechnungsnummer, summe, abholdatum],
    )?;
    let auftrag_id = tx.last_insert_rowid();
    posten_einfuegen(&tx, auftrag_id, &posten)?;
    tx.commit()?;
    archiv_aktualisieren(conn)?;

    auftrag_holen(conn, auftrag_id)
}

/// Beim Abholen im Kundenblatt: Arbeiten (evtl. mit angepasstem Preis)
/// und Zahlart festhalten - ab jetzt Umsatz, das Rechnungsdatum ist heute.
/// Mit "Rechnung" bleibt der Betrag als offener Posten stehen.
pub fn auftrag_abrechnen(
    conn: &mut Connection,
    auftrag_id: i64,
    zahlart: &str,
    posten: Vec<Posten>,
) -> Result<Auftrag, GeschaeftFehler> {
    zahlart_pruefen(zahlart)?;
    let posten: Vec<Posten> = posten.into_iter().filter(|p| !p.bezeichnung.trim().is_empty()).collect();
    if posten.is_empty() {
        return Err(GeschaeftFehler::KeinePosten);
    }
    let status: String = conn
        .query_row("SELECT status FROM auftraege WHERE id = ?1", [auftrag_id], |z| z.get(0))
        .optional()?
        .ok_or(GeschaeftFehler::AuftragNichtGefunden)?;
    if status == STATUS_ABGEHOLT {
        return Err(GeschaeftFehler::BereitsAbgerechnet);
    }

    let summe: f64 = posten.iter().map(|p| p.stueck * p.preis).sum();
    let bezahlt = zahlart != "Rechnung";

    let tx = conn.transaction()?;
    tx.execute("DELETE FROM auftrag_posten WHERE auftrag_id = ?1", [auftrag_id])?;
    posten_einfuegen(&tx, auftrag_id, &posten)?;
    tx.execute(
        "UPDATE auftraege SET zahlart = ?1, summe = ?2, status = 'Abgeholt', datum = date('now'),
                bezahlt = ?3, bezahlt_am = CASE WHEN ?3 = 1 THEN date('now') END
         WHERE id = ?4",
        params![zahlart, summe, bezahlt as i64, auftrag_id],
    )?;
    tx.commit()?;
    archiv_aktualisieren(conn)?;

    auftrag_holen(conn, auftrag_id)
}

/// Status eines laufenden Auftrags aendern (Angenommen / In Arbeit /
/// Abholbereit). "Abgeholt" geht bewusst nur ueber das Abrechnen, damit
/// kein Auftrag das Haus ohne Beleg verlaesst.
pub fn auftrag_status_setzen(conn: &Connection, auftrag_id: i64, status: &str) -> Result<(), GeschaeftFehler> {
    if status == STATUS_ABGEHOLT || !AUFTRAG_STATUS.contains(&status) {
        return Err(GeschaeftFehler::UngueltigerStatus);
    }
    let bisher: String = conn
        .query_row("SELECT status FROM auftraege WHERE id = ?1", [auftrag_id], |z| z.get(0))
        .optional()?
        .ok_or(GeschaeftFehler::AuftragNichtGefunden)?;
    if bisher == STATUS_ABGEHOLT {
        return Err(GeschaeftFehler::BereitsAbgerechnet);
    }
    conn.execute("UPDATE auftraege SET status = ?1 WHERE id = ?2", params![status, auftrag_id])?;
    Ok(())
}

/// Offenen Posten (abgerechnet per Rechnung) als bezahlt markieren.
pub fn auftrag_bezahlt_markieren(conn: &Connection, auftrag_id: i64) -> Result<(), GeschaeftFehler> {
    let status: String = conn
        .query_row("SELECT status FROM auftraege WHERE id = ?1", [auftrag_id], |z| z.get(0))
        .optional()?
        .ok_or(GeschaeftFehler::AuftragNichtGefunden)?;
    if status != STATUS_ABGEHOLT {
        return Err(GeschaeftFehler::NochNichtAbgerechnet);
    }
    conn.execute(
        "UPDATE auftraege SET bezahlt = 1, bezahlt_am = COALESCE(bezahlt_am, date('now')) WHERE id = ?1",
        [auftrag_id],
    )?;
    Ok(())
}

const ZEILE_SQL: &str = r#"
    SELECT a.id, a.rechnungsnummer, k.id, k.nummer, TRIM(k.vorname || ' ' || k.name),
           a.datum, a.angenommen_am, a.abholdatum, a.status, a.zahlart, a.summe, a.bezahlt,
           COALESCE((SELECT p.bezeichnung FROM auftrag_posten p WHERE p.auftrag_id = a.id ORDER BY p.id LIMIT 1), ''),
           (SELECT COUNT(*) FROM auftrag_posten p WHERE p.auftrag_id = a.id),
           CAST(julianday(date('now', 'localtime')) - julianday(a.datum) AS INTEGER),
           k.telefon
    FROM auftraege a JOIN kunden k ON k.id = a.kunde_id
"#;

fn zeile_lesen(z: &rusqlite::Row) -> rusqlite::Result<AuftragZeile> {
    let erste: String = z.get(12)?;
    let anzahl: i64 = z.get(13)?;
    let arbeit = if anzahl > 1 { format!("{erste} +{}", anzahl - 1) } else { erste };
    Ok(AuftragZeile {
        id: z.get(0)?,
        rechnungsnummer: z.get(1)?,
        kunde_id: z.get(2)?,
        kunde_nummer: z.get(3)?,
        kunde_name: z.get(4)?,
        kunde_telefon: z.get(15)?,
        datum: z.get(5)?,
        angenommen_am: z.get(6)?,
        abholdatum: z.get(7)?,
        status: z.get(8)?,
        zahlart: z.get(9)?,
        summe: z.get(10)?,
        bezahlt: z.get::<_, i64>(11)? != 0,
        arbeit,
        alter_tage: z.get::<_, i64>(14)?.max(0),
    })
}

fn zeilen_abfragen(conn: &Connection, bedingung_und_sortierung: &str) -> rusqlite::Result<Vec<AuftragZeile>> {
    let sql = format!("{ZEILE_SQL} {bedingung_und_sortierung}");
    let mut stmt = conn.prepare(&sql)?;
    let zeilen = stmt.query_map([], zeile_lesen)?.collect::<Result<Vec<_>, _>>()?;
    Ok(zeilen)
}

/// Auftragsliste im Reiter "Auftraege" mit Filter:
/// - "laufend": noch nicht abgeholt, naechstes Abholdatum zuerst
/// - "abholbereit": fertig, wartet auf die Kundin
/// - "unbezahlt": alle noch nicht bezahlten, aelteste zuerst
/// - "alle": die letzten 300, neueste zuerst
pub fn auftraege_liste(conn: &Connection, filter: &str) -> Result<Vec<AuftragZeile>, GeschaeftFehler> {
    let teil = match filter {
        "laufend" => "WHERE a.status <> 'Abgeholt' ORDER BY a.abholdatum IS NULL, a.abholdatum, a.datum, a.id",
        "abholbereit" => "WHERE a.status = 'Abholbereit' ORDER BY a.abholdatum IS NULL, a.abholdatum, a.datum, a.id",
        "unbezahlt" => "WHERE a.bezahlt = 0 ORDER BY a.datum, a.id",
        "alle" => "ORDER BY a.datum DESC, a.id DESC LIMIT 300",
        _ => return Err(GeschaeftFehler::UngueltigerStatus),
    };
    Ok(zeilen_abfragen(conn, teil)?)
}

/// Kennzahlen und Listen fuer die Startseite.
pub fn uebersicht(conn: &Connection) -> Result<Uebersicht, GeschaeftFehler> {
    let heute = zeilen_abfragen(
        conn,
        "WHERE a.status <> 'Abgeholt' AND a.abholdatum = date('now', 'localtime') ORDER BY a.id",
    )?;
    let ueberfaellig = zeilen_abfragen(
        conn,
        "WHERE a.status <> 'Abgeholt' AND a.abholdatum < date('now', 'localtime') ORDER BY a.abholdatum, a.id",
    )?;
    let (laufend_anzahl, abholbereit_anzahl, unbezahlt_anzahl, unbezahlt_summe, umsatz_monat) = conn.query_row(
        "SELECT
            COALESCE(SUM(status <> 'Abgeholt'), 0),
            COALESCE(SUM(status = 'Abholbereit'), 0),
            COALESCE(SUM(bezahlt = 0), 0),
            COALESCE(SUM(CASE WHEN bezahlt = 0 THEN summe END), 0),
            COALESCE(SUM(CASE WHEN status = 'Abgeholt'
                               AND strftime('%Y-%m', datum) = strftime('%Y-%m', 'now', 'localtime')
                              THEN summe END), 0)
         FROM auftraege",
        [],
        |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?, z.get(3)?, z.get(4)?)),
    )?;
    Ok(Uebersicht { heute, ueberfaellig, laufend_anzahl, abholbereit_anzahl, unbezahlt_anzahl, unbezahlt_summe, umsatz_monat })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geschaeft::{kunde_anlegen, kunde_holen, monatsstatistik, NeuerAuftrag, NeuerKunde};

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn
    }

    fn kunde(conn: &Connection) -> i64 {
        kunde_anlegen(
            conn,
            NeuerKunde {
                nummer: None,
                name: "Meier".into(),
                vorname: "Anna".into(),
                telefon: "".into(),
                ort: "".into(),
                adresse: "".into(),
                email: "".into(),
                notiz: "".into(),
            },
        )
        .unwrap()
        .id
    }

    fn posten(bezeichnung: &str, preis: f64) -> Posten {
        Posten { bezeichnung: bezeichnung.into(), stueck: 1.0, preis }
    }

    fn annehmen(conn: &mut Connection, kunde_id: i64, abholdatum: Option<&str>) -> Auftrag {
        auftrag_annehmen(
            conn,
            NeueAnnahme {
                kunde_id,
                posten: vec![posten("Hose kürzen", 25.0), posten("Saum", 10.0)],
                abholdatum: abholdatum.map(String::from),
            },
        )
        .unwrap()
    }

    fn heute_plus(tage: i64) -> String {
        (chrono::Local::now().date_naive() + chrono::Duration::days(tage)).format("%Y-%m-%d").to_string()
    }

    fn aktuelles_jahr() -> i32 {
        chrono::Local::now().format("%Y").to_string().parse().unwrap()
    }

    #[test]
    fn angenommener_auftrag_ist_offen_und_noch_kein_umsatz() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let a = annehmen(&mut conn, k, Some(&heute_plus(3)));

        assert_eq!(a.status, "Angenommen");
        assert!(!a.bezahlt);
        assert_eq!(a.summe, 35.0);
        assert_eq!(a.posten.len(), 2);
        assert!(a.angenommen_am.is_some());

        let kd = kunde_holen(&conn, k).unwrap();
        assert_eq!(kd.jahresumsatz, 0.0, "noch nicht abgerechnet -> kein Umsatz");
        assert_eq!(kd.offen_summe, 35.0);
        assert!(monatsstatistik(&conn, aktuelles_jahr()).unwrap().is_empty());
    }

    #[test]
    fn abrechnen_mit_bar_macht_den_auftrag_zu_bezahltem_umsatz() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let a = annehmen(&mut conn, k, None);

        // Preis beim Abholen noch angepasst (z.B. zusaetzliche Arbeit)
        let abgerechnet = auftrag_abrechnen(&mut conn, a.id, "Bar", vec![posten("Hose kürzen", 30.0)]).unwrap();
        assert_eq!(abgerechnet.status, "Abgeholt");
        assert!(abgerechnet.bezahlt);
        assert!(abgerechnet.bezahlt_am.is_some());
        assert_eq!(abgerechnet.zahlart, "Bar");
        assert_eq!(abgerechnet.summe, 30.0);
        assert_eq!(abgerechnet.posten.len(), 1);
        assert_eq!(abgerechnet.rechnungsnummer, a.rechnungsnummer, "Auftrag und Rechnung tragen dieselbe Nummer");

        let kd = kunde_holen(&conn, k).unwrap();
        assert_eq!(kd.jahresumsatz, 30.0);
        assert_eq!(kd.offen_summe, 0.0);

        let erneut = auftrag_abrechnen(&mut conn, a.id, "Bar", vec![posten("x", 1.0)]);
        assert!(matches!(erneut, Err(GeschaeftFehler::BereitsAbgerechnet)));
    }

    #[test]
    fn abrechnen_per_rechnung_bleibt_offener_posten_bis_bezahlt() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let a = annehmen(&mut conn, k, None);

        // Noch nicht abgerechnet -> darf nicht einfach als bezahlt gelten
        assert!(matches!(auftrag_bezahlt_markieren(&conn, a.id), Err(GeschaeftFehler::NochNichtAbgerechnet)));

        let abgerechnet = auftrag_abrechnen(&mut conn, a.id, "Rechnung", vec![posten("Hose kürzen", 25.0)]).unwrap();
        assert!(!abgerechnet.bezahlt);
        assert_eq!(kunde_holen(&conn, k).unwrap().jahresumsatz, 25.0, "Rechnung zaehlt wie bisher als Umsatz");
        assert_eq!(auftraege_liste(&conn, "unbezahlt").unwrap().len(), 1);

        auftrag_bezahlt_markieren(&conn, a.id).unwrap();
        assert!(auftraege_liste(&conn, "unbezahlt").unwrap().is_empty());
        assert!(crate::geschaeft::auftrag_holen(&conn, a.id).unwrap().bezahlt_am.is_some());
    }

    #[test]
    fn sofort_ablauf_im_kundenblatt_mit_rechnung_ist_ein_offener_posten() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let bar = crate::geschaeft::auftrag_anlegen(
            &mut conn,
            NeuerAuftrag { kunde_id: k, zahlart: "Bar".into(), posten: vec![posten("Knopf", 5.0)] },
        )
        .unwrap();
        let rechnung = crate::geschaeft::auftrag_anlegen(
            &mut conn,
            NeuerAuftrag { kunde_id: k, zahlart: "Rechnung".into(), posten: vec![posten("Jacke", 80.0)] },
        )
        .unwrap();
        assert!(bar.bezahlt && bar.status == "Abgeholt");
        assert!(!rechnung.bezahlt && rechnung.status == "Abgeholt");
        let offen = auftraege_liste(&conn, "unbezahlt").unwrap();
        assert_eq!(offen.len(), 1);
        assert_eq!(offen[0].summe, 80.0);
    }

    #[test]
    fn status_aendern_nur_fuer_laufende_auftraege_und_nie_direkt_auf_abgeholt() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let a = annehmen(&mut conn, k, None);

        auftrag_status_setzen(&conn, a.id, "In Arbeit").unwrap();
        auftrag_status_setzen(&conn, a.id, "Abholbereit").unwrap();
        assert_eq!(auftraege_liste(&conn, "abholbereit").unwrap().len(), 1);

        assert!(matches!(auftrag_status_setzen(&conn, a.id, "Abgeholt"), Err(GeschaeftFehler::UngueltigerStatus)));
        assert!(matches!(auftrag_status_setzen(&conn, a.id, "Fertig"), Err(GeschaeftFehler::UngueltigerStatus)));

        auftrag_abrechnen(&mut conn, a.id, "Twint", vec![posten("Hose", 20.0)]).unwrap();
        assert!(matches!(auftrag_status_setzen(&conn, a.id, "In Arbeit"), Err(GeschaeftFehler::BereitsAbgerechnet)));
    }

    #[test]
    fn unbezahlt_liste_ist_nach_alter_sortiert_aelteste_zuerst() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let neu = annehmen(&mut conn, k, None);
        let alt = annehmen(&mut conn, k, None);
        conn.execute("UPDATE auftraege SET datum = date('now', '-20 days') WHERE id = ?1", [alt.id]).unwrap();

        let liste = auftraege_liste(&conn, "unbezahlt").unwrap();
        assert_eq!(liste.iter().map(|z| z.id).collect::<Vec<_>>(), vec![alt.id, neu.id]);
        assert!(liste[0].alter_tage >= 19);
        assert_eq!(liste[0].arbeit, "Hose kürzen +1");
        assert_eq!(liste[0].kunde_name, "Anna Meier");
    }

    #[test]
    fn uebersicht_zeigt_heute_faellige_und_ueberfaellige_auftraege() {
        let mut conn = test_db();
        let k = kunde(&conn);
        annehmen(&mut conn, k, Some(&heute_plus(0)));
        annehmen(&mut conn, k, Some(&heute_plus(-2)));
        annehmen(&mut conn, k, Some(&heute_plus(5)));
        let abgeholt = annehmen(&mut conn, k, Some(&heute_plus(-1)));
        auftrag_abrechnen(&mut conn, abgeholt.id, "Bar", vec![posten("Hose", 40.0)]).unwrap();

        let u = uebersicht(&conn).unwrap();
        assert_eq!(u.heute.len(), 1);
        assert_eq!(u.ueberfaellig.len(), 1, "der abgeholte Auftrag zaehlt nicht als ueberfaellig");
        assert_eq!(u.laufend_anzahl, 3);
        assert_eq!(u.unbezahlt_anzahl, 3);
        assert!((u.unbezahlt_summe - 105.0).abs() < 0.001);
        assert!((u.umsatz_monat - 40.0).abs() < 0.001);
    }

    #[test]
    fn ungueltiges_abholdatum_und_leere_posten_werden_abgelehnt() {
        let mut conn = test_db();
        let k = kunde(&conn);
        let falsches_datum = auftrag_annehmen(
            &mut conn,
            NeueAnnahme { kunde_id: k, posten: vec![posten("Hose", 10.0)], abholdatum: Some("31.02.2026".into()) },
        );
        assert!(matches!(falsches_datum, Err(GeschaeftFehler::UngueltigesDatum)));

        let ohne_posten = auftrag_annehmen(
            &mut conn,
            NeueAnnahme { kunde_id: k, posten: vec![posten("  ", 10.0)], abholdatum: None },
        );
        assert!(matches!(ohne_posten, Err(GeschaeftFehler::KeinePosten)));

        let unbekannte_kundin = auftrag_annehmen(
            &mut conn,
            NeueAnnahme { kunde_id: 999, posten: vec![posten("Hose", 10.0)], abholdatum: None },
        );
        assert!(matches!(unbekannte_kundin, Err(GeschaeftFehler::KundeNichtGefunden)));
    }
}
