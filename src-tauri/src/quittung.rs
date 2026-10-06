// Erzeugt die Quittung als eigenstaendiges PDF, statt sie nur ueber
// window.print() im Programmfenster zu drucken. Grund: Stefan meldet
// einen hartnaeckigen Kopf-/Fusszeilen-Zusatz (Datum, Seitentitel,
// "tauri.localhost", Seitenzahl), den Windows' eingebauter Druckdialog
// (WebView2/Edge-Technik) selbst hinzufuegt und der sich von hier aus
// nicht abschalten laesst - nur manuell im Druckdialog selbst. Ein
// selbst erzeugtes PDF hat so etwas von vornherein nicht, weil kein
// Browser-Druckvorgang mehr involviert ist.
//
// Bewusst mit printpdf's eingebautem, sehr einfachen HTML-Renderer
// (verschachtelte Tabellen/divs werden NICHT unterstuetzt - darum hier
// flach aufgebaut, keine Wiederverwendung des app.js-HTMLs) statt Text
// und Linien von Hand zu positionieren - weniger Code, und automatischer
// Seitenumbruch waere bei einer sehr langen Quittung (viele Posten)
// gratis mit dabei.

use crate::einstellungen::{self, farbe_gueltig, Einstellungen};
use crate::geschaeft::{Auftrag, Kunde};
use crate::sicherung::sicherungs_ordner;
use printpdf::{Base64OrRaw, GeneratePdfOptions, PdfDocument, PdfSaveOptions, RawImage};
use rusqlite::Connection;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn html_escapen(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// "2026-10-05" -> "05.10.2026" - dieselbe Umrechnung wie datumKurz() in
/// app.js, nur hier auf der Rust-Seite noch einmal gebraucht.
fn datum_kurz(iso: &str) -> String {
    let teile: Vec<&str> = iso.split('-').collect();
    if teile.len() == 3 {
        format!("{}.{}.{}", teile[2], teile[1], teile[0])
    } else {
        iso.to_string()
    }
}

/// Ein bezahlter Auftrag bekommt eine Quittung, ein noch offener (per
/// Rechnung abgerechneter) eine Rechnung - sonst stuende auf dem Beleg
/// "bezahlt", obwohl noch nichts bezahlt ist.
fn beleg_titel(auftrag: &Auftrag) -> &'static str {
    if auftrag.bezahlt {
        "Quittung"
    } else {
        "Rechnung"
    }
}

fn total_text(auftrag: &Auftrag) -> String {
    if auftrag.bezahlt {
        format!("Total · bezahlt {}", auftrag.zahlart)
    } else if auftrag.zahlart == "Rechnung" {
        "Total · zahlbar per Rechnung".to_string()
    } else {
        "Total · noch offen".to_string()
    }
}

/// Baut den Quittungskopf (Logo, Name, Zeile 2, Adresse, Telefon/Web) -
/// eine Zeile wird nur ausgegeben, wenn in den Einstellungen dafuer auch
/// ein Text hinterlegt ist. So kann Stefan ueber "Einstellungen" selbst
/// bestimmen, was auf der Quittung steht: ein leer gelassenes Feld
/// erscheint dort einfach nicht.
fn quittung_kopf(e: &Einstellungen, logo_groesse_mm: Option<(f32, f32)>) -> String {
    let mut kopf = String::new();
    if let Some((breite, hoehe)) = logo_groesse_mm {
        kopf.push_str(&format!("<img src=\"logo\" style=\"width:{breite}mm;height:{hoehe}mm;margin:0 0 3mm 0;\"/>\n"));
    }
    kopf.push_str(&format!(
        "<p style=\"font-size:14pt;font-weight:bold;margin:0;\">{}</p>\n",
        html_escapen(&e.geschaeft_name)
    ));
    if !e.geschaeft_zeile2.trim().is_empty() {
        kopf.push_str(&format!("<p style=\"margin:0;color:#555555;\">{}</p>\n", html_escapen(&e.geschaeft_zeile2)));
    }
    if !e.geschaeft_adresse.trim().is_empty() {
        kopf.push_str(&format!("<p style=\"margin:0;color:#555555;\">{}</p>\n", html_escapen(&e.geschaeft_adresse)));
    }
    let kontakt = match (e.geschaeft_telefon.trim(), e.geschaeft_web.trim()) {
        ("", "") => String::new(),
        (t, "") => t.to_string(),
        ("", w) => w.to_string(),
        (t, w) => format!("{t} · {w}"),
    };
    if !kontakt.is_empty() {
        kopf.push_str(&format!("<p style=\"margin:0 0 4mm 0;color:#555555;\">{}</p>\n", html_escapen(&kontakt)));
    }
    kopf
}

