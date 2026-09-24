// Automatische Sicherung. Ohne Cloud dahinter ist das hier der Ersatz fuer
// ein Cloud-Backup - darum bewusst zwei Formen auf einmal:
//
// 1. Eine 1:1-Kopie der kompletten Datenbankdatei (.sqlite3) - falls das
//    Programm neu installiert werden muss, spielt man diese Datei einfach
//    zurueck und alles ist wieder da, exakt wie vorher.
// 2. Kunden und Auftraege zusaetzlich als CSV - lesbar mit Excel/LibreOffice
//    ganz ohne das Programm, falls mal nur schnell reingeschaut werden muss
//    oder die Zahlen dem Steuerberater gegeben werden.
//
// Der Ordner liegt unter Dokumente\Atelierbuch Straub\Sicherung. Empfehlung
// an Stefan (siehe Chat): diesen Ordner zusaetzlich per kostenlosem
// OneDrive/Google-Drive-Ordner synchronisieren lassen, dann liegt eine
// Kopie auch ausserhalb des einen PCs - kostet nichts, ist aber ausserhalb
// der Kontrolle dieses Programms und muss einmalig von Hand eingerichtet
// werden.

use crate::geschaeft;
use chrono::Local;
use rusqlite::Connection;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn sicherungs_ordner() -> PathBuf {
    let mut pfad = dirs::document_dir().unwrap_or_else(|| dirs::home_dir().unwrap_or_default());
    pfad.push("Atelierbuch Straub");
    pfad.push("Sicherung");
    pfad
}

fn csv_feld(text: &str) -> String {
    // Anfuehrungszeichen verdoppeln und das Feld in Anfuehrungszeichen
    // setzen, sobald ein Komma, Zeilenumbruch oder Anfuehrungszeichen
    // drinsteckt - sonst zerlegt Excel die Zeile an der falschen Stelle.
    if text.contains(',') || text.contains('"') || text.contains('\n') {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text.to_string()
    }
}

pub fn jetzt_sichern(conn: &Connection) -> Result<PathBuf, String> {
    let heute = Local::now().format("%Y-%m-%d_%H-%M").to_string();
    let ziel_ordner = sicherungs_ordner().join(&heute);
    fs::create_dir_all(&ziel_ordner).map_err(|e| e.to_string())?;

    // 1) Volle Datenbankdatei kopieren.
    let db_kopie = ziel_ordner.join("atelierbuch.sqlite3");
    fs::copy(crate::db::datenbank_pfad(), &db_kopie).map_err(|e| e.to_string())?;

    // 2) Kundenliste als CSV - dieselbe "saubere Auflistung" wie die
    //    Export-Vorschau aus der Skizze, nur jetzt ein echter Download.
    let kunden = geschaeft::kunden_suchen(conn, "", true).map_err(|e| e.to_string())?;
    let mut kunden_csv = String::from("Nummer,Name,Vorname,Ort,Telefon,Kartensatz,Jahresumsatz,Archiviert\n");
    for k in &kunden {
        let karte = k.kartensatz.map(|s| format!("{s}")).unwrap_or_default();
        kunden_csv.push_str(&format!(
            "{},{},{},{},{},{},{:.2},{}\n",
            k.nummer,
            csv_feld(&k.name),
            csv_feld(&k.vorname),
            csv_feld(&k.ort),
            csv_feld(&k.telefon),
            karte,
            k.jahresumsatz,
            if k.archiviert { "ja" } else { "nein" }
        ));
    }
    schreiben(&ziel_ordner.join("kunden.csv"), &kunden_csv)?;

    // 3) Alle Auftraege/Rechnungen als CSV - Grundlage fuer den
    //    Steuerberater-Export, der als naechstes dazukommt.
    let mut stmt = conn
        .prepare(
            "SELECT a.rechnungsnummer, a.datum, k.nummer, k.name, k.vorname, a.zahlart, a.summe
             FROM auftraege a JOIN kunden k ON k.id = a.kunde_id
             ORDER BY a.datum, a.rechnungsnummer",
        )
        .map_err(|e| e.to_string())?;
    let mut auftraege_csv = String::from("Rechnungsnr,Datum,Kundennr,Name,Vorname,Zahlart,Betrag\n");
    let zeilen = stmt
        .query_map([], |z| {
            Ok((
                z.get::<_, i64>(0)?,
                z.get::<_, String>(1)?,
                z.get::<_, i64>(2)?,
                z.get::<_, String>(3)?,
                z.get::<_, String>(4)?,
                z.get::<_, String>(5)?,
                z.get::<_, f64>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    for zeile in zeilen {
        let (rnr, datum, knr, name, vorname, zahlart, summe) = zeile.map_err(|e| e.to_string())?;
        auftraege_csv.push_str(&format!(
            "{rnr},{datum},{knr},{},{},{zahlart},{summe:.2}\n",
            csv_feld(&name),
            csv_feld(&vorname)
        ));
    }
    schreiben(&ziel_ordner.join("auftraege.csv"), &auftraege_csv)?;

    alte_sicherungen_aufraeumen()?;
    Ok(ziel_ordner)
}

fn schreiben(pfad: &PathBuf, inhalt: &str) -> Result<(), String> {
    let mut datei = fs::File::create(pfad).map_err(|e| e.to_string())?;
    // Byte-Order-Mark, damit Excel Umlaute (Muller, Kuhn, ...) korrekt
    // anzeigt statt als kryptische Zeichen - ein bekannter Excel-Eigenheit.
    datei.write_all(&[0xEF, 0xBB, 0xBF]).map_err(|e| e.to_string())?;
    datei.write_all(inhalt.as_bytes()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Behaelt die letzten 60 Sicherungen (bei taeglichem Gebrauch gut zwei
/// Monate) und loescht aeltere automatisch, damit der Ordner nicht endlos
/// waechst. Die 10-Jahre-Aufbewahrungspflicht wird dadurch NICHT verletzt -
/// die eigentlichen Buchungen bleiben ja permanent in der Datenbank selbst
/// und in den jaehrlichen Steuerberater-Exporten, die separat archiviert
/// werden. Diese Sicherungen hier sind der Notfall-Schnappschuss vom
/// laufenden Betrieb, kein Langzeitarchiv.
fn alte_sicherungen_aufraeumen() -> Result<(), String> {
    let ordner = sicherungs_ordner();
    let Ok(eintraege) = fs::read_dir(&ordner) else { return Ok(()) };
    let mut sicherungen: Vec<_> = eintraege.filter_map(|e| e.ok()).collect();
    sicherungen.sort_by_key(|e| e.file_name());
    if sicherungen.len() > 60 {
        for alt in &sicherungen[..sicherungen.len() - 60] {
            let _ = fs::remove_dir_all(alt.path());
        }
    }
    Ok(())
}
