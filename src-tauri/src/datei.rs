// Liest eine ausgewaehlte Datei (Excel .xlsx/.xls oder CSV) als einfache
// Tabelle (Zeilen von Textzellen) ein - fuer den dateibasierten Import
// (Kunden, Stunden). Bewusst keine eigene Spalten-Erkennung hier: das
// Ergebnis wird 1:1 wie eingefuegter Text aus Excel/LibreOffice behandelt
// (gleiche Kopfzeilen-Erkennung in app.js wie beim Copy&Paste-Import),
// damit es nur einen Ort fuer diese Logik gibt.

use calamine::{open_workbook_auto, Data, Reader};
use std::path::Path;

/// Markiert in der Tabelle den Beginn eines Tabellenblatts, wenn
/// `blattnamen` gesetzt ist: eine Zeile ["#BLATT", Name]. Der Treuhand-
/// Import liest daraus den Monat ("Juni", "Juni A") und ueberspringt
/// Zusammenfassungen ("Jahr", "Treuhand").
pub const BLATT_MARKE: &str = "#BLATT";

/// `alle_blaetter`: bei Excel jedes Tabellenblatt nacheinander lesen
/// (Stunden-Import: z.B. ein Blatt pro Jahr), sonst nur das erste.
pub fn datei_als_tabelle_lesen(pfad: &str, alle_blaetter: bool, blattnamen: bool) -> Result<Vec<Vec<String>>, String> {
    let pfad = Path::new(pfad);
    // calamine liest nur "echte" Tabellenformate (xlsx/xls/xlsb/ods) - CSV
    // ist reiner Text und wird separat behandelt, mit derselben Trennzeichen-
    // Erkennung wie beim Copy&Paste-Import in app.js.
    let ist_csv = pfad.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("csv"));

    let tabelle = if ist_csv {
        let inhalt = std::fs::read_to_string(pfad).map_err(|e| format!("Datei konnte nicht gelesen werden: {e}"))?;
        // Eine evtl. vorhandene UTF-8-BOM entfernen - genau die, die
        // sicherung.rs selbst beim Schreiben voranstellt (fuer Excel-
        // Umlaute), damit eine eigene exportierte Datei auch wieder
        // sauber eingelesen werden kann.
        csv_text_als_tabelle(inhalt.strip_prefix('\u{FEFF}').unwrap_or(&inhalt))
    } else {
        let mut arbeitsmappe = open_workbook_auto(pfad).map_err(|e| format!("Datei konnte nicht gelesen werden: {e}"))?;
        let mut blaetter = arbeitsmappe.sheet_names();
        if blaetter.is_empty() {
            return Err("Die Datei enthält kein Tabellenblatt".to_string());
        }
        if !alle_blaetter {
            blaetter.truncate(1);
        }
        let mut zeilen = Vec::new();
        for blatt in blaetter {
            let bereich = arbeitsmappe
                .worksheet_range(&blatt)
                .map_err(|e| format!("Tabellenblatt „{blatt}“ konnte nicht gelesen werden: {e}"))?;
            if blattnamen {
                zeilen.push(vec![BLATT_MARKE.to_string(), blatt.clone()]);
            }
            zeilen.extend(bereich.rows().map(|zeile| zeile.iter().map(zelle_zu_text).collect::<Vec<String>>()));
        }
        zeilen
    };

    Ok(tabelle.into_iter().filter(|zeile: &Vec<String>| zeile.iter().any(|z| !z.is_empty())).collect())
}

fn csv_text_als_tabelle(inhalt: &str) -> Vec<Vec<String>> {
    let zeilen: Vec<&str> = inhalt.lines().filter(|z| !z.trim().is_empty()).collect();
    if zeilen.is_empty() {
        return Vec::new();
    }
    let trenner = csv_trennzeichen_erkennen(zeilen[0]);
    zeilen.iter().map(|z| csv_zeile_spalten(z, trenner)).collect()
}

fn csv_trennzeichen_erkennen(erste_zeile: &str) -> char {
    [',', ';', '\t']
        .into_iter()
        .max_by_key(|&t| erste_zeile.matches(t).count())
        .unwrap_or(',')
}

/// Anfuehrungszeichen-fester Spalten-Zerleger, analog zu kiZeileSpalten in
/// app.js - faengt auch ein Feld ab, das selbst das Trennzeichen enthaelt
/// und deshalb in Anfuehrungszeichen steht.
fn csv_zeile_spalten(zeile: &str, trenner: char) -> Vec<String> {
    let mut ergebnis = Vec::new();
    let mut feld = String::new();
    let mut in_anfuehrung = false;
    let mut zeichen = zeile.chars().peekable();
    while let Some(c) = zeichen.next() {
        if in_anfuehrung {
            if c == '"' {
                if zeichen.peek() == Some(&'"') {
                    feld.push('"');
                    zeichen.next();
                } else {
                    in_anfuehrung = false;
                }
            } else {
                feld.push(c);
            }
        } else if c == '"' {
            in_anfuehrung = true;
        } else if c == trenner {
            ergebnis.push(std::mem::take(&mut feld));
        } else {
            feld.push(c);
        }
    }
    ergebnis.push(feld);
    ergebnis.into_iter().map(|s| s.trim().to_string()).collect()
}