/// Das Kleingedruckte ganz unten - genau wie beim Kopf: eine leere Zeile
/// in den Einstellungen erscheint auf der Quittung gar nicht erst.
fn quittung_fuss(e: &Einstellungen) -> String {
    let mut fuss = String::new();
    if !e.quittung_hinweis1.trim().is_empty() {
        fuss.push_str(&format!(
            "<p style=\"font-size:9pt;color:#777777;margin-top:10mm;\">{}</p>\n",
            html_escapen(&e.quittung_hinweis1)
        ));
    }
    if !e.quittung_hinweis2.trim().is_empty() {
        fuss.push_str(&format!("<p style=\"font-size:9pt;color:#777777;margin:0;\">{}</p>\n", html_escapen(&e.quittung_hinweis2)));
    }
    fuss
}

/// Berechnet Breite/Hoehe (in mm) fuer das Logo im Quittungskopf, so dass
/// es seine Bildproportionen behaelt (sonst verzerrt printpdf's einfacher
/// HTML-Renderer das Bild, wenn im CSS nur eine Seite angegeben ist) -
/// maximal 28mm breit, maximal 20mm hoch, je nachdem was zuerst zuschlaegt.
fn logo_abmessung_mm(breite_px: usize, hoehe_px: usize) -> Option<(f32, f32)> {
    if breite_px == 0 || hoehe_px == 0 {
        return None;
    }
    let seitenverhaeltnis = hoehe_px as f32 / breite_px as f32;
    let (max_breite, max_hoehe) = (28.0_f32, 20.0_f32);
    let mut breite = max_breite;
    let mut hoehe = breite * seitenverhaeltnis;
    if hoehe > max_hoehe {
        hoehe = max_hoehe;
        breite = hoehe / seitenverhaeltnis;
    }
    Some((breite, hoehe))
}

/// Zahlungshinweis (z.B. Frist, IBAN) - nur auf einer noch offenen Rechnung.
fn zahlungshinweis_html(auftrag: &Auftrag, e: &Einstellungen) -> String {
    if auftrag.bezahlt || e.beleg_zahlungshinweis.trim().is_empty() {
        return String::new();
    }
    format!("<p style=\"font-size:10pt;margin:3mm 0 0 0;\">{}</p>\n", html_escapen(&e.beleg_zahlungshinweis))
}

/// Teilt eine Einstellungen-Zeile wie "Staldenbachstrasse 13, 8808 Pfaeffikon"
/// oder "Aenderungen · Rosmarie Straub" fuer den klassischen Kopf auf
/// mehrere Zeilen auf - so wie auf der bisherigen Excel-Rechnung.
fn zeilen_aufteilen(text: &str) -> Vec<String> {
    text.split(" · ")
        .flat_map(|teil| teil.split(", "))
        .map(|z| z.trim().to_string())
        .filter(|z| !z.is_empty())
        .collect()
}

