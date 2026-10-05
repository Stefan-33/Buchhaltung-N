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

use crate::einstellungen::Einstellungen;
use crate::geschaeft::{Auftrag, Kunde};
use crate::sicherung::sicherungs_ordner;
use printpdf::{GeneratePdfOptions, PdfDocument, PdfSaveOptions};
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

fn quittung_html(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen) -> String {
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
<p style="font-size:14pt;font-weight:bold;margin:0;">{name}</p>
<p style="margin:0;color:#555555;">{zeile2}</p>
<p style="margin:0;color:#555555;">{adresse}</p>
<p style="margin:0 0 4mm 0;color:#555555;">{telefon} · {web}</p>

<p style="font-size:13pt;font-weight:bold;margin:0;">Quittung {nr} · {datum}</p>
<p style="margin:0 0 4mm 0;color:#555555;">{vorname} {kname} · {ort}</p>

<hr/>
<table style="width:100%;">
<tr style="font-weight:bold;"><td>Stück</td><td>Arbeit</td><td style="text-align:right;">à CHF</td><td style="text-align:right;">Total</td></tr>
{zeilen}
</table>
<hr/>

<p style="text-align:right;font-weight:bold;font-size:13pt;">Total · bezahlt {zahlart} &nbsp; CHF {summe:.2}</p>

<p style="font-size:9pt;color:#777777;margin-top:10mm;">{hinweis1}</p>
<p style="font-size:9pt;color:#777777;margin:0;">{hinweis2}</p>
</body></html>"#,
        name = html_escapen(&e.geschaeft_name),
        zeile2 = html_escapen(&e.geschaeft_zeile2),
        adresse = html_escapen(&e.geschaeft_adresse),
        telefon = html_escapen(&e.geschaeft_telefon),
        web = html_escapen(&e.geschaeft_web),
        nr = auftrag.rechnungsnummer,
        datum = datum_kurz(&auftrag.datum),
        vorname = html_escapen(&kunde.vorname),
        kname = html_escapen(&kunde.name),
        ort = html_escapen(&kunde.ort),
        zeilen = zeilen,
        zahlart = html_escapen(&auftrag.zahlart),
        summe = auftrag.summe,
        hinweis1 = html_escapen(&e.quittung_hinweis1),
        hinweis2 = html_escapen(&e.quittung_hinweis2),
    )
}

/// Nur die reine PDF-Erzeugung, ohne Datei-/OS-Zugriff - damit sich das
/// ohne echtes Drucker-/Betriebssystem-Verhalten testen laesst.
fn quittung_pdf_bytes(auftrag: &Auftrag, kunde: &Kunde, e: &Einstellungen) -> Result<Vec<u8>, String> {
    let html = quittung_html(auftrag, kunde, e);
    let images = BTreeMap::new();
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
    let pfad = ordner.join(format!("Quittung_{}.pdf", auftrag.rechnungsnummer));
    std::fs::write(&pfad, bytes).map_err(|e| e.to_string())?;
    Ok(pfad)
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
        }
    }

    #[test]
    fn datum_kurz_rechnet_iso_datum_ins_schweizer_format_um() {
        assert_eq!(datum_kurz("2026-10-05"), "05.10.2026");
    }

    #[test]
    fn quittung_html_enthaelt_alle_relevanten_angaben() {
        let html = quittung_html(&test_auftrag(), &test_kunde(), &Einstellungen::default());
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
        let html = quittung_html(&test_auftrag(), &test_kunde(), &Einstellungen::default());
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
}