fn zelle_zu_text(zelle: &Data) -> String {
    match zelle {
        Data::Empty => String::new(),
        Data::String(s) => s.trim().to_string(),
        Data::Bool(b) => if *b { "wahr" } else { "falsch" }.to_string(),
        Data::Int(i) => i.to_string(),
        // Ganzzahlige Werte ohne ".0" anzeigen - haeufig bei Telefonnummern
        // oder Kundennummern, die in Excel als Zahl statt Text drinstehen.
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                f.to_string()
            }
        }
        Data::DateTime(dt) => match dt.as_datetime() {
            Some(zeitpunkt) => {
                // Excel kennt kein reines "nur Uhrzeit"-Format - eine Zelle,
                // die nur eine Uhrzeit zeigt, landet technisch trotzdem auf
                // einem Platzhalter-Datum nahe dem Excel-Nullpunkt. WICHTIG:
                // nicht anhand des umgerechneten Datums selbst entscheiden -
                // Excels beruehmter "1900 ist ein Schaltjahr"-Fehler
                // verschiebt kleine Werte (reine Uhrzeiten, Rohwert < 1) um
                // einen Tag, das Datum landet dann auf dem 31.12.1899 statt
                // dem 30.12.1899. Stattdessen den rohen Zahlenwert selbst
                // anschauen: < 1 -> reine Uhrzeit, ganzzahlig -> reines
                // Datum, sonst Datum mit Uhrzeit.
                let roh = dt.as_f64();
                if roh < 1.0 {
                    zeitpunkt.format("%H:%M").to_string()
                } else if roh.fract() == 0.0 {
                    zeitpunkt.format("%Y-%m-%d").to_string()
                } else {
                    zeitpunkt.format("%Y-%m-%d %H:%M").to_string()
                }
            }
            None => dt.as_f64().to_string(),
        },
        Data::DateTimeIso(s) => s.clone(),
        Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("#FEHLER {e:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{ExcelDateTime, ExcelDateTimeType};
    use std::io::Write;

    fn excel_zeit(wert: f64) -> Data {
        Data::DateTime(ExcelDateTime::new(wert, ExcelDateTimeType::DateTime, false))
    }

    // Genau der Fehler, der beim Testen mit Stefans echter Stunden.xlsx
    // aufgefallen ist: eine reine Uhrzeit-Zelle landet wegen Excels
    // "1900 ist ein Schaltjahr"-Eigenheit beim Umrechnen auf dem 31.12.1899
    // statt dem erwarteten Nullpunkt 30.12.1899 - ein Datums-Vergleich auf
    // das fixe Nullpunkt-Datum erkennt sie faelschlich nicht als Uhrzeit.
    #[test]
    fn reine_uhrzeit_wird_auch_bei_der_schaltjahr_eigenheit_korrekt_erkannt() {
        assert_eq!(zelle_zu_text(&excel_zeit(0.0)), "00:00");
        assert_eq!(zelle_zu_text(&excel_zeit(0.5625)), "13:30"); // 13:30 Uhr als Bruchteil des Tages
    }

    #[test]
    fn reines_datum_ohne_uhrzeit_wird_ohne_zeitanteil_ausgegeben() {
        // Serienwert fuer ein "normales" Datum (deutlich groesser als 1 Tag) -
        // die Jahreszahl selbst ist hier nicht der Punkt, sondern dass kein
        // Zeitanteil ("00:00") an ein reines Datum drangehaengt wird.
        let text = zelle_zu_text(&excel_zeit(46000.0));
        assert_eq!(text, "2025-12-09");
        assert!(!text.contains(':'), "reines Datum darf keine Uhrzeit enthalten: {text}");
    }

    #[test]
    fn liest_eine_einfache_csv_datei() {
        let mut datei = tempfile_schreiben("Name,Telefon\nMeier,079 111 22 33\nKeller,079 444 55 66\n");
        let tabelle = datei_als_tabelle_lesen(datei.path_str(), false, false).unwrap();
        assert_eq!(tabelle, vec![
            vec!["Name".to_string(), "Telefon".to_string()],
            vec!["Meier".to_string(), "079 111 22 33".to_string()],
            vec!["Keller".to_string(), "079 444 55 66".to_string()],
        ]);
        datei.aufraeumen();
    }

    #[test]
    fn leere_zeilen_werden_uebersprungen() {
        let mut datei = tempfile_schreiben("Name,Telefon\nMeier,079 111 22 33\n,\n\nKeller,079 444 55 66\n");
        let tabelle = datei_als_tabelle_lesen(datei.path_str(), false, false).unwrap();
        assert_eq!(tabelle.len(), 3); // Kopfzeile + 2 echte Zeilen, die leere faellt raus
        datei.aufraeumen();
    }

    #[test]
    fn nicht_vorhandene_datei_gibt_verstaendlichen_fehler() {
        let ergebnis = datei_als_tabelle_lesen("/pfad/der/nicht/existiert.csv", false, false);
        assert!(ergebnis.is_err());
    }

    // Kleine Test-Hilfe: schreibt Inhalt in eine temporaere Datei im
    // System-Temp-Ordner und raeumt sie danach wieder auf.
    struct TempDatei {
        pfad: std::path::PathBuf,
    }
    impl TempDatei {
        fn path_str(&self) -> &str {
            self.pfad.to_str().unwrap()
        }
        fn aufraeumen(&mut self) {
            let _ = std::fs::remove_file(&self.pfad);
        }
    }
    fn tempfile_schreiben(inhalt: &str) -> TempDatei {
        // Eigener Zaehler noetig, nicht nur die Prozess-ID: cargo test
        // fuehrt Tests standardmaessig parallel in Threads desselben
        // Prozesses aus - ohne das wuerden sich zwei Tests denselben
        // Dateinamen teilen und sich gegenseitig die Datei unter dem
        // Lesen wegloeschen.
        static ZAEHLER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = ZAEHLER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut pfad = std::env::temp_dir();
        pfad.push(format!("atelierbuch_test_{}_{n}.csv", std::process::id()));
        let mut datei = std::fs::File::create(&pfad).unwrap();
        datei.write_all(inhalt.as_bytes()).unwrap();
        TempDatei { pfad }
    }
}