/// Vorlage "Klassisch": nachgebaut nach Stefans bisheriger Excel-Rechnung
/// (Logo links, Adresse daneben, Kontakt rechts, farbiger Balken, Tabelle
/// mit Linien, "Besten Dank" neben dem Total). Nur flache Tabellen mit
/// reinem Text/Bild in den Zellen - das kann printpdf zuverlaessig.
fn klassisch_html(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen, logo_groesse_mm: Option<(f32, f32)>) -> String {
    const ZELLE: &str = "border:1px solid #333333;padding:1mm;";
    let farbe = if farbe_gueltig(&e.beleg_farbe) { e.beleg_farbe.as_str() } else { "#92D050" };

    let mut links = vec![format!("<b>{}</b>", html_escapen(&e.geschaeft_name))];
    for text in [&e.geschaeft_zeile2, &e.geschaeft_adresse] {
        links.extend(zeilen_aufteilen(text).iter().map(|z| html_escapen(z)));
    }
    let mut rechts = Vec::new();
    if !e.geschaeft_telefon.trim().is_empty() {
        rechts.push(format!("Tel. {}", html_escapen(e.geschaeft_telefon.trim())));
    }
    for text in [&e.geschaeft_email, &e.geschaeft_web] {
        if !text.trim().is_empty() {
            rechts.push(html_escapen(text.trim()));
        }
    }
    let logo_zelle = match logo_groesse_mm {
        Some((b, h)) => format!(
            "<td style=\"width:{}mm;vertical-align:top;\"><img src=\"logo\" style=\"width:{b}mm;height:{h}mm;\"/></td>",
            b + 4.0
        ),
        None => String::new(),
    };

    let zeilen: String = auftrag
        .posten
        .iter()
        .map(|p| {
            format!(
                "<tr><td style=\"{ZELLE}\">{}</td><td style=\"{ZELLE}\">{}</td><td style=\"{ZELLE}text-align:right;\">{:.2}</td><td style=\"{ZELLE}text-align:right;\">{:.2}</td></tr>",
                p.stueck,
                html_escapen(&p.bezeichnung),
                p.preis,
                p.stueck * p.preis
            )
        })
        .collect();

    let zahlung = if auftrag.bezahlt {
        format!("bezahlt {}", html_escapen(&auftrag.zahlart))
    } else if auftrag.zahlart == "Rechnung" {
        "zahlbar per Rechnung".to_string()
    } else {
        "noch offen".to_string()
    };

    format!(
        r#"<html><body style="padding:8mm;font-family:sans-serif;font-size:10pt;">
<table style="width:100%;border-collapse:collapse;">
<tr>{logo_zelle}<td style="vertical-align:top;">{links}</td><td style="vertical-align:bottom;text-align:right;">{rechts}</td></tr>
</table>
<div style="background-color:{farbe};height:4mm;margin:2mm 0 3mm 0;"></div>
<table style="width:100%;">
<tr><td style="font-size:14pt;font-weight:bold;">{titel} {nr}</td><td style="text-align:right;font-weight:bold;">{zusatz}</td></tr>
</table>
<table style="width:100%;margin-top:2mm;">
<tr><td style="width:14mm;color:#555555;">Name</td><td>{vorname} {kname} (Nr. {knr})</td><td style="width:10mm;color:#555555;">Tel</td><td>{ktel}</td><td style="text-align:right;color:#555555;">Datum</td></tr>
<tr><td style="color:#555555;">Ort</td><td>{ort}</td><td style="color:#555555;">Mail</td><td>{kmail}</td><td style="text-align:right;">{datum}</td></tr>
</table>
<table style="width:100%;border-collapse:collapse;margin-top:4mm;">
<tr style="font-weight:bold;"><td style="{ZELLE}">Stück</td><td style="{ZELLE}">Arbeit</td><td style="{ZELLE}text-align:right;">à</td><td style="{ZELLE}text-align:right;">CHF</td></tr>
{zeilen}
<tr><td style="padding:1mm;" colspan="2">{dank}</td><td style="padding:1mm;text-align:right;font-weight:bold;">Total</td><td style="{ZELLE}text-align:right;font-weight:bold;">{summe:.2}</td></tr>
</table>
<p style="text-align:right;margin:2mm 0 0 0;">{zahlung}</p>
{zahlungshinweis}
{fuss}
</body></html>"#,
        links = links.join("<br/>"),
        rechts = rechts.join("<br/>"),
        titel = beleg_titel(auftrag),
        nr = auftrag.rechnungsnummer,
        zusatz = html_escapen(&e.beleg_titel_zusatz),
        vorname = html_escapen(&kunde.vorname),
        kname = html_escapen(&kunde.name),
        knr = kunde.nummer,
        ktel = html_escapen(&kunde.telefon),
        ort = html_escapen(&kunde.ort),
        kmail = html_escapen(&kunde.email),
        datum = datum_kurz(&auftrag.datum),
        dank = html_escapen(&e.beleg_dank),
        summe = auftrag.summe,
        zahlungshinweis = zahlungshinweis_html(auftrag, e),
        fuss = quittung_fuss(e),
    )
}

