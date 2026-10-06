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

use crate::einstellungen::{self, Einstellungen};
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

fn quittung_html(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen, logo_groesse_mm: Option<(f32, f32)>) -> String {
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
        fuss = quittung_fuss(e),
    )
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
    // A5 (148 x 210 mm) - siehe styles.css @page-Regel fuer die Browser-
    // Druckvorschau, hier dieselbe Groesse fuer das PDF.
    let options = GeneratePdfOptions { page_width: Some(148.0), page_height: Some(210.0), ..Default::default() };

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

    #[test]
    fn bezahlter_auftrag_wird_als_quittung_mit_zahlart_gedruckt() {
        let html = quittung_html(&test_auftrag(), &test_kunde(), &Einstellungen::default(), None);
        assert!(html.contains("Quittung 1259"));
        assert!(html.contains("Total · bezahlt Bar"));
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
