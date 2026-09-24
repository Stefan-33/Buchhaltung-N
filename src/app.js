(function () {
  "use strict";

  var invoke = window.__TAURI__.core.invoke;

  var fr = new Intl.NumberFormat("de-CH", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  function chf(n) { return fr.format(n || 0); }

  function datumKurz(iso) {
    // Rust liefert Datum als "YYYY-MM-DD" - fuer die Anzeige ins
    // gewohnte Schweizer Format drehen.
    if (!iso) return "";
    var teile = iso.split("-");
    return teile[2] + "." + teile[1] + "." + teile[0].slice(2);
  }

  function fehlerText(e) {
    // Tauri liefert Fehler aus Rust als reinen String (siehe Antwort<T> =
    // Result<T, String> in commands.rs).
    return typeof e === "string" ? e : "Unerwarteter Fehler.";
  }

  // ================= LOGIN =================
  var aktuellerBenutzer = null;

  var elLoginBuehne = document.getElementById("login-bildschirm");
  var elProgramm = document.getElementById("programm");
  var elLoginFormular = document.getElementById("login-formular");
  var elLoginFehler = document.getElementById("login-fehler");

  elLoginFormular.addEventListener("submit", function (ev) {
    ev.preventDefault();
    elLoginFehler.hidden = true;
    var benutzername = document.getElementById("login-benutzername").value.trim();
    var passwort = document.getElementById("login-passwort").value;

    invoke("anmelden", { benutzername: benutzername, passwort: passwort })
      .then(function (benutzer) {
        aktuellerBenutzer = benutzer;
        elLoginBuehne.hidden = true;
        elProgramm.hidden = false;
        document.getElementById("angemeldet-als").textContent = "Angemeldet: " + benutzer.anzeigename;
        programmStarten();
      })
      .catch(function (e) {
        elLoginFehler.textContent = fehlerText(e);
        elLoginFehler.hidden = false;
        document.getElementById("login-passwort").value = "";
        document.getElementById("login-passwort").focus();
      });
  });

  document.getElementById("abmeldenKnopf").addEventListener("click", function () {
    aktuellerBenutzer = null;
    elProgramm.hidden = true;
    elLoginBuehne.hidden = false;
    elLoginFormular.reset();
    document.getElementById("login-benutzername").focus();
  });

  // ================= KUNDENLISTE =================
  var elListe = document.getElementById("liste");
  var elSuche = document.getElementById("suche");
  var elArchiv = document.getElementById("archivAn");
  var elTreffer = document.getElementById("treffer");
  var elBlatt = document.getElementById("kundenblatt");

  var kundenCache = [];   // letzte Suchergebnisse, fuer die Export-Vorschau
  var gewaehlteId = null;
  var exportOffen = false;

  function suchtextSuchen() {
    invoke("kunden_suchen", { suchtext: elSuche.value.trim(), archiv_zeigen: elArchiv.checked })
      .then(function (kunden) {
        kundenCache = kunden;
        listeZeichnen(kunden);
        if (exportOffen) exportZeichnen();
      })
      .catch(function (e) { elListe.innerHTML = '<p class="leer">' + fehlerText(e) + "</p>"; });
  }

  function listeZeichnen(kunden) {
    elTreffer.textContent = kunden.length + (kunden.length === 1 ? " Kunde" : " Kunden");

    if (!kunden.length) {
      elListe.innerHTML = '<p class="leer">Niemand gefunden.<br>Mit „+ Neuer Kunde" unten anlegen.</p>';
      return;
    }
    elListe.innerHTML = "";
    kunden.forEach(function (k) {
      var b = document.createElement("button");
      b.className = "zeile";
      b.type = "button";
      if (k.id === gewaehlteId) b.setAttribute("aria-current", "true");
      var merkmal = k.archiviert ? '<span class="merkmal m-archiv">Archiv</span>' : "";
      b.innerHTML =
        '<span><span class="zeile-name">' + escapeHtml(k.name) + " " + escapeHtml(k.vorname) + "</span>" +
        '<br><span class="zeile-ort">Nr. ' + k.nummer + " · " + escapeHtml(k.ort) + " · " + escapeHtml(k.telefon) + "</span></span>" +
        merkmal +
        '<span class="zeile-betrag">' + chf(k.jahresumsatz) + "</span>";
      b.addEventListener("click", function () { gewaehlteId = k.id; listeZeichnen(kundenCache); kundeLaden(k.id); });
      elListe.appendChild(b);
    });
  }

  function escapeHtml(s) {
    return String(s || "").replace(/[&<>"']/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c];
    });
  }

  elSuche.addEventListener("input", debounce(suchtextSuchen, 120));
  elArchiv.addEventListener("change", suchtextSuchen);

  function debounce(fn, ms) {
    var timer = null;
    return function () {
      clearTimeout(timer);
      var args = arguments;
      timer = setTimeout(function () { fn.apply(null, args); }, ms);
    };
  }

  // ================= NEUER KUNDE =================
  var elDialog = document.getElementById("neuerKundeDialog");
  document.getElementById("neuerKundeKnopf").addEventListener("click", function () {
    document.getElementById("neuerKundeFormular").reset();
    elDialog.showModal();
    document.getElementById("nk-name").focus();
  });
  document.getElementById("nk-abbrechen").addEventListener("click", function () { elDialog.close(); });

  document.getElementById("neuerKundeFormular").addEventListener("submit", function (ev) {
    ev.preventDefault();
    var eingabe = {
      name: document.getElementById("nk-name").value.trim(),
      vorname: document.getElementById("nk-vorname").value.trim(),
      telefon: document.getElementById("nk-telefon").value.trim(),
      ort: document.getElementById("nk-ort").value.trim(),
      adresse: document.getElementById("nk-adresse").value.trim(),
      email: document.getElementById("nk-email").value.trim(),
    };
    if (!eingabe.name) return;
    invoke("kunde_anlegen", { eingabe: eingabe })
      .then(function (kunde) {
        elDialog.close();
        gewaehlteId = kunde.id;
        elSuche.value = "";
        suchtextSuchen();
        kundeLaden(kunde.id);
      })
      .catch(function (e) { alert(fehlerText(e)); });
  });

  // ================= KUNDENBLATT =================
  var aktuellerKunde = null;
  var aktuelleAuftraege = [];
  var posten = [];
  var zahlart = "Bar";
  var letzteQuittung = null; // gebuchter Auftrag, dessen Beleg gerade angezeigt/gedruckt wird

  function neuePostenzeile() { return { bezeichnung: "", stueck: 1, preis: 0 }; }

  function kundeLaden(id) {
    posten = [neuePostenzeile()];
    zahlart = "Bar";
    letzteQuittung = null;
    elBlatt.innerHTML = '<p class="leer" style="padding:40px">Lade…</p>';

    Promise.all([invoke("kunde_holen", { id: id }), invoke("auftraege_von_kunde", { kunde_id: id })])
      .then(function (ergebnisse) {
        aktuellerKunde = ergebnisse[0];
        aktuelleAuftraege = ergebnisse[1];
        blattZeichnen();
      })
      .catch(function (e) { elBlatt.innerHTML = '<p class="leer">' + fehlerText(e) + "</p>"; });
  }

  function postenSumme() {
    return posten.reduce(function (s, p) { return s + p.stueck * p.preis; }, 0);
  }

  function kartehinweisHtml(k) {
    if (k.kartensatz) {
      var satzText = String(k.kartensatz).replace(".", ",");
      return '<div class="kartehinweis"><span>&#10003;</span><span>' +
        "Kartensatz " + satzText + " % ist im Preis schon eingerechnet – " +
        "die Kundin zahlt den ausgewiesenen Betrag, sonst nichts.</span></div>";
    }
    return '<div class="kartehinweis neu"><span>&#9888;</span><span>' +
      "Erste Kartenzahlung dieser Kundin – die Gebühr geht diesmal zu unseren Lasten. " +
      "Sobald die Abrechnung kommt, Satz oben eintragen: ab dann rechnet es das Programm automatisch ein.</span></div>";
  }

  function quittungHtml(auftrag, kunde) {
    var zeilen = auftrag.posten
      .map(function (p) {
        return "<tr><td>" + p.stueck + "</td><td>" + escapeHtml(p.bezeichnung) + '</td><td class="re">' +
          chf(p.preis) + '</td><td class="re">' + chf(p.stueck * p.preis) + "</td></tr>";
      })
      .join("");
    return (
      '<div class="quittung" id="druckBereich">' +
      '<div class="quittung-kopf">' +
      "<div><strong>Nähservice Straub</strong>" +
      "<p>Änderungen und Reparaturen · Rosmarie Straub<br>Staldenbachstrasse 13, 8808 Pfäffikon SZ<br>055 410 72 06 · naehservicestraub.ch</p></div>" +
      '<div style="text-align:right"><strong>Quittung ' + auftrag.rechnungsnummer + "</strong>" +
      "<p>" + datumKurz(auftrag.datum) + "<br>" + escapeHtml(kunde.vorname) + " " + escapeHtml(kunde.name) + "<br>" + escapeHtml(kunde.ort) + "</p></div>" +
      "</div>" +
      "<table><tbody>" + zeilen + "</tbody>" +
      '<tfoot><tr><td colspan="3">Total · bezahlt ' + auftrag.zahlart + '</td><td class="re">CHF ' + chf(auftrag.summe) + "</td></tr></tfoot></table>" +
      '<p class="kleingedruckt">Reklamationen innert 10 Tagen nach Abholung</p>' +
      '<p class="kleingedruckt">Kundenexemplar · Kartensatz und Gebühr erscheinen hier nie.</p>' +
      "</div>"
    );
  }

  function blattZeichnen() {
    var k = aktuellerKunde;
    var summe = postenSumme();

    var verlaufZeilen = aktuelleAuftraege
      .map(function (a) {
        var erste = a.posten && a.posten[0] ? a.posten[0].bezeichnung : "";
        var mehr = a.posten && a.posten.length > 1 ? " +" + (a.posten.length - 1) : "";
        return (
          '<tr><td class="zahl" style="white-space:nowrap;color:var(--tinte-2)">' + datumKurz(a.datum) + "</td>" +
          "<td>" + escapeHtml(erste) + escapeHtml(mehr) + '<br><span style="font-size:12px;color:var(--tinte-3)">Nr. ' + a.rechnungsnummer + "</span></td>" +
          '<td class="re"><span class="zahlart' + (a.zahlart === "Rechnung" ? " za-offen" : "") + '">' + a.zahlart + "</span></td>" +
          '<td class="re">' + chf(a.summe) + "</td></tr>"
        );
      })
      .join("");

    var postenZeilen = posten
      .map(function (p, i) {
        return (
          '<div class="posten">' +
          '<input type="number" min="1" value="' + p.stueck + '" data-i="' + i + '" data-f="stueck" aria-label="Stück">' +
          '<input type="text" value="' + escapeHtml(p.bezeichnung) + '" data-i="' + i + '" data-f="bezeichnung" aria-label="Arbeit" placeholder="z. B. Hose kürzen">' +
          '<input type="number" min="0" step="0.05" value="' + p.preis + '" data-i="' + i + '" data-f="preis" aria-label="Preis">' +
          '<span class="summe zahl">' + chf(p.stueck * p.preis) + "</span>" +
          '<button type="button" class="weg" data-weg="' + i + '" aria-label="Zeile entfernen">&times;</button>' +
          "</div>"
        );
      })
      .join("");

    var karteName = { "1.5": "1,5 %", "2.5": "2,5 %", "": "noch nie" };
    var karteKey = k.kartensatz === null || k.kartensatz === undefined ? "" : String(k.kartensatz);
    var ZAHLARTEN = ["Bar", "Twint", "Karte", "Rechnung"];

    elBlatt.innerHTML =
      '<div class="blatt-kopf">' +
      "<div><h2>" + escapeHtml(k.vorname) + " " + escapeHtml(k.name) + ' <span class="kundennr">Nr. ' + k.nummer + "</span></h2>" +
      '<p class="kontakt"><span>' + escapeHtml(k.telefon) + "</span> · <span>" + escapeHtml(k.ort) + "</span>" +
      (k.letzter_besuch ? " · <span>zuletzt " + datumKurz(k.letzter_besuch) + "</span>" : "") + "</p>" +
      '<div class="kartenblock"><span>Kartensatz</span><div class="kartewahl" id="kartewahl">' +
      ["1.5", "2.5", ""]
        .map(function (satz) {
          return '<button type="button" data-satz="' + satz + '" aria-pressed="' + (satz === karteKey) + '">' + karteName[satz] + "</button>";
        })
        .join("") +
      "</div></div></div>" +
      '<div class="kennzahlen">' +
      '<div class="kennzahl"><b class="zahl">' + chf(k.jahresumsatz) + "</b><span>dieses Jahr</span></div>" +
      '<div class="kennzahl"><b class="zahl">' + k.anzahl_auftraege + "</b><span>Aufträge</span></div>" +
      "</div>" +
      "</div>" +
      '<div class="abschnitt"><h3>Neuer Auftrag</h3>' +
      '<div class="posten posten-kopf"><span>Stück</span><span>Arbeit</span><span style="text-align:right">à CHF</span><span style="text-align:right">Total</span><span></span></div>' +
      postenZeilen +
      '<div class="knopfreihe"><button type="button" class="knopf" id="zeilePlus">+ Zeile</button></div>' +
      '<div class="knopfreihe" style="margin-top:15px"><span style="font-size:13px;color:var(--tinte-2);font-weight:600">Bezahlt mit</span></div>' +
      '<div class="zahlwahl" id="zahlwahl">' +
      ZAHLARTEN.map(function (z) { return '<button type="button" data-z="' + z + '" aria-pressed="' + (z === zahlart) + '">' + z + "</button>"; }).join("") +
      "</div>" +
      (zahlart === "Karte" ? kartehinweisHtml(k) : "") +
      '<div class="endsumme"><span>Total</span><b class="zahl">CHF ' + chf(summe) + "</b></div>" +
      '<div class="knopfreihe">' +
      '<button type="button" class="knopf knopf-voll" id="abschliessenKnopf"' + (summe <= 0 ? " disabled" : "") + ">Auftrag abschliessen &amp; Beleg</button>" +
      '<span id="fertigFehler" class="fehler"></span>' +
      "</div>" +
      (letzteQuittung ? quittungHtml(letzteQuittung, k) + '<div class="knopfreihe"><button type="button" class="knopf" id="druckenKnopf">Beleg drucken</button></div>' : "") +
      "</div>" +
      '<div class="abschnitt"><h3>Bisher bei uns</h3>' +
      (verlaufZeilen
        ? '<table class="verlauf"><thead><tr><th>Datum</th><th>Arbeit</th><th class="re">Zahlart</th><th class="re">CHF</th></tr></thead><tbody>' + verlaufZeilen + "</tbody></table>"
        : '<p style="color:var(--tinte-2);font-size:14px;margin:0">Noch keine Aufträge erfasst.</p>') +
      "</div>";

    verdrahten();
  }

  function verdrahten() {
    elBlatt.querySelectorAll(".posten input").forEach(function (inp) {
      inp.addEventListener("input", function () {
        var i = +inp.dataset.i, f = inp.dataset.f;
        posten[i][f] = f === "bezeichnung" ? inp.value : Math.max(0, parseFloat(inp.value) || 0);
        elBlatt.querySelectorAll(".posten .summe").forEach(function (s, j) {
          if (posten[j]) s.textContent = chf(posten[j].stueck * posten[j].preis);
        });
        var e = elBlatt.querySelector(".endsumme b");
        if (e) e.textContent = "CHF " + chf(postenSumme());
        var knopf = document.getElementById("abschliessenKnopf");
        if (knopf) knopf.disabled = postenSumme() <= 0;
      });
    });
    elBlatt.querySelectorAll("[data-weg]").forEach(function (b) {
      b.addEventListener("click", function () {
        posten.splice(+b.dataset.weg, 1);
        if (!posten.length) posten.push(neuePostenzeile());
        blattZeichnen();
      });
    });
    var plus = elBlatt.querySelector("#zeilePlus");
    if (plus) plus.addEventListener("click", function () { posten.push(neuePostenzeile()); blattZeichnen(); });

    elBlatt.querySelectorAll("#zahlwahl button").forEach(function (b) {
      b.addEventListener("click", function () { zahlart = b.dataset.z; blattZeichnen(); });
    });
    elBlatt.querySelectorAll("#kartewahl button").forEach(function (b) {
      b.addEventListener("click", function () {
        var satz = b.dataset.satz;
        var neu = satz === "" ? null : parseFloat(satz);
        invoke("kartensatz_setzen", { kunde_id: aktuellerKunde.id, kartensatz: neu })
          .then(function () { aktuellerKunde.kartensatz = neu; blattZeichnen(); })
          .catch(function (e) { alert(fehlerText(e)); });
      });
    });

    var abschliessen = document.getElementById("abschliessenKnopf");
    if (abschliessen) {
      abschliessen.addEventListener("click", function () {
        var gueltig = posten.filter(function (p) { return p.bezeichnung.trim() && p.stueck > 0; });
        var fehlerEl = document.getElementById("fertigFehler");
        if (!gueltig.length) { fehlerEl.textContent = "Mindestens eine Position mit Bezeichnung nötig."; return; }

        abschliessen.disabled = true;
        invoke("auftrag_anlegen", { eingabe: { kunde_id: aktuellerKunde.id, zahlart: zahlart, posten: gueltig } })
          .then(function (auftrag) {
            letzteQuittung = auftrag;
            posten = [neuePostenzeile()];
            // Kunde + Verlauf neu laden, damit Jahresumsatz/Auftragszahl sofort stimmen.
            return Promise.all([invoke("kunde_holen", { id: aktuellerKunde.id }), invoke("auftraege_von_kunde", { kunde_id: aktuellerKunde.id })]);
          })
          .then(function (ergebnisse) {
            aktuellerKunde = ergebnisse[0];
            aktuelleAuftraege = ergebnisse[1];
            blattZeichnen();
            suchtextSuchen(); // Betrag/Reihenfolge in der Liste links auffrischen
          })
          .catch(function (e) {
            document.getElementById("fertigFehler").textContent = fehlerText(e);
            abschliessen.disabled = false;
          });
      });
    }

    var drucken = document.getElementById("druckenKnopf");
    if (drucken) drucken.addEventListener("click", function () { window.print(); });
  }

  // ================= EXPORT-VORSCHAU =================
  document.getElementById("exportListeBtn").addEventListener("click", function () {
    exportOffen = !exportOffen;
    exportZeichnen();
  });

  function exportZeichnen() {
    var btn = document.getElementById("exportListeBtn");
    var el = document.getElementById("exportVorschau");
    if (!exportOffen) {
      el.hidden = true; el.innerHTML = "";
      btn.textContent = "Liste exportieren";
      return;
    }
    btn.textContent = "Vorschau schliessen";
    var karteName = { "1.5": "1,5 %", "2.5": "2,5 %" };
    var zeilen = kundenCache
      .slice()
      .sort(function (a, b) { return a.nummer - b.nummer; })
      .map(function (k) {
        var karte = k.kartensatz ? karteName[String(k.kartensatz)] : "–";
        return "<tr><td>" + k.nummer + "</td><td>" + escapeHtml(k.name) + " " + escapeHtml(k.vorname) + "</td>" +
          "<td>" + escapeHtml(k.ort) + "</td><td>" + escapeHtml(k.telefon) + "</td><td>" + karte + "</td>" +
          "<td>" + chf(k.jahresumsatz) + "</td></tr>";
      })
      .join("");
    el.hidden = false;
    el.innerHTML =
      '<div class="exporthinweis">Vorschau – die vollständige, herunterladbare Datei liegt nach „Jetzt sichern" ' +
      '(Reiter „Monat &amp; Jahr") im Ordner „Dokumente \\ Atelierbuch Straub \\ Sicherung".</div>' +
      '<div class="tabellenrahmen"><table class="auflistung"><thead><tr>' +
      "<th>Nr.</th><th>Name</th><th>Ort</th><th>Telefon</th><th>Karte</th><th>Jahresumsatz</th>" +
      "</tr></thead><tbody>" + zeilen + "</tbody></table></div>";
  }

  // ================= MONAT & JAHR =================
  function monatLaden() {
    var jahr = new Date().getFullYear();
    document.getElementById("monatTitel").textContent = "Jahr " + jahr;
    invoke("monatsstatistik", { jahr: jahr }).then(function (zeilen) {
      monatZeichnen(zeilen, jahr);
    });
  }

  var MONATSNAMEN = ["Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.", "Dez."];

  function monatZeichnen(zeilen, jahr) {
    // Server liefert nur Monate mit Bewegung - auf 12 volle Monate auffuellen,
    // damit die Tabelle wie in der Skizze immer alle Monate zeigt.
    var proMonat = {};
    zeilen.forEach(function (z) { proMonat[z.monat] = z; });
    var alle = [];
    for (var m = 1; m <= 12; m++) {
      alle.push(proMonat[m] || { monat: m, bar: 0, twint: 0, karte: 0, rechnung: 0, anzahl_kunden: 0 });
    }

    var summe = { total: 0, bar: 0, twint: 0, karte: 0, rechnung: 0, kunden: 0 };
    alle.forEach(function (m) {
      var total = m.bar + m.twint + m.karte + m.rechnung;
      summe.total += total; summe.bar += m.bar; summe.twint += m.twint;
      summe.karte += m.karte; summe.rechnung += m.rechnung; summe.kunden += m.anzahl_kunden;
    });

    document.getElementById("monatsZeilen").innerHTML = alle
      .map(function (m) {
        var total = m.bar + m.twint + m.karte + m.rechnung;
        var leer = total === 0;
        return (
          "<tr" + (leer ? ' class="still"' : "") + "><td>" + MONATSNAMEN[m.monat - 1] + "</td>" +
          "<td>" + (leer ? "—" : chf(total)) + "</td><td>" + (leer ? "—" : chf(m.bar)) + "</td>" +
          "<td>" + (leer ? "—" : chf(m.karte)) + "</td><td>" + (leer ? "—" : chf(m.twint)) + "</td>" +
          "<td>" + (leer ? "—" : chf(m.rechnung)) + "</td><td>" + (leer ? "—" : m.anzahl_kunden) + "</td>" +
          "<td>" + (leer ? "—" : chf(total / m.anzahl_kunden)) + "</td></tr>"
        );
      })
      .join("");

    document.getElementById("monatsFuss").innerHTML =
      "<tr><td>Total</td><td>" + chf(summe.total) + "</td><td>" + chf(summe.bar) + "</td>" +
      "<td>" + chf(summe.karte) + "</td><td>" + chf(summe.twint) + "</td><td>" + chf(summe.rechnung) + "</td>" +
      "<td>" + summe.kunden + "</td><td>" + (summe.kunden ? chf(summe.total / summe.kunden) : "—") + "</td></tr>";

    document.getElementById("kacheln").innerHTML =
      '<div class="kachel"><b>' + chf(summe.total) + "</b><span>Umsatz " + jahr + "</span></div>" +
      '<div class="kachel"><b>' + summe.kunden + "</b><span>Kunden bedient</span></div>" +
      '<div class="kachel"><b>' + (summe.kunden ? chf(summe.total / summe.kunden) : "—") + "</b><span>pro Kunde</span></div>" +
      '<div class="kachel"><b>' + (summe.total ? Math.round(((summe.bar + summe.karte + summe.twint) / summe.total) * 100) : 0) + "%</b><span>sofort bezahlt</span></div>";

    var hoechst = Math.max.apply(null, alle.map(function (m) { return m.bar + m.twint + m.karte + m.rechnung; })) || 1;
    document.getElementById("saeulen").innerHTML = alle
      .map(function (m) {
        var total = m.bar + m.twint + m.karte + m.rechnung;
        var h = total ? Math.max(2, (total / hoechst) * 100) : 2;
        return (
          '<span class="saeule' + (total ? "" : " still") + '">' +
          '<i style="height:' + h + '%" title="' + MONATSNAMEN[m.monat - 1] + ": CHF " + chf(total) + '"></i>' +
          "<small>" + MONATSNAMEN[m.monat - 1].replace(".", "") + "</small></span>"
        );
      })
      .join("");
  }

  document.getElementById("sicherungKnopf").addEventListener("click", function () {
    var echo = document.getElementById("sicherungEcho");
    echo.textContent = "Sichere …";
    invoke("jetzt_sichern")
      .then(function (pfad) { echo.textContent = "Gesichert nach: " + pfad; })
      .catch(function (e) { echo.textContent = fehlerText(e); echo.style.color = "var(--faden)"; });
  });

  // ================= REITER =================
  document.querySelectorAll('[role="tab"]').forEach(function (t) {
    t.addEventListener("click", function () {
      document.querySelectorAll('[role="tab"]').forEach(function (x) {
        var an = x === t;
        x.setAttribute("aria-selected", an);
        document.getElementById(x.getAttribute("aria-controls")).hidden = !an;
      });
      if (t.id === "r-monat") monatLaden();
    });
  });

  // ================= START =================
  function programmStarten() {
    suchtextSuchen();
    elBlatt.innerHTML = '<p class="leer" style="padding:40px">Links einen Kunden wählen oder „+ Neuer Kunde".</p>';
  }
})();