fn quittung_html(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen, logo_groesse_mm: Option<(f32, f32)>) -> String {
    if e.beleg_vorlage == "schlicht" {
        schlicht_html(auftrag, kunde, e, logo_groesse_mm)
    } else {
        klassisch_html(auftrag, kunde, e, logo_groesse_mm)
    }
}

/// Vorlage "Schlicht": das einfache Layout ohne Linien.
fn schlicht_html(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen, logo_groesse_mm: Option<(f32, f32)>) -> String {
    let zeilen: String = auftrag
        .posten
        .iter()
        .map(|p| {
            format!(
                "<tr><td>{}</td><td>{}</td><td style=\"text-align:right;\">{:.2}</td><td style=\"text-align:right;\">{:.2}</td></tr>",
                p.stueck,
                html_escapen(&p.bezeichnung),
                p.preis,
                p.stueck * p.preis
            )
        })
        .collect();

    format!(
        r#"<html><body style="padding:8mm;font-family:sans-serif;font-size:11pt;">
{kopf}
<p style="font-size:13pt;font-weight:bold;margin:0;">{titel} {nr} · {datum}</p>
<p style="margin:0 0 4mm 0;color:#555555;">{vorname} {kname} · {ort}</p>

<hr/>
<table style="width:100%;">
<tr style="font-weight:bold;"><td>Stück</td><td>Arbeit</td><td style="text-align:right;">à CHF</td><td style="text-align:right;">Total</td></tr>
{zeilen}
</table>
<hr/>

<p style="text-align:right;font-weight:bold;font-size:13pt;">{total_text} &nbsp; CHF {summe:.2}</p>
{zahlungshinweis}
{fuss}
</body></html>"#,
        kopf = quittung_kopf(e, logo_groesse_mm),
        titel = beleg_titel(auftrag),
        nr = auftrag.rechnungsnummer,
        datum = datum_kurz(&auftrag.datum),
        vorname = html_escapen(&kunde.vorname),
        kname = html_escapen(&kunde.name),
        ort = html_escapen(&kunde.ort),
        zeilen = zeilen,
        total_text = html_escapen(&total_text(auftrag)),
        summe = auftrag.summe,
        zahlungshinweis = zahlungshinweis_html(auftrag, e),
        fuss = quittung_fuss(e),
    )
}

/// Seitengroesse in mm je nach gewaehltem Papierformat.
fn seite_mm(e: &Einstellungen) -> (f32, f32) {
    if e.beleg_format == "A4" {
        (210.0, 297.0)
    } else {
        (148.0, 210.0)
    }
}

/// Nur die reine PDF-Erzeugung, ohne Datei-/OS-Zugriff - damit sich das
/// ohne echtes Drucker-/Betriebssystem-Verhalten testen laesst.
fn quittung_pdf_bytes(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen) -> Result<Vec<u8>, String> {
    let mut images: BTreeMap<String, Base64OrRaw> = BTreeMap::new();
    let mut logo_groesse_mm = None;
    // Falls die Logo-Datei zwischenzeitlich verschoben/geloescht wurde oder
    // kein gueltiges Bild (mehr) ist, die Quittung trotzdem ohne Logo
    // erzeugen statt abzubrechen.
    if let Some(logo_pfad) = e.quittung_logo_pfad.as_deref().filter(|p| !p.trim().is_empty()) {
        if let Ok(bytes) = std::fs::read(logo_pfad) {
            let mut bild_warnungen = Vec::new();
            if let Ok(raw) = RawImage::decode_from_bytes(&bytes, &mut bild_warnungen) {
                logo_groesse_mm = logo_abmessung_mm(raw.width, raw.height);
                images.insert("logo".to_string(), Base64OrRaw::Raw(bytes));
            }
        }
    }
    let html = quittung_html(auftrag, kunde, e, logo_groesse_mm);
    let fonts = BTreeMap::new();
    let (breite, hoehe) = seite_mm(e);
    let options = GeneratePdfOptions { page_width: Some(breite), page_height: Some(hoehe), ..Default::default() };

    let mut warnungen = Vec::new();
    let doc = PdfDocument::from_html(&html, &images, &fonts, &options, &mut warnungen)
        .map_err(|e| format!("PDF konnte nicht erzeugt werden: {e}"))?;
    let mut speicher_warnungen = Vec::new();
    Ok(doc.save(&PdfSaveOptions::default(), &mut speicher_warnungen))
}

