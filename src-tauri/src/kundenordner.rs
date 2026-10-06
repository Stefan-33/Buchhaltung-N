// "Kundenordner einlesen": Stefans bisherige Ablage - pro Kundin ein Ordner
// "Nummer Name" (z.B. "251 Bona"), darin eine Excel-Datei
// ("Aenderungen.xlsx"), in der jedes Blatt ("Rechn", "Rechn (2)", ...) eine
// Rechnung ist: links das Original, rechts daneben eine Kopie.
//
// Das Programm geht alle Ordner durch und
// - legt jede Kundin mit ihrer Nummer an (oder ergaenzt eine schon
//   vorhandene mit derselben Nummer),
// - liest aus jedem Blatt die Rechnung (Datum, Arbeiten, Total, Zahlart)
//   als "fruehere Rechnung" - bewusst NICHT als Auftrag: diese Betraege
//   zaehlen nicht im Umsatz, sonst kaemen die Monate doppelt, die schon
//   ueber die Treuhand-Excel drin sind,
// - speichert die Dateien selbst in der Datenbank, damit sie auch auf einem
//   anderen PC (Daten-Paket) noch mit einem Klick aufgehen.

use calamine::{open_workbook_auto, Reader};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Groesste Datei, die in die Datenbank uebernommen wird (Fotos, Scans).
const MAX_DATEI_BYTES: u64 = 25 * 1024 * 1024;

fn norm(text: &str) -> String {
    text.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect()
}

const LABELS: &[&str] = &[
    "anrede", "name", "nachname", "vorname", "tel", "telefon", "natel", "mobile", "handy", "ort", "wohnort", "plzort",
    "mail", "email", "datum", "erstellt", "adresse", "strasse",
];

fn betrag(text: &str) -> Option<f64> {
    let t: String = text.trim().replace('\'', "").replace("Fr.", "").replace("CHF", "").replace(',', ".");
    let t = t.trim().trim_end_matches(".-").trim_end_matches('-').trim();
    t.parse::<f64>().ok().filter(|b| b.is_finite())
}

/// "08.04.26", "8.4.2026" oder "2026-04-08" -> "2026-04-08".
fn datum(text: &str) -> Option<String> {
    let t = text.trim();
    if let Some(iso) = t.get(0..10) {
        if chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d").is_ok() {
            return Some(iso.to_string());
        }
    }
    let teile: Vec<&str> = t.split('.').map(str::trim).collect();
    if teile.len() == 3 {
        let (tag, monat) = (teile[0].parse::<u32>().ok()?, teile[1].parse::<u32>().ok()?);
        let jahr_text = teile[2].split_whitespace().next()?;
        let mut jahr = jahr_text.parse::<i32>().ok()?;
        if jahr_text.len() == 2 {
            jahr += 2000;
        }
        return chrono::NaiveDate::from_ymd_opt(jahr, monat, tag).map(|d| d.format("%Y-%m-%d").to_string());
    }
    None
}

/// Eine Rechnung aus einem Blatt.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
pub struct BlattRechnung {
    pub datum: String,
    pub summe: f64,
    pub zahlart: String,
    /// Arbeiten, eine pro Zeile.
    pub posten: String,
    // Angaben zur Kundin, wie sie im Blatt stehen
    pub anrede: String,
    pub name: String,
    pub vorname: String,
    pub telefon: String,
    pub ort: String,
    pub email: String,
    pub adresse: String,
}