/// Erzeugt die Quittung als PDF und legt sie unter "Dokumente \
/// Atelierbuch Straub \ Sicherung \ Quittungen" ab - landet damit gleich
/// im selben Ordner wie die anderen Exporte, als Nebeneffekt auch ein
/// automatisches PDF-Archiv jeder ausgestellten Quittung.
pub fn quittung_pdf_erzeugen(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen) -> Result<PathBuf, String> {
    let bytes = quittung_pdf_bytes(auftrag, kunde, e)?;
    let ordner = sicherungs_ordner().join("Quittungen");
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let pfad = ordner.join(format!("{}_{}.pdf", beleg_titel(auftrag), auftrag.rechnungsnummer));
    std::fs::write(&pfad, bytes).map_err(|e| e.to_string())?;
    Ok(pfad)
}

/// Muster-Beleg mit Beispieldaten fuer "Beleg-Design -> Muster ansehen":
/// zeigt die (noch nicht gespeicherten) Einstellungen aus dem Formular als
/// echte PDF, damit man vor dem Speichern sieht, wie es aussieht. Als
/// offene Rechnung, damit auch der Zahlungshinweis sichtbar ist.
pub fn muster_pdf_erzeugen(e: &Einstellungen) -> Result<PathBuf, String> {
    use crate::geschaeft::Posten;
    let auftrag = Auftrag {
        id: 0,
        rechnungsnummer: 1234,
        datum: chrono::Local::now().format("%Y-%m-%d").to_string(),
        zahlart: "Rechnung".into(),
        summe: 55.0,
        posten: vec![
            Posten { bezeichnung: "Reissverschluss ersetzen".into(), stueck: 1.0, preis: 35.0 },
            Posten { bezeichnung: "Hose kürzen".into(), stueck: 2.0, preis: 10.0 },
        ],
        status: "Abgeholt".into(),
        abholdatum: None,
        angenommen_am: None,
        bezahlt: false,
        bezahlt_am: None,
    };
    let kunde = Kunde {
        id: 0,
        nummer: 101,
        name: "Muster".into(),
        vorname: "Anna".into(),
        telefon: "079 123 45 67".into(),
        ort: "Pfäffikon".into(),
        adresse: "".into(),
        email: "anna.muster@example.ch".into(),
        kartensatz: None,
        archiviert: false,
        notiz: "".into(),
        jahresumsatz: 0.0,
        anzahl_auftraege: 0,
        letzter_besuch: None,
        offen_summe: 0.0,
    };
    let bytes = quittung_pdf_bytes(&auftrag, &kunde, e)?;
    let ordner = sicherungs_ordner().join("Quittungen");
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let pfad = ordner.join("Muster.pdf");
    // Ist die vorherige Muster.pdf noch im PDF-Programm offen (gesperrt),
    // eine neue Datei daneben anlegen statt abzubrechen.
    if std::fs::write(&pfad, &bytes).is_ok() {
        return Ok(pfad);
    }
    let ausweich = ordner.join(format!("Muster_{}.pdf", chrono::Local::now().format("%H%M%S")));
    std::fs::write(&ausweich, &bytes).map_err(|e| e.to_string())?;
    Ok(ausweich)
}

/// Kopiert eine vom nativen Dateidialog ausgewaehlte Foto-/Logo-Datei in
/// den Sicherungsordner (immer als "logo.<Endung>", ueberschreibt ein
/// evtl. vorhandenes altes Logo - es gibt immer nur eines) und hinterlegt
/// den Pfad in den Einstellungen, damit kuenftige Quittungen es zeigen.
pub fn logo_setzen(conn: &Connection, quelle: &str) -> Result<String, String> {
    let quelle_pfad = std::path::Path::new(quelle);
    let endung = quelle_pfad.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let ordner = sicherungs_ordner().join("Logo");
    std::fs::create_dir_all(&ordner).map_err(|e| e.to_string())?;
    let ziel = ordner.join(format!("logo.{endung}"));
    std::fs::copy(quelle_pfad, &ziel).map_err(|e| e.to_string())?;
    let ziel_text = ziel.display().to_string();

    let mut einstellungen = einstellungen::einstellungen_lesen(conn).map_err(|e| e.to_string())?;
    einstellungen.quittung_logo_pfad = Some(ziel_text.clone());
    einstellungen::einstellungen_speichern(conn, &einstellungen).map_err(|e| e.to_string())?;
    Ok(ziel_text)
}