/// Liest eine Rechnung aus den Zellen eines Blatts (Zeilen von Text).
/// Gesucht wird ueber die Beschriftungen ("Name", "Tel", "Total",
/// "Erstellt", "Stück" ...), nicht ueber feste Zellen - so stoert es nicht,
/// wenn eine Rechnung eine Zeile mehr oder weniger hat. Nur die linke Haelfte
/// zaehlt (rechts steht die Kopie). Ein leeres Blatt ergibt None.
pub fn rechnung_aus_blatt(zellen: &[Vec<String>]) -> Option<BlattRechnung> {
    let zelle = |r: usize, c: usize| zellen.get(r).and_then(|z| z.get(c)).map(String::as_str).unwrap_or("");
    let breite_gesamt = zellen.iter().map(Vec::len).max().unwrap_or(0);

    // Kopfzeile der Arbeiten ("Stück ... CHF"); kommt "Stück" zweimal vor,
    // beginnt beim zweiten die Kopie.
    let ist_stueck = |t: &str| matches!(norm(t).as_str(), "stück" | "stueck" | "stk" | "anzahl");
    let mut kopf: Option<usize> = None;
    let mut grenze = breite_gesamt;
    for (r, zeile) in zellen.iter().enumerate() {
        let spalten: Vec<usize> = zeile.iter().enumerate().filter(|(_, t)| ist_stueck(t)).map(|(c, _)| c).collect();
        if !spalten.is_empty() {
            kopf = Some(r);
            if spalten.len() >= 2 {
                grenze = spalten[1];
            }
            break;
        }
    }

    let ist_label = |t: &str| LABELS.contains(&norm(t).as_str());
    // Wert rechts neben einer Beschriftung (bis zur naechsten Beschriftung).
    let wert = |namen: &[&str]| -> String {
        for (r, zeile) in zellen.iter().enumerate() {
            for c in 0..zeile.len().min(grenze) {
                if namen.contains(&norm(&zeile[c]).as_str()) {
                    let mut teile = Vec::new();
                    for c2 in c + 1..zeile.len().min(grenze) {
                        let t = zelle(r, c2).trim();
                        if ist_label(t) {
                            break;
                        }
                        if !t.is_empty() {
                            teile.push(t.to_string());
                        }
                    }
                    if !teile.is_empty() {
                        return teile.join(" ");
                    }
                }
            }
        }
        String::new()
    };

    // Total: erste Zahl rechts von "Total".
    let mut total_zeile: Option<usize> = None;
    let mut summe = 0.0;
    'suche: for (r, zeile) in zellen.iter().enumerate() {
        for c in 0..zeile.len().min(grenze) {
            if norm(&zeile[c]) == "total" {
                total_zeile = Some(r);
                for c2 in c + 1..zeile.len().min(grenze) {
                    if let Some(b) = betrag(zelle(r, c2)) {
                        summe = b;
                        break 'suche;
                    }
                }
                break 'suche;
            }
        }
    }

    // Zahlart: ein "X" links neben Bar / Twint / Karte / Rechnung.
    let mut zahlart = String::new();
    for zeile in zellen {
        for c in 0..zeile.len().min(grenze) {
            if norm(&zeile[c]) == "x" {
                for c2 in c + 1..zeile.len().min(grenze) {
                    let n = norm(&zeile[c2]);
                    let art = match n.as_str() {
                        "bar" => "Bar",
                        "twint" => "Twint",
                        "karte" | "kreditkarte" | "ec" | "ekarte" => "Karte",
                        "rechnung" => "Rechnung",
                        "" => continue,
                        _ => break,
                    };
                    zahlart = art.to_string();
                    break;
                }
            }
        }
    }

    // Arbeiten zwischen Kopfzeile und Total: eine Zeile mit Stueckzahl
    // beginnt einen neuen Posten, Folgezeilen gehoeren dazu.
    let mut posten: Vec<(String, Option<f64>)> = Vec::new();
    if let Some(k) = kopf {
        let kz = &zellen[k];
        let spalte = |namen: &[&str]| kz.iter().take(grenze).position(|t| namen.contains(&norm(t).as_str()));
        let stk = spalte(&["stück", "stueck", "stk", "anzahl"]);
        let chf = spalte(&["chf", "betrag", "preis", "total"]);
        let a = spalte(&["à", "a", "einzelpreis"]);
        let ende = total_zeile.unwrap_or(zellen.len());
        for r in k + 1..ende {
            let zeile = &zellen[r];
            let stueck = stk.and_then(|c| betrag(zelle(r, c))).filter(|s| *s > 0.0);
            let preis = chf.and_then(|c| betrag(zelle(r, c))).filter(|p| *p > 0.0);
            let text: Vec<&str> = (0..zeile.len().min(grenze))
                .filter(|c| Some(*c) != stk && Some(*c) != chf && Some(*c) != a)
                .map(|c| zelle(r, c).trim())
                .filter(|t| !t.is_empty() && *t != "0")
                .collect();
            let text = text.join(" ");
            if let Some(s) = stueck {
                posten.push((format!("{}× {}", s, text).trim().to_string(), preis));
            } else if !text.is_empty() {
                match posten.last_mut() {
                    Some(letzter) => {
                        letzter.0.push(' ');
                        letzter.0.push_str(&text);
                    }
                    None => posten.push((text, preis)),
                }
                if let (Some(p), Some(letzter)) = (preis, posten.last_mut()) {
                    letzter.1.get_or_insert(p);
                }
            } else if let (Some(p), Some(letzter)) = (preis, posten.last_mut()) {
                letzter.1.get_or_insert(p);
            }
        }
    }
    if summe <= 0.0 {
        summe = posten.iter().filter_map(|p| p.1).sum();
    }
    if summe <= 0.0 && posten.is_empty() {
        return None; // leere Vorlage
    }
    let posten_text = posten
        .iter()
        .map(|(t, p)| match p {
            Some(p) => format!("{t} – {p:.2}"),
            None => t.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n");

    let datum_text = datum(&wert(&["erstellt"])).or_else(|| datum(&wert(&["datum"]))).unwrap_or_default();
    Some(BlattRechnung {
        datum: datum_text,
        summe: (summe * 100.0).round() / 100.0,
        zahlart,
        posten: posten_text,
        anrede: wert(&["anrede"]),
        name: wert(&["name", "nachname"]),
        vorname: wert(&["vorname"]),
        telefon: wert(&["tel", "telefon", "natel", "mobile", "handy"]),
        ort: wert(&["ort", "wohnort", "plzort"]),
        email: wert(&["mail", "email"]),
        adresse: wert(&["adresse", "strasse"]),
    })
}

/// "251 Bona" -> (Some(251), "Bona"); "Hofstetter" -> (None, "Hofstetter").
pub fn ordnername_zerlegen(name: &str) -> (Option<i64>, String) {
    let t = name.trim();
    let ziffern: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if ziffern.is_empty() {
        return (None, t.to_string());
    }
    (ziffern.parse().ok(), t[ziffern.len()..].trim().to_string())
}

/// Eine Rechnung samt Herkunft (Datei / Blatt).
#[derive(Debug, Clone, Serialize)]
pub struct GeleseneRechnung {
    pub quelle: String,
    #[serde(flatten)]
    pub rechnung: BlattRechnung,
}

#[derive(Debug, Serialize)]
pub struct OrdnerKunde {
    pub ordner: String,
    pub nummer: Option<i64>,
    pub name: String,
    pub vorname: String,
    pub telefon: String,
    pub ort: String,
    pub email: String,
    pub adresse: String,
    pub anrede: String,
    pub rechnungen: Vec<GeleseneRechnung>,
    pub dateien: Vec<PathBuf>,
    pub fehler: Vec<String>,
}

fn tabellen_dateien(ordner: &Path) -> Vec<PathBuf> {
    let mut dateien: Vec<PathBuf> = std::fs::read_dir(ordner)
        .map(|it| it.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect())
        .unwrap_or_default();
    dateien.retain(|p| {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        !name.starts_with("~$") && !name.starts_with('.') && !name.eq_ignore_ascii_case("desktop.ini") && !name.eq_ignore_ascii_case("thumbs.db")
    });
    dateien.sort();
    dateien
}

/// Liest einen Kundenordner: Name/Nummer aus dem Ordnernamen, Rechnungen
/// aus allen Excel-Dateien darin (jedes Blatt eine Rechnung).
pub fn kundenordner_lesen(ordner: &Path) -> OrdnerKunde {
    let ordnername = ordner.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
    let (nummer, rest) = ordnername_zerlegen(&ordnername);
    let dateien = tabellen_dateien(ordner);
    let mut rechnungen = Vec::new();
    let mut fehler = Vec::new();
    for datei in &dateien {
        let endung = datei.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        if !matches!(endung.as_str(), "xlsx" | "xls" | "xlsm" | "ods") {
            continue;
        }
        let dateiname = datei.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let mut mappe = match open_workbook_auto(datei) {
            Ok(m) => m,
            Err(e) => {
                fehler.push(format!("{dateiname}: {e}"));
                continue;
            }
        };
        for blatt in mappe.sheet_names() {
            let Ok(bereich) = mappe.worksheet_range(&blatt) else { continue };
            // Ab Zeile/Spalte 0 auffuellen, damit die Spalten stimmen,
            // auch wenn das Blatt nicht in A1 beginnt.
            let (r0, c0) = bereich.start().map(|(r, c)| (r as usize, c as usize)).unwrap_or((0, 0));
            let mut zellen: Vec<Vec<String>> = vec![Vec::new(); r0];
            for zeile in bereich.rows() {
                let mut z = vec![String::new(); c0];
                z.extend(zeile.iter().map(crate::datei::zelle_zu_text));
                zellen.push(z);
            }
            if let Some(r) = rechnung_aus_blatt(&zellen) {
                rechnungen.push(GeleseneRechnung { quelle: format!("{dateiname} / {blatt}"), rechnung: r });
            }
        }
    }

    // Angaben zur Kundin: die neuste nicht-leere Angabe aus den Blaettern.
    let neuste = |f: fn(&BlattRechnung) -> &String| -> String {
        rechnungen.iter().rev().map(|r| f(&r.rechnung).trim().to_string()).find(|t| !t.is_empty()).unwrap_or_default()
    };
    let blatt_name = neuste(|r| &r.name);
    // Ziffern im Ordnernamen ("Hofstetter  076 533") sind ein Telefon-Anfang
    // zur Unterscheidung - als Name nur den Text davor nehmen.
    let ordner_name: String = rest.split_whitespace().take_while(|w| !w.chars().all(|c| c.is_ascii_digit())).collect::<Vec<_>>().join(" ");
    let name = if !blatt_name.is_empty() { blatt_name } else if !ordner_name.is_empty() { ordner_name } else { rest.clone() };

    OrdnerKunde {
        ordner: ordnername,
        nummer,
        name,
        vorname: neuste(|r| &r.vorname),
        telefon: neuste(|r| &r.telefon),
        ort: neuste(|r| &r.ort),
        email: neuste(|r| &r.email),
        adresse: neuste(|r| &r.adresse),
        anrede: neuste(|r| &r.anrede),
        rechnungen,
        dateien,
        fehler,
    }
}

fn unterordner(hauptordner: &Path) -> Result<Vec<PathBuf>, String> {
    let mut ordner: Vec<PathBuf> = std::fs::read_dir(hauptordner)
        .map_err(|e| format!("Ordner konnte nicht gelesen werden: {e}"))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    ordner.sort();
    Ok(ordner)
}

/// Kurzfassung pro Ordner fuer die Vorschau.
#[derive(Debug, Serialize)]
pub struct OrdnerVorschau {
    pub ordner: String,
    pub nummer: Option<i64>,
    pub name: String,
    pub telefon: String,
    pub ort: String,
    pub rechnungen: usize,
    pub summe: f64,
    pub letzte: String,
    pub dateien: usize,
    pub vorhanden: bool,
    pub fehler: Vec<String>,
}

pub fn vorschau(conn: &Connection, hauptordner: &Path) -> Result<Vec<OrdnerVorschau>, String> {
    let mut liste = Vec::new();
    for ordner in unterordner(hauptordner)? {
        let k = kundenordner_lesen(&ordner);
        let vorhanden = match k.nummer {
            Some(n) => conn
                .query_row("SELECT 1 FROM kunden WHERE nummer = ?1", [n], |_| Ok(()))
                .optional()
                .map_err(|e| e.to_string())?
                .is_some(),
            None => false,
        };
        liste.push(OrdnerVorschau {
            letzte: k.rechnungen.iter().map(|r| r.rechnung.datum.clone()).max().unwrap_or_default(),
            summe: (k.rechnungen.iter().map(|r| r.rechnung.summe).sum::<f64>() * 100.0).round() / 100.0,
            rechnungen: k.rechnungen.len(),
            dateien: k.dateien.len(),
            ordner: k.ordner,
            nummer: k.nummer,
            name: k.name,
            telefon: k.telefon,
            ort: k.ort,
            vorhanden,
            fehler: k.fehler,
        });
    }
    Ok(liste)
}

#[derive(Debug, Default, Serialize, PartialEq)]
pub struct OrdnerImport {
    pub neu: usize,
    pub ergaenzt: usize,
    pub rechnungen: usize,
    pub dateien: usize,
}

/// Uebernimmt einen gelesenen Kundenordner in die Datenbank (ohne
/// Transaktion - die macht der Aufrufer). Schon vorhandene Rechnungen
/// (gleiche Quelle) und Dateien (gleicher Name, gleiche Groesse) werden
/// nicht doppelt eingetragen; bei einer schon vorhandenen Kundin werden nur
/// leere Felder ergaenzt.
pub fn ordner_uebernehmen(conn: &Connection, k: &OrdnerKunde, ergebnis: &mut OrdnerImport) -> Result<(), String> {
    let e = |e: rusqlite::Error| e.to_string();
    let mut notiz_teile = Vec::new();
    if !k.anrede.is_empty() {
        notiz_teile.push(k.anrede.clone());
    }
    notiz_teile.push(format!("Ordner: {}", k.ordner));
    let notiz = notiz_teile.join(" · ");

    let vorhanden: Option<i64> = match k.nummer {
        Some(n) => conn.query_row("SELECT id FROM kunden WHERE nummer = ?1", [n], |z| z.get(0)).optional().map_err(e)?,
        None => None,
    };
    let kunde_id = match vorhanden {
        Some(id) => {
            conn.execute(
                "UPDATE kunden SET
                    vorname = CASE WHEN vorname = '' THEN ?2 ELSE vorname END,
                    telefon = CASE WHEN telefon = '' THEN ?3 ELSE telefon END,
                    ort = CASE WHEN ort = '' THEN ?4 ELSE ort END,
                    email = CASE WHEN email = '' THEN ?5 ELSE email END,
                    adresse = CASE WHEN adresse = '' THEN ?6 ELSE adresse END,
                    notiz = CASE WHEN instr(notiz, ?7) > 0 THEN notiz
                                 WHEN notiz = '' THEN ?7 ELSE notiz || ' · ' || ?7 END
                 WHERE id = ?1",
                params![id, k.vorname, k.telefon, k.ort, k.email, k.adresse, notiz],
            )
            .map_err(e)?;
            ergebnis.ergaenzt += 1;
            id
        }
        None => {
            let nummer = match k.nummer {
                Some(n) => n,
                None => crate::geschaeft::naechster_zaehler(conn, "naechste_kundennummer", 101).map_err(e)?,
            };
            conn.execute(
                "INSERT INTO kunden (nummer, name, vorname, telefon, ort, adresse, email, notiz) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![nummer, k.name, k.vorname, k.telefon, k.ort, k.adresse, k.email, notiz],
            )
            .map_err(e)?;
            // Sofort merken - das Zaehler-Update unten wuerde die "letzte
            // eingefuegte Zeile" sonst ueberschreiben.
            let neue_id = conn.last_insert_rowid();
            if k.nummer.is_some() {
                // Zaehler hinter die hoechste uebernommene Nummer setzen.
                conn.execute(
                    "INSERT INTO einstellungen (schluessel, wert) VALUES ('naechste_kundennummer', ?1)
                     ON CONFLICT(schluessel) DO UPDATE SET wert = ?1 WHERE CAST(wert AS INTEGER) < ?1",
                    params![nummer.to_string()],
                )
                .map_err(e)?;
            }
            ergebnis.neu += 1;
            neue_id
        }
    };

    for r in &k.rechnungen {
        let da: bool = conn
            .query_row("SELECT EXISTS(SELECT 1 FROM alte_rechnungen WHERE kunde_id = ?1 AND quelle = ?2)", params![kunde_id, r.quelle], |z| {
                z.get(0)
            })
            .map_err(e)?;
        if da {
            continue;
        }
        conn.execute(
            "INSERT INTO alte_rechnungen (kunde_id, datum, summe, zahlart, posten, quelle) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![kunde_id, r.rechnung.datum, r.rechnung.summe, r.rechnung.zahlart, r.rechnung.posten, r.quelle],
        )
        .map_err(e)?;
        ergebnis.rechnungen += 1;
    }

    for pfad in &k.dateien {
        let groesse = std::fs::metadata(pfad).map(|m| m.len()).unwrap_or(0);
        if groesse == 0 || groesse > MAX_DATEI_BYTES {
            continue;
        }
        let dateiname = pfad.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
        let da: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM kunden_dateien WHERE kunde_id = ?1 AND dateiname = ?2 AND groesse = ?3)",
                params![kunde_id, dateiname, groesse as i64],
                |z| z.get(0),
            )
            .map_err(e)?;
        if da {
            continue;
        }
        let inhalt = std::fs::read(pfad).map_err(|e| format!("{dateiname}: {e}"))?;
        conn.execute(
            "INSERT INTO kunden_dateien (kunde_id, dateiname, inhalt, groesse) VALUES (?1, ?2, ?3, ?4)",
            params![kunde_id, dateiname, inhalt, groesse as i64],
        )
        .map_err(e)?;
        ergebnis.dateien += 1;
    }
    Ok(())
}

/// Alle Unterordner einlesen und uebernehmen - in einer Transaktion: geht
/// etwas schief, bleibt die Datenbank wie vorher.
pub fn importieren(conn: &mut Connection, hauptordner: &Path) -> Result<OrdnerImport, String> {
    let ordner = unterordner(hauptordner)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let mut ergebnis = OrdnerImport::default();
    for o in ordner {
        let k = kundenordner_lesen(&o);
        // Ein Ordner ohne Nummer und ohne Rechnung ist keine Kundin
        // (z.B. "Vorlagen").
        if k.nummer.is_none() && k.rechnungen.is_empty() {
            continue;
        }
        ordner_uebernehmen(&tx, &k, &mut ergebnis)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(ergebnis)
}

// ---------------------------------------------------------------------------
// Anzeige im Kundenblatt
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct AlteRechnung {
    pub id: i64,
    pub datum: String,
    pub summe: f64,
    pub zahlart: String,
    pub posten: String,
    pub quelle: String,
}

pub fn alte_rechnungen_von_kunde(conn: &Connection, kunde_id: i64) -> Result<Vec<AlteRechnung>, String> {
    let mut stmt = conn
        .prepare("SELECT id, datum, summe, zahlart, posten, quelle FROM alte_rechnungen WHERE kunde_id = ?1 ORDER BY datum DESC, id DESC")
        .map_err(|e| e.to_string())?;
    let zeilen = stmt
        .query_map([kunde_id], |z| {
            Ok(AlteRechnung { id: z.get(0)?, datum: z.get(1)?, summe: z.get(2)?, zahlart: z.get(3)?, posten: z.get(4)?, quelle: z.get(5)? })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(zeilen)
}

#[derive(Debug, Serialize)]
pub struct KundenDatei {
    pub id: i64,
    pub dateiname: String,
    pub groesse: i64,
}

pub fn dateien_von_kunde(conn: &Connection, kunde_id: i64) -> Result<Vec<KundenDatei>, String> {
    let mut stmt = conn
        .prepare("SELECT id, dateiname, groesse FROM kunden_dateien WHERE kunde_id = ?1 ORDER BY dateiname")
        .map_err(|e| e.to_string())?;
    let zeilen = stmt
        .query_map([kunde_id], |z| Ok(KundenDatei { id: z.get(0)?, dateiname: z.get(1)?, groesse: z.get(2)? }))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(zeilen)
}

/// Schreibt eine gespeicherte Datei in den Temp-Ordner und gibt den Pfad
/// zurueck (zum Oeffnen mit Excel & Co.).
pub fn datei_auspacken(conn: &Connection, id: i64) -> Result<PathBuf, String> {
    let (kunde_id, name, inhalt): (i64, String, Vec<u8>) = conn
        .query_row("SELECT kunde_id, dateiname, inhalt FROM kunden_dateien WHERE id = ?1", [id], |z| Ok((z.get(0)?, z.get(1)?, z.get(2)?)))
        .map_err(|_| "Datei nicht gefunden.".to_string())?;
    let sicherer_name: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') { c } else { '_' }).collect();
    let ordner = std::env::temp_dir().join("Atelierbuch-Dateien").join(kunde_id.to_string());
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let pfad = ordner.join(sicherer_name);
    std::fs::write(&pfad, inhalt).map_err(|e| e.to_string())?;
    Ok(pfad)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zeile(werte: &[&str]) -> Vec<String> {
        werte.iter().map(|s| s.to_string()).collect()
    }

    // Nachbau von Stefans Blatt "Rechn" (Kundin 251 Bona): Original in den
    // Spalten A-H, Kopie ab Spalte J - gelesen wird nur das Original.
    fn bona_blatt() -> Vec<Vec<String>> {
        let mut z = vec![Vec::new(); 10];
        z.push(zeile(&["Anrede", "Frau", "", "", "", "Datum :", "", "", "", "Anrede", "Frau"]));
        z.push(zeile(&["Name", "Bona", "", "Tel", "079 773 49 53", "", "", "", "", "Name", "Bona"]));
        z.push(zeile(&["Ort", "", "", "Mail", "", "", "", "", "", "Ort"]));
        z.push(Vec::new());
        z.push(zeile(&["Stück", "Bezeichnung", "Arbeit", "", "", "", "à", "CHF", "", "Stück", "Bezeichnung", "Arbeit", "", "", "", "à", "CHF"]));
        z.push(Vec::new());
        z.push(zeile(&["1", "Strickpulli", "Am Kragen löcher flicken", "", "", "", "", "", "", "1", "Strickpulli"]));
        z.push(zeile(&["", "dunkelblau"]));
        z.push(zeile(&["", "", "", "", "", "", "", "30", "", "", "0"]));
        z.push(zeile(&["1", "Strickpulli", "Faden einziehen"]));
        z.push(zeile(&["", "schwarz", "Strich kann nicht entfernt werden"]));
        z.push(zeile(&["", "", "sonst gibt es ein Loch"]));
        for _ in 0..10 {
            z.push(Vec::new());
        }
        z.push(zeile(&["", "Besten Dank", "", "", "", "", "Total", "30", "", "", "Besten Dank", "", "", "", "", "Total", "30"]));
        z.push(Vec::new());
        z.push(zeile(&["Reklamationen innert 10 Tagen nach Abholung"]));
        z.push(Vec::new());
        z.push(zeile(&["Erstellt", "2026-04-08", "", "", "X", "Bar"]));
        z.push(zeile(&["251", "", "", "", "", "Twint"]));
        z.push(zeile(&["", "", "", "", "", "Karte"]));
        z
    }

    #[test]
    fn rechnung_aus_stefans_blatt() {
        let r = rechnung_aus_blatt(&bona_blatt()).unwrap();
        assert_eq!(r.datum, "2026-04-08");
        assert_eq!(r.summe, 30.0);
        assert_eq!(r.zahlart, "Bar");
        assert_eq!((r.anrede.as_str(), r.name.as_str(), r.telefon.as_str()), ("Frau", "Bona", "079 773 49 53"));
        let zeilen: Vec<&str> = r.posten.lines().collect();
        assert_eq!(zeilen.len(), 2, "{}", r.posten);
        assert!(zeilen[0].starts_with("1× Strickpulli Am Kragen löcher flicken dunkelblau"), "{}", zeilen[0]);
        assert!(zeilen[0].ends_with("– 30.00"));
        assert!(zeilen[1].contains("Faden einziehen") && zeilen[1].contains("sonst gibt es ein Loch"));
    }

    #[test]
    fn leere_vorlage_ist_keine_rechnung_und_datum_als_text() {
        let mut leer = bona_blatt();
        for z in leer.iter_mut().skip(16).take(6) {
            z.clear();
        }
        let total = leer.iter().position(|z| z.iter().any(|t| t == "Total")).unwrap();
        leer[total] = zeile(&["", "Besten Dank", "", "", "", "", "Total", ""]);
        assert!(rechnung_aus_blatt(&leer).is_none());

        assert_eq!(datum("08.04.26").as_deref(), Some("2026-04-08"));
        assert_eq!(datum("8.4.2026").as_deref(), Some("2026-04-08"));
        assert_eq!(datum("31.02.26"), None);
    }

    #[test]
    fn ordnernamen() {
        assert_eq!(ordnername_zerlegen("251 Bona"), (Some(251), "Bona".into()));
        assert_eq!(ordnername_zerlegen("626 Hofstetter  076 533"), (Some(626), "Hofstetter  076 533".into()));
        assert_eq!(ordnername_zerlegen("Ohne Nummer"), (None, "Ohne Nummer".into()));
    }

    // Ganzer Ablauf mit echten Ordnern (die Excel-Datei ist hier eine
    // Textdatei - es geht um Nummer, Name, Dateien und doppeltes Einlesen).
    #[test]
    fn ordner_einlesen_legt_kundinnen_an_und_ergaenzt_ohne_doppel() {
        let basis = std::env::temp_dir().join(format!("atelierbuch-ordner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&basis);
        for (ordner, datei) in [("251 Bona", "Notiz.txt"), ("626 Hofstetter  076 533", "Foto.jpg"), ("Leer", "")] {
            std::fs::create_dir_all(basis.join(ordner)).unwrap();
            if !datei.is_empty() {
                std::fs::write(basis.join(ordner).join(datei), b"Inhalt").unwrap();
            }
        }
        std::fs::write(basis.join("251 Bona").join("~$Aenderungen.xlsx"), b"Sperrdatei").unwrap();

        let mut conn = Connection::open_in_memory().unwrap();
        crate::db::schema_fuer_tests_anlegen(&conn);
        conn.execute("INSERT INTO kunden (nummer, name, telefon) VALUES (251, 'Bona', '079 773 49 53')", []).unwrap();

        let e = importieren(&mut conn, &basis).unwrap();
        assert_eq!(e, OrdnerImport { neu: 1, ergaenzt: 1, rechnungen: 0, dateien: 2 }, "Ordner ohne Nummer und Rechnung zaehlt nicht");
        let (name, notiz): (String, String) =
            conn.query_row("SELECT name, notiz FROM kunden WHERE nummer = 626", [], |z| Ok((z.get(0)?, z.get(1)?))).unwrap();
        assert_eq!(name, "Hofstetter");
        assert_eq!(notiz, "Ordner: 626 Hofstetter  076 533");

        // Zweites Einlesen: nichts doppelt.
        let e2 = importieren(&mut conn, &basis).unwrap();
        assert_eq!((e2.neu, e2.dateien), (0, 0));
        let anzahl: i64 = conn.query_row("SELECT COUNT(*) FROM kunden", [], |z| z.get(0)).unwrap();
        assert_eq!(anzahl, 2);
        // Naechste neue Kundin bekommt eine Nummer hinter 626.
        let n = crate::geschaeft::naechster_zaehler(&conn, "naechste_kundennummer", 101).unwrap();
        assert!(n > 626, "{n}");

        let hofstetter: i64 = conn.query_row("SELECT id FROM kunden WHERE nummer = 626", [], |z| z.get(0)).unwrap();
        assert_eq!(dateien_von_kunde(&conn, hofstetter).unwrap()[0].dateiname, "Foto.jpg");
        let bona: i64 = conn.query_row("SELECT id FROM kunden WHERE nummer = 251", [], |z| z.get(0)).unwrap();
        let dateien = dateien_von_kunde(&conn, bona).unwrap();
        assert_eq!(dateien.len(), 1, "Sperrdatei ~$ wird nicht uebernommen");
        let pfad = datei_auspacken(&conn, dateien[0].id).unwrap();
        assert_eq!(std::fs::read(&pfad).unwrap(), b"Inhalt");
        let _ = std::fs::remove_dir_all(&basis);
    }
}