/// Entfernt das Logo wieder von der Quittung (die Datei im
/// Sicherungsordner bleibt bewusst liegen statt geloescht zu werden -
/// falls Stefan es sich anders ueberlegt, muss er nicht neu hochladen).
pub fn logo_entfernen(conn: &Connection) -> Result<(), String> {
    let mut einstellungen = einstellungen::einstellungen_lesen(conn).map_err(|e| e.to_string())?;
    einstellungen.quittung_logo_pfad = None;
    einstellungen::einstellungen_speichern(conn, &einstellungen).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geschaeft::Posten;

    fn test_auftrag() -> Auftrag {
        Auftrag {
            id: 1,
            rechnungsnummer: 1259,
            datum: "2026-10-05".into(),
            zahlart: "Bar".into(),
            summe: 43.5,
            posten: vec![
                Posten { bezeichnung: "Hose kürzen".into(), stueck: 1.0, preis: 25.0 },
                Posten { bezeichnung: "Reissverschluss ersetzen".into(), stueck: 1.0, preis: 18.5 },
            ],
            status: "Abgeholt".into(),
            abholdatum: None,
            angenommen_am: None,
            bezahlt: true,
            bezahlt_am: Some("2026-10-05".into()),
        }
    }

    fn test_kunde() -> Kunde {
        Kunde {
            id: 1,
            nummer: 101,
            name: "Meier".into(),
            vorname: "Hans".into(),
            telefon: "".into(),
            ort: "Pfäffikon".into(),
            adresse: "".into(),
            email: "".into(),
            kartensatz: None,
            archiviert: false,
            notiz: "".into(),
            jahresumsatz: 0.0,
            anzahl_auftraege: 1,
            letzter_besuch: None,
            offen_summe: 0.0,
        }
    }

    #[test]
    fn datum_kurz_rechnet_iso_datum_ins_schweizer_format_um() {
        assert_eq!(datum_kurz("2026-10-05"), "05.10.2026");
    }

    fn mit_vorlage(vorlage: &str) -> Einstellungen {
        Einstellungen { beleg_vorlage: vorlage.into(), ..Einstellungen::default() }
    }

    #[test]
    fn bezahlter_auftrag_wird_in_beiden_vorlagen_als_quittung_mit_zahlart_gedruckt() {
        for vorlage in ["klassisch", "schlicht"] {
            let html = quittung_html(&test_auftrag(), &test_kunde(), &mit_vorlage(vorlage), None);
            assert!(html.contains("Quittung 1259"), "{vorlage}");
            assert!(html.contains("bezahlt Bar"), "{vorlage}");
            assert!(html.contains("Hose kürzen") && html.contains("43.50"), "{vorlage}");
        }
    }

    // Nachgebaut nach Stefans alter Excel-Rechnung: farbiger Balken,
    // "für Aenderungen / Reparaturen", "Besten Dank", Adresse auf mehreren
    // Zeilen, Kontakt (Tel./E-Mail/Web) rechts.
    #[test]
    fn klassische_vorlage_hat_balken_titelzusatz_dank_und_aufgeteilte_adresse() {
        let mut e = mit_vorlage("klassisch");
        e.geschaeft_email = "naehservice@example.ch".into();
        let html = quittung_html(&test_auftrag(), &test_kunde(), &e, None);
        assert!(html.contains("background-color:#92D050"));
        assert!(html.contains("für Aenderungen / Reparaturen"));
        assert!(html.contains("Besten Dank"));
        assert!(html.contains("Staldenbachstrasse 13<br/>8808 Pfäffikon SZ"));
        assert!(html.contains("Tel. 055 410 72 06<br/>naehservice@example.ch<br/>naehservicestraub.ch"));
        assert!(html.contains("(Nr. 101)"), "Kundennummer auf der Rechnung");
    }

    // Die Farbe landet direkt im HTML - eine ungueltige (z.B. manipulierte)
    // Angabe darf dort nie unveraendert auftauchen.
    #[test]
    fn ungueltige_farbe_faellt_auf_standardgruen_zurueck() {
        let mut e = mit_vorlage("klassisch");
        e.beleg_farbe = "red;\"><script>".into();
        let html = quittung_html(&test_auftrag(), &test_kunde(), &e, None);
        assert!(!html.contains("<script>"));
        assert!(html.contains("background-color:#92D050"));
    }

    #[test]
    fn zahlungshinweis_nur_auf_offener_rechnung() {
        for vorlage in ["klassisch", "schlicht"] {
            let mut e = mit_vorlage(vorlage);
            e.beleg_zahlungshinweis = "Zahlbar innert 30 Tagen".into();
            let bezahlt = quittung_html(&test_auftrag(), &test_kunde(), &e, None);
            assert!(!bezahlt.contains("Zahlbar innert 30 Tagen"), "{vorlage}");

            let mut offen = test_auftrag();
            offen.zahlart = "Rechnung".into();
            offen.bezahlt = false;
            let html = quittung_html(&offen, &test_kunde(), &e, None);
            assert!(html.contains("Zahlbar innert 30 Tagen"), "{vorlage}");
        }
    }

    #[test]
    fn beide_vorlagen_und_formate_ergeben_gueltige_pdfs() {
        for vorlage in ["klassisch", "schlicht"] {
            for format in ["A5", "A4"] {
                let e = Einstellungen { beleg_format: format.into(), ..mit_vorlage(vorlage) };
                let bytes = quittung_pdf_bytes(&test_auftrag(), &test_kunde(), &e).unwrap();
                assert!(bytes.starts_with(b"%PDF"), "{vorlage} {format}");
            }
        }
        assert_eq!(seite_mm(&Einstellungen { beleg_format: "A4".into(), ..Einstellungen::default() }), (210.0, 297.0));
        assert_eq!(seite_mm(&Einstellungen::default()), (148.0, 210.0));
    }

    // Stefans Wunsch: ein per Rechnung abgerechneter, noch offener Auftrag
    // darf auf dem Beleg nicht "bezahlt" heissen.
    #[test]
    fn offener_auftrag_wird_als_rechnung_ohne_bezahlt_vermerk_gedruckt() {
        let mut auftrag = test_auftrag();
        auftrag.zahlart = "Rechnung".into();
        auftrag.bezahlt = false;
        auftrag.bezahlt_am = None;
        let html = quittung_html(&auftrag, &test_kunde(), &Einstellungen::default(), None);
        assert!(html.contains("Rechnung 1259"));
        assert!(html.contains("zahlbar per Rechnung"));
        assert!(!html.contains("bezahlt"), "darf nirgends 'bezahlt' stehen");
    }

    #[test]
    fn quittung_html_enthaelt_alle_relevanten_angaben() {
        let html = quittung_html(&test_auftrag(), &test_kunde(), &Einstellungen::default(), None);
        assert!(html.contains("Quittung 1259"));
        assert!(html.contains("Hans"));
        assert!(html.contains("Meier"));
        assert!(html.contains("Hose kürzen"));
        assert!(html.contains("43.50"));
        assert!(html.contains("Nähservice Straub"));
    }

    // Stefan meldet per Screenshot: "&middot;" taucht woertlich in der
    // gedruckten Quittung auf, statt als "·" angezeigt zu werden - printpdf's
    // eingebauter HTML-Renderer kennt benannte HTML-Entities wie "&middot;"
    // nicht (nur ein paar wenige Grundlegende). Fix: das Trennzeichen direkt
    // als UTF-8-Zeichen im Text statt als Entity - diese Regression darf
    // nicht wiederkommen.
    #[test]
    fn trennzeichen_wird_als_echtes_zeichen_und_nicht_als_entity_geschrieben() {
        let html = quittung_html(&test_auftrag(), &test_kunde(), &Einstellungen::default(), None);
        assert!(!html.contains("&middot;"), "waere woertlich in der Quittung zu sehen");
        assert!(html.contains('·'), "das Trennzeichen muss trotzdem vorkommen");
    }

    // Genau der Fall, der ueberhaupt erst zu diesem Modul gefuehrt hat:
    // eine gueltige, einseitige PDF-Datei ohne Browser-Kopf-/Fusszeile.
    #[test]
    fn quittung_pdf_bytes_liefert_eine_gueltige_pdf_datei() {
        let bytes = quittung_pdf_bytes(&test_auftrag(), &test_kunde(), &Einstellungen::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"), "muss mit der PDF-Kennung beginnen");
        assert!(bytes.len() > 500, "verdaechtig kleine Datei: {} Bytes", bytes.len());
    }

    // Auch mit sehr vielen Posten darf es nicht abstuerzen (automatischer
    // Seitenumbruch statt eines Fehlers bei einer langen Quittung).
    #[test]
    fn quittung_pdf_bytes_kommt_auch_mit_vielen_posten_klar() {
        let mut auftrag = test_auftrag();
        auftrag.posten = (0..40)
            .map(|i| Posten { bezeichnung: format!("Posten {i}"), stueck: 1.0, preis: 10.0 })
            .collect();
        let bytes = quittung_pdf_bytes(&auftrag, &test_kunde(), &Einstellungen::default()).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    // Stefans Wunsch: "wo was steht" selbst bestimmen koennen - ein in den
    // Einstellungen leer gelassenes Feld darf auf der Quittung gar nicht
    // erst als Zeile erscheinen (nicht nur als leere Zeile).
    #[test]
    fn leer_gelassene_felder_erscheinen_nicht_auf_der_quittung() {
        let mut e = Einstellungen::default();
        e.geschaeft_zeile2 = "".into();
        e.geschaeft_adresse = "".into();
        e.geschaeft_telefon = "".into();
        e.geschaeft_web = "".into();
        e.quittung_hinweis1 = "".into();
        e.quittung_hinweis2 = "".into();
        let html = quittung_html(&test_auftrag(), &test_kunde(), &e, None);
        assert!(html.contains("Nähservice Straub"), "der Name bleibt, nur die leeren Felder fallen weg");
        assert!(!html.contains("Änderungen und Reparaturen"));
        assert!(!html.contains("Staldenbachstrasse"));
        assert!(!html.contains("055 410"));
        assert!(!html.contains("naehservicestraub.ch"));
        assert!(!html.contains("Reklamationen"));
        assert!(!html.contains("Kundenexemplar"));
    }

    #[test]
    fn ohne_logo_erscheint_kein_bild_im_quittungskopf() {
        let html = quittung_html(&test_auftrag(), &test_kunde(), &Einstellungen::default(), None);
        assert!(!html.contains("<img"));
    }

    // Das Seitenverhaeltnis muss erhalten bleiben (sonst verzerrt printpdf's
    // einfacher Renderer das Bild, wenn im CSS nur eine Seite steht) - ein
    // doppelt so breites wie hohes Bild bleibt auch im Quittungskopf 2:1.
    #[test]
    fn logo_abmessung_behaelt_das_seitenverhaeltnis_und_haelt_sich_an_die_maximalgroesse() {
        assert_eq!(logo_abmessung_mm(300, 150), Some((28.0, 14.0)));
        // sehr hohes, schmales Bild -> die Hoehe begrenzt statt der Breite
        let (b, h) = logo_abmessung_mm(100, 400).unwrap();
        assert!((h - 20.0).abs() < 0.01);
        assert!((b - 5.0).abs() < 0.01);
        assert_eq!(logo_abmessung_mm(0, 100), None);
    }

    #[test]
    fn mit_hinterlegtem_logo_erscheint_das_bild_im_kopf_und_im_pdf() {
        let logo_pfad = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/logo_test.png");
        let mut e = Einstellungen::default();
        e.quittung_logo_pfad = Some(logo_pfad.display().to_string());

        let html = quittung_html(&test_auftrag(), &test_kunde(), &e, logo_abmessung_mm(300, 150));
        assert!(html.contains("<img src=\"logo\""));

        let bytes = quittung_pdf_bytes(&test_auftrag(), &test_kunde(), &e).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }

    // Die hinterlegte Datei kann zwischenzeitlich verschoben oder geloescht
    // worden sein - die Quittung muss trotzdem erzeugt werden, nur ohne Bild.
    #[test]
    fn fehlende_logo_datei_laesst_die_pdf_erzeugung_nicht_abstuerzen() {
        let mut e = Einstellungen::default();
        e.quittung_logo_pfad = Some("/pfad/der/nicht/existiert.png".into());
        let bytes = quittung_pdf_bytes(&test_auftrag(), &test_kunde(), &e).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
    }
}
