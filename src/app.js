(function () {
  "use strict";

  // Faengt WIRKLICH jeden Fehler auf der ganzen Seite ab, auch ausserhalb
  // von try/catch-Bloecken - steht ganz am Anfang, noch vor dem ersten
  // Zugriff auf window.__TAURI__, damit selbst ein Fehler dort sichtbar
  // waere statt die Seite stumm nichts tun zu lassen.
  (function () {
    var el = document.getElementById("globaler-fehler");
    var elText = document.getElementById("globaler-fehler-text");
    if (!el || !elText) return; // sollte nie passieren, aber sicher ist sicher

    function zeigen(text) {
      elText.textContent = text;
      el.hidden = false;
    }
    window.addEventListener("error", function (ev) {
      zeigen((ev.message || "Unbekannter Fehler") + (ev.filename ? " (" + ev.filename + ":" + ev.lineno + ")" : ""));
    });
    window.addEventListener("unhandledrejection", function (ev) {
      var grund = ev.reason;
      zeigen(typeof grund === "string" ? grund : (grund && grund.message) || "Unbekannter Fehler (Promise abgelehnt)");
    });
    var schliessen = document.getElementById("globaler-fehler-schliessen");
    if (schliessen) schliessen.addEventListener("click", function () { el.hidden = true; });
  })();

  // Hell/Dunkel von Hand uebersteuern (Reiter "Einstellungen") - ganz am
  // Anfang anwenden, noch vor allem anderen, damit die Seite nicht erst im
  // falschen Farbschema aufblitzt. Reine Geraete-Einstellung, bewusst nicht
  // in der Datenbank (siehe einstellungen.rs auf der Rust-Seite).
  (function () {
    var wert;
    try { wert = localStorage.getItem("darstellung"); } catch (e) { wert = null; }
    if (wert === "hell") document.documentElement.setAttribute("data-theme", "light");
    else if (wert === "dunkel") document.documentElement.setAttribute("data-theme", "dark");
  })();

  var invoke = window.__TAURI__.core.invoke;

  var fr = new Intl.NumberFormat("de-CH", { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  function chf(n) { return fr.format(n || 0); }

  // Geschaeftsangaben + Kartengebuehr-Saetze - Standardwerte decken sich
  // bewusst mit dem, was bisher fix im Code stand, damit vor dem ersten
  // Laden (oder falls das Laden fehlschlaegt) nichts anders aussieht.
  // Wird direkt nach der Anmeldung mit den echten, gespeicherten Werten
  // ueberschrieben (siehe nachAnmeldung).
  var aktuelleEinstellungen = {
    geschaeft_name: "Nähservice Straub",
    geschaeft_zeile2: "Änderungen und Reparaturen · Rosmarie Straub",
    geschaeft_adresse: "Staldenbachstrasse 13, 8808 Pfäffikon SZ",
    geschaeft_telefon: "055 410 72 06",
    geschaeft_web: "naehservicestraub.ch",
    kartensatz_a: 1.5,
    kartensatz_b: 2.5,
    quittung_hinweis1: "Reklamationen innert 10 Tagen nach Abholung",
    quittung_hinweis2: "Kundenexemplar · Kartensatz und Gebühr erscheinen hier nie.",
    lohn_ferienzuschlag_satz: 8.33,
    lohn_ahv_satz: 5.3,
    lohn_alv_satz: 1.1,
  };
  function kartensatzText(wert) {
    return wert === null || wert === undefined || wert === "" ? "" : String(wert).replace(".", ",") + " %";
  }

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

  // ================= LOGIN / ERSTEINRICHTUNG =================
  var aktuellerBenutzer = null;

  var elLoginBuehne = document.getElementById("login-bildschirm");
  var elEinrichtungBuehne = document.getElementById("einrichtung-bildschirm");
  var elProgramm = document.getElementById("programm");
  var elLoginFormular = document.getElementById("login-formular");
  var elLoginFehler = document.getElementById("login-fehler");
  var elEinrichtungFormular = document.getElementById("einrichtung-formular");
  var elEinrichtungFehler = document.getElementById("einrichtung-fehler");

  // Blendet einen ganzen Bildschirm (Login / Ersteinrichtung / Programm)
  // ein oder aus - setzt "hidden" UND "inert" gemeinsam. "inert" ist die
  // doppelte Absicherung: selbst wenn irgendeine CSS-Regel das Verstecken
  // optisch aushebeln wuerde, bleibt ein unsichtbarer Bildschirm trotzdem
  // garantiert unklickbar und wird von der Tab-Taste komplett uebersprungen.
  function bildschirmZeigen(el, zeigen) {
    el.hidden = !zeigen;
    el.inert = !zeigen;
  }

  // Eine Mitarbeiterin sieht nur "Arbeiten" (Kunden suchen/anlegen/
  // importieren, Aufträge buchen) und "Stunden" (nur ihre eigenen) - Umsatz,
  // Auswertung, Sicherung, das Anlegen weiterer Konten und die
  // Stunden-Uebersicht aller Mitarbeiterinnen bleiben Papa/Mama (Rolle
  // "inhaber") vorbehalten. Reine Oberflaechen-Einschraenkung, keine
  // scharfe Zugriffssperre - passt zum ueberschaubaren familiaeren Rahmen
  // hier.
  function rolleAnwenden(rolle) {
    var istMitarbeiterin = rolle === "mitarbeiterin";
    document.getElementById("r-monat").hidden = istMitarbeiterin;
    document.getElementById("r-treuhand").hidden = istMitarbeiterin;
    document.getElementById("stAlleTafel").hidden = istMitarbeiterin;
    // Geschaeftsangaben und Kartengebuehr-Saetze gelten fuer den ganzen
    // Betrieb, nicht fuer eine einzelne Person - nur Papa/Mama aendern die.
    document.getElementById("eiGeschaeftTafel").hidden = istMitarbeiterin;
    document.getElementById("eiKartenTafel").hidden = istMitarbeiterin;
    document.getElementById("eiLohnTafel").hidden = istMitarbeiterin;
    if (istMitarbeiterin) {
      // Falls von einem frueheren Login noch der Monat-Reiter aktiv war.
      document.getElementById("r-start").click();
    }
  }

  function nachAnmeldung(benutzer) {
    aktuellerBenutzer = benutzer;
    bildschirmZeigen(elLoginBuehne, false);
    bildschirmZeigen(elEinrichtungBuehne, false);
    bildschirmZeigen(elProgramm, true);
    document.getElementById("angemeldet-als").textContent = "Angemeldet: " + benutzer.anzeigename;
    rolleAnwenden(benutzer.rolle);
    invoke("einstellungen_lesen")
      .then(function (e) {
        aktuelleEinstellungen = e;
        einstellungenFormularFuellen();
      })
      .catch(function () {}); // Vorbelegte Standardwerte bleiben, Seite funktioniert trotzdem
    programmStarten();
  }

  // Waehrend eine Anfrage laeuft, den Knopf sperren UND den Text sichtbar
  // auf "Lädt ..." umstellen - sonst kann ein Doppelklick dieselbe Anfrage
  // zweimal losschicken, und man sieht von aussen nicht, ob ueberhaupt
  // etwas passiert oder alles wirklich haengt.
  function knopfSperren(knopf, gesperrt) {
    if (!knopf) return;
    knopf.disabled = gesperrt;
    if (gesperrt) {
      knopf.dataset.text = knopf.textContent;
      knopf.textContent = "Lädt …";
    } else if (knopf.dataset.text) {
      knopf.textContent = knopf.dataset.text;
    }
  }

  // Haengt eine Anfrage trotz allem fest (z.B. eine blockierte Datei durch
  // ein noch laufendes altes Programmfenster), soll das nicht unsichtbar
  // ewig weiterlaufen, sondern nach 10 Sekunden eine klare Meldung zeigen.
  function mitZeitgrenze(versprechen, sekunden) {
    return Promise.race([
      versprechen,
      new Promise(function (_, ablehnen) {
        setTimeout(function () {
          ablehnen("Das dauert ungewöhnlich lange. Bitte das Programm ganz schliessen " +
            "(im Taskmanager prüfen, ob es noch läuft) und neu öffnen.");
        }, sekunden * 1000);
      }),
    ]);
  }

  // Bewusst KEIN <form submit> mehr - nur ein ganz normaler Knopf-Klick.
  // Das eingebaute "Formular absenden"-Verhalten des Browsers (inklusive
  // der eingebauten Pflichtfeld-Pruefung) hat sich in genau diesem
  // eingebetteten Programmfenster als unzuverlaessig gezeigt: der Klick
  // kam nicht zuverlaessig als "submit"-Ereignis an. Ein direkter
  // Klick-Listener auf den Knopf selbst umgeht das komplett.
  var elLoginKnopf = document.getElementById("login-knopf");
  var elEinrichtungKnopf = document.getElementById("einrichtung-knopf");

  function loginAbsenden() {
    elLoginFehler.hidden = true;
    var benutzername = document.getElementById("login-benutzername").value.trim();
    var passwort = document.getElementById("login-passwort").value;

    if (!benutzername || !passwort) {
      elLoginFehler.textContent = "Bitte Benutzername und Passwort eingeben.";
      elLoginFehler.hidden = false;
      return;
    }

    knopfSperren(elLoginKnopf, true);
    mitZeitgrenze(invoke("anmelden", { benutzername: benutzername, passwort: passwort }), 10)
      .then(nachAnmeldung)
      .catch(function (e) {
        elLoginFehler.textContent = fehlerText(e);
        elLoginFehler.hidden = false;
        document.getElementById("login-passwort").value = "";
        document.getElementById("login-passwort").focus();
      })
      .finally(function () { knopfSperren(elLoginKnopf, false); });
  }

  function einrichtungAbsenden() {
    elEinrichtungFehler.hidden = true;
    var anzeigename = document.getElementById("ek-anzeigename").value.trim();
    var benutzername = document.getElementById("ek-benutzername").value.trim();
    var passwort = document.getElementById("ek-passwort").value;
    var passwort2 = document.getElementById("ek-passwort2").value;

    if (!anzeigename || !benutzername || !passwort || !passwort2) {
      elEinrichtungFehler.textContent = "Bitte alle Felder ausfüllen.";
      elEinrichtungFehler.hidden = false;
      return;
    }
    if (passwort !== passwort2) {
      elEinrichtungFehler.textContent = "Die beiden Passwörter stimmen nicht überein.";
      elEinrichtungFehler.hidden = false;
      return;
    }
    if (passwort.length < 6) {
      elEinrichtungFehler.textContent = "Mindestens 6 Zeichen.";
      elEinrichtungFehler.hidden = false;
      return;
    }

    knopfSperren(elEinrichtungKnopf, true);
    mitZeitgrenze(
      invoke("ersteinrichtung_abschliessen", { benutzername: benutzername, anzeigename: anzeigename, passwort: passwort }),
      10
    )
      .then(nachAnmeldung)
      .catch(function (e) {
        elEinrichtungFehler.textContent = fehlerText(e);
        elEinrichtungFehler.hidden = false;
      })
      .finally(function () { knopfSperren(elEinrichtungKnopf, false); });
  }

  elLoginKnopf.addEventListener("click", loginAbsenden);
  elEinrichtungKnopf.addEventListener("click", einrichtungAbsenden);

  // Enter-Taste soll weiterhin wie gewohnt funktionieren - jetzt aber
  // ueber einen eigenen, einfachen Tastendruck-Listener statt ueber das
  // Formular-"submit"-Ereignis.
  function enterLoest(auslöser) {
    return function (ev) {
      if (ev.key === "Enter") { ev.preventDefault(); auslöser(); }
    };
  }
  elLoginFormular.querySelectorAll("input").forEach(function (f) {
    f.addEventListener("keydown", enterLoest(loginAbsenden));
  });
  elEinrichtungFormular.querySelectorAll("input").forEach(function (f) {
    f.addEventListener("keydown", enterLoest(einrichtungAbsenden));
  });

  document.getElementById("abmeldenKnopf").addEventListener("click", function () {
    aktuellerBenutzer = null;
    bildschirmZeigen(elProgramm, false);
    bildschirmZeigen(elLoginBuehne, true);
    elLoginFormular.reset();
    document.getElementById("login-benutzername").focus();
  });

  // Beim Start pruefen: noch niemand eingerichtet -> Ersteinrichtung
  // zeigen, sonst ganz normal den Login-Bildschirm.
  invoke("ist_ersteinrichtung")
    .then(function (leer) {
      if (leer) {
        bildschirmZeigen(elEinrichtungBuehne, true);
        document.getElementById("ek-anzeigename").focus();
      } else {
        bildschirmZeigen(elLoginBuehne, true);
        document.getElementById("login-benutzername").focus();
      }
    })
    .catch(function () {
      // Falls die Pruefung selbst scheitert, lieber den normalen Login
      // zeigen als die Seite leer zu lassen.
      bildschirmZeigen(elLoginBuehne, true);
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

  // ================= NEUER KUNDE / KUNDE BEARBEITEN =================
  // Derselbe Dialog fuer beides - "nkBearbeitenId" entscheidet, ob beim
  // Speichern ein neuer Kunde angelegt oder der bestehende aktualisiert
  // wird (siehe kundeBearbeitenOeffnen, von "Bearbeiten" im Kundenblatt
  // aufgerufen - z.B. um ein "Herr"/"Frau" aus dem Telefonlisten-Import
  // im Vorname-Feld zu korrigieren).
  var elDialog = document.getElementById("neuerKundeDialog");
  var nkBearbeitenId = null;
  var nkNachSpeichern = null; // z.B. Reiter Auftraege: neue Kundin direkt uebernehmen

  document.getElementById("neuerKundeKnopf").addEventListener("click", function () {
    nkBearbeitenId = null;
    nkNachSpeichern = null;
    document.getElementById("neuerKundeFormular").reset();
    document.getElementById("nk-titel").textContent = "Neuer Kunde";
    document.getElementById("nk-speichern").textContent = "Anlegen";
    elDialog.showModal();
    document.getElementById("nk-name").focus();
  });

  function kundeBearbeitenOeffnen(k) {
    nkBearbeitenId = k.id;
    nkNachSpeichern = null;
    document.getElementById("nk-titel").textContent = "Kunde bearbeiten";
    document.getElementById("nk-speichern").textContent = "Speichern";
    document.getElementById("nk-name").value = k.name;
    document.getElementById("nk-vorname").value = k.vorname;
    document.getElementById("nk-telefon").value = k.telefon;
    document.getElementById("nk-ort").value = k.ort;
    document.getElementById("nk-adresse").value = k.adresse;
    document.getElementById("nk-email").value = k.email;
    elDialog.showModal();
    document.getElementById("nk-name").focus();
  }

  document.getElementById("nk-abbrechen").addEventListener("click", function () { nkNachSpeichern = null; elDialog.close(); });

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

    var aufruf = nkBearbeitenId
      ? invoke("kunde_aktualisieren", { kunde_id: nkBearbeitenId, eingabe: eingabe })
      : invoke("kunde_anlegen", { eingabe: eingabe });

    aufruf
      .then(function (kunde) {
        elDialog.close();
        if (nkNachSpeichern) { var rueckruf = nkNachSpeichern; nkNachSpeichern = null; rueckruf(kunde); }
        gewaehlteId = kunde.id;
        elSuche.value = "";
        suchtextSuchen();
        kundeLaden(kunde.id);
      })
      .catch(function (e) { alert(fehlerText(e)); });
  });

  // ================= DATEI AUSWAEHLEN (Kunden/Stunden-Import) =================
  // Gemeinsame Hilfsfunktion fuer beide Import-Dialoge: oeffnet den
  // nativen Datei-Dialog (Tauri-Plugin), schickt den gewaehlten Pfad an
  // Rust (liest Excel .xlsx/.xls oder CSV, siehe datei.rs) und setzt das
  // Ergebnis als Tab-getrennten Text ins Einfuege-Feld - ab da laeuft
  // exakt dieselbe Erkennung wie bei einem Copy&Paste aus Excel.
  function dateiFuerImportLesen(zielTextarea, nachErfolg, aufFehler) {
    if (!window.__TAURI__.dialog || !window.__TAURI__.dialog.open) {
      aufFehler("Dateiauswahl ist in dieser Programmversion nicht verfügbar.");
      return;
    }
    window.__TAURI__.dialog
      .open({ multiple: false, filters: [{ name: "Tabellen", extensions: ["xlsx", "xls", "csv"] }] })
      .then(function (pfad) {
        if (!pfad) return; // Dialog abgebrochen
        return invoke("datei_als_tabelle_lesen", { pfad: pfad }).then(function (tabelle) {
          zielTextarea.value = tabelle.map(function (zeile) { return zeile.join("\t"); }).join("\n");
          nachErfolg();
        });
      })
      .catch(function (e) { aufFehler(fehlerText(e)); });
  }

  // ================= KUNDEN IMPORTIEREN =================
  // Nimmt entgegen, was aus Excel/LibreOffice Calc (oder sonst einer
  // Tabellenkalkulation) kopiert und hier eingefuegt wird - normalerweise
  // Tab-getrennt (so liefert ein Copy&Paste aus einer Tabelle), notfalls
  // auch Semikolon- oder Komma-getrennt wie ein CSV-Export. Oder per Knopf
  // "Datei auswählen" direkt aus einer Excel-/CSV-Datei (ohne Copy&Paste).
  var elKiDialog = document.getElementById("kundenImportDialog");
  var elKiText = document.getElementById("ki-text");
  var elKiKopfzeile = document.getElementById("ki-kopfzeile");
  var elKiVorschau = document.getElementById("ki-vorschau");
  var elKiFehler = document.getElementById("ki-fehler");
  var elKiImportierenKnopf = document.getElementById("ki-importieren");
  var kiEintraegeAlle = []; // letztes Parse-Ergebnis, mit Namen und ohne

  // Welche Spaltenueberschrift zu welchem Feld gehoert - deckt die
  // gaengigsten deutschen Varianten ab. "indexOf" statt nur "===", damit
  // auch "Telefon privat" oder "Telefonnummer" erkannt werden. "telefon"
  // bewusst NICHT hier drin - Telefonspalten werden separat erkannt (siehe
  // kiTelefonSpaltenErkennen), weil es davon mehrere geben kann (Mobil,
  // Privat, Geschäft, ...), die zusammengefuehrt werden muessen.
  var KI_FELD_SYNONYME = {
    nummer: ["nummer", "kundennummer", "knr", "kdnr"],
    name: ["name", "nachname", "familienname"],
    vorname: ["vorname"],
    ort: ["ort", "wohnort", "stadt"],
    adresse: ["adresse", "strasse", "straße", "wohnadresse"],
    email: ["email", "mail"],
    notiz: ["notiz", "bemerkung", "anmerkung", "spez", "spezial", "hinweis"],
  };
  var KI_STANDARD_REIHENFOLGE = ["name", "vorname", "telefon", "ort", "adresse", "email"];
  // Reihenfolge = Prioritaet: die erste gefundene, nicht-leere Nummer
  // einer Zeile wird das Telefon-Hauptfeld, alle weiteren vorhandenen
  // landen beschriftet in der Notiz (siehe Stefans Telefonliste: Mobil,
  // Privat, Geschäft, Ausland in eigenen Spalten).
  var KI_TELEFON_PRIORITAET = ["mobil", "handy", "natel", "telefon", "tel", "privat", "festnetz", "geschaeft", "gesch", "business", "ausland", "fax"];
  var kiKopfzeileErkannt = false;

  function kiTextNormalisieren(s) {
    return String(s || "").toLowerCase().replace(/ß/g, "ss").replace(/[^a-z0-9]/g, "").trim();
  }

  function kiZeilenAufteilen(text) {
    return text.split(/\r\n|\r|\n/).filter(function (z) { return z.trim() !== ""; });
  }

  // Erkennt, ob Tab, Semikolon oder Komma getrennt wurde - anhand der
  // ersten Zeile, da genau diese drei beim Einfuegen aus einer
  // Tabellenkalkulation oder einem CSV-Export vorkommen.
  function kiTrennzeichenErkennen(ersteZeile) {
    var kandidaten = ["\t", ";", ","];
    var beste = "\t", bestAnzahl = -1;
    kandidaten.forEach(function (t) {
      var anzahl = ersteZeile.split(t).length - 1;
      if (anzahl > bestAnzahl) { bestAnzahl = anzahl; beste = t; }
    });
    return beste;
  }

  // Einfacher, anfuehrungszeichen-fester Spalten-Zerleger - faengt auch
  // einen CSV-Export ab, bei dem ein Feld selbst das Trennzeichen enthaelt
  // und deshalb in Anfuehrungszeichen steht.
  function kiZeileSpalten(zeile, trenner) {
    var ergebnis = [];
    var feld = "";
    var inAnfuehrung = false;
    for (var i = 0; i < zeile.length; i++) {
      var c = zeile[i];
      if (inAnfuehrung) {
        if (c === '"') {
          if (zeile[i + 1] === '"') { feld += '"'; i++; }
          else { inAnfuehrung = false; }
        } else {
          feld += c;
        }
      } else if (c === '"') {
        inAnfuehrung = true;
      } else if (c === trenner) {
        ergebnis.push(feld); feld = "";
      } else {
        feld += c;
      }
    }
    ergebnis.push(feld);
    return ergebnis;
  }

  // Versucht, jede Spalte der ersten Zeile einem Feld zuzuordnen - in zwei
  // Durchgaengen: zuerst nur exakte Treffer (verhindert z.B. dass "Vorname"
  // - enthaelt "name" als Teilstring - faelschlich dem Feld "name"
  // zugeordnet wird), danach Teilstring-Treffer fuer den Rest (faengt z.B.
  // "Telefonnummer" oder "Telefon privat" ab). Telefon-Spalten (koennen
  // mehrere sein) separat danach erkennen, siehe kiTelefonSpaltenErkennen.
  function kiKopfzeileZuordnen(spalten) {
    var normSpalten = spalten.map(kiTextNormalisieren);
    var zuordnung = {};
    var belegtFeld = {};
    var belegtSpalte = {};

    normSpalten.forEach(function (norm, i) {
      if (!norm) return;
      for (var feld in KI_FELD_SYNONYME) {
        if (belegtFeld[feld]) continue;
        if (KI_FELD_SYNONYME[feld].indexOf(norm) !== -1) {
          zuordnung[i] = feld; belegtFeld[feld] = true; belegtSpalte[i] = true; break;
        }
      }
    });
    normSpalten.forEach(function (norm, i) {
      if (!norm || belegtSpalte[i]) return;
      for (var feld in KI_FELD_SYNONYME) {
        if (belegtFeld[feld]) continue;
        var passt = KI_FELD_SYNONYME[feld].some(function (syn) { return norm.indexOf(syn) !== -1; });
        if (passt) { zuordnung[i] = feld; belegtFeld[feld] = true; belegtSpalte[i] = true; break; }
      }
    });

    var telefonSpalten = kiTelefonSpaltenErkennen(spalten, normSpalten, belegtSpalte);
    return { zuordnung: zuordnung, telefonSpalten: telefonSpalten };
  }

  // Findet ALLE Telefon-aehnlichen Spalten (nicht nur die erste) - noetig,
  // weil z.B. Stefans Telefonliste eigene Spalten fuer Mobil/Privat/
  // Geschäft/Ausland hat. Spalten, die schon einem anderen Feld zugeordnet
  // sind, werden uebersprungen.
  function kiTelefonSpaltenErkennen(spalten, normSpalten, belegtSpalte) {
    var treffer = [];
    normSpalten.forEach(function (norm, i) {
      if (!norm || belegtSpalte[i]) return;
      var prioritaet = KI_TELEFON_PRIORITAET.indexOf(norm);
      if (prioritaet === -1) {
        for (var p = 0; p < KI_TELEFON_PRIORITAET.length; p++) {
          if (norm.indexOf(KI_TELEFON_PRIORITAET[p]) !== -1) { prioritaet = p; break; }
        }
      }
      if (prioritaet !== -1) {
        treffer.push({ index: i, prioritaet: prioritaet, label: String(spalten[i] || "").trim() });
        belegtSpalte[i] = true;
      }
    });
    treffer.sort(function (a, b) { return a.prioritaet - b.prioritaet; });
    return treffer;
  }

  function kiEintraegeBauen(tabelle, zuordnung, telefonSpalten, kopfzeileUeberspringen) {
    var zeilen = kopfzeileUeberspringen ? tabelle.slice(1) : tabelle;
    return zeilen.map(function (spalten) {
      var e = { nummer: "", name: "", vorname: "", telefon: "", ort: "", adresse: "", email: "", notiz: "" };
      spalten.forEach(function (wert, i) {
        var feld = zuordnung[i];
        if (feld) e[feld] = String(wert || "").trim();
      });
      if (telefonSpalten && telefonSpalten.length) {
        var nummernMitLabel = telefonSpalten
          .map(function (t) { return { label: t.label, wert: String(spalten[t.index] || "").trim() }; })
          .filter(function (n) { return n.wert; });
        if (nummernMitLabel.length) {
          e.telefon = nummernMitLabel[0].wert;
          var weitere = nummernMitLabel.slice(1).map(function (n) { return n.label + ": " + n.wert; }).join(", ");
          if (weitere) e.notiz = e.notiz ? e.notiz + " · " + weitere : weitere;
        }
      }
      return e;
    });
  }

  // Zerlegt den eingefuegten Text neu und ordnet die Spalten zu - wird bei
  // jeder Texteingabe aufgerufen (debounced) und wenn die Kopfzeile-Checkbox
  // von Hand umgestellt wird.
  function kiNeuVerarbeiten(kopfzeileCheckboxVonHand) {
    var zeilen = kiZeilenAufteilen(elKiText.value);
    if (!zeilen.length) {
      kiEintraegeAlle = [];
      kiVorschauZeichnen();
      return;
    }

    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    var tabelle = zeilen.map(function (z) { return kiZeileSpalten(z, trenner); });
    var ergebnis = kiKopfzeileZuordnen(tabelle[0]);
    var zuordnung = ergebnis.zuordnung;
    var telefonSpalten = ergebnis.telefonSpalten;
    // Mindestens 2 Treffer verlangen (bei nur einer Spalte reicht 1) -
    // sonst koennte ein einzelner Zufallstreffer (z.B. "Seestrasse"
    // enthaelt "strasse") eine ganz normale erste Datenzeile faelschlich
    // als Kopfzeile einstufen und damit verschlucken.
    var mindestTreffer = tabelle[0].length <= 1 ? 1 : 2;
    kiKopfzeileErkannt = Object.keys(zuordnung).length + telefonSpalten.length >= mindestTreffer;

    if (!kiKopfzeileErkannt) {
      zuordnung = {};
      KI_STANDARD_REIHENFOLGE.forEach(function (feld, i) { zuordnung[i] = feld; });
      telefonSpalten = [];
    }
    if (!kopfzeileCheckboxVonHand) elKiKopfzeile.checked = kiKopfzeileErkannt;

    kiEintraegeAlle = kiEintraegeBauen(tabelle, zuordnung, telefonSpalten, elKiKopfzeile.checked);
    kiVorschauZeichnen();
  }

  function kiVorschauZeichnen() {
    if (!kiEintraegeAlle.length) {
      elKiVorschau.innerHTML = "";
      elKiImportierenKnopf.disabled = true;
      return;
    }

    var mitName = kiEintraegeAlle.filter(function (e) { return e.name; });
    var ohneName = kiEintraegeAlle.length - mitName.length;

    var zeilenHtml = kiEintraegeAlle.slice(0, 50).map(function (e) {
      var klasse = e.name ? "" : ' class="zeile-uebersprungen"';
      return "<tr" + klasse + "><td>" + escapeHtml(e.nummer) + "</td><td>" + escapeHtml(e.name) + "</td><td>" + escapeHtml(e.vorname) + "</td>" +
        "<td>" + escapeHtml(e.telefon) + "</td><td>" + escapeHtml(e.ort) + "</td>" +
        "<td>" + escapeHtml(e.adresse) + "</td><td>" + escapeHtml(e.email) + "</td><td>" + escapeHtml(e.notiz) + "</td></tr>";
    }).join("");

    var hinweisKopf = kiKopfzeileErkannt
      ? "Kopfzeile erkannt – Spalten automatisch zugeordnet."
      : "Keine Kopfzeile erkannt – Reihenfolge Name, Vorname, Telefon, Ort, Adresse, E-Mail angenommen.";
    var mehrHinweis = kiEintraegeAlle.length > 50 ? " (zeigt die ersten 50 von " + kiEintraegeAlle.length + ")" : "";
    var namenHinweis = mitName.length + (mitName.length === 1 ? " Kunde wird importiert" : " Kunden werden importiert");
    if (ohneName) {
      namenHinweis += ", " + ohneName + " Zeile" + (ohneName === 1 ? "" : "n") +
        " ohne Namen wird" + (ohneName === 1 ? "" : "en") + " übersprungen (durchgestrichen)";
    }

    elKiVorschau.innerHTML =
      '<div class="import-zusammenfassung">' + hinweisKopf + "<br>" + namenHinweis + mehrHinweis + "</div>" +
      '<div class="tabellenrahmen"><table class="auflistung"><thead><tr>' +
      "<th>Nr.</th><th>Name</th><th>Vorname</th><th>Telefon</th><th>Ort</th><th>Adresse</th><th>E-Mail</th><th>Notiz</th>" +
      "</tr></thead><tbody>" + zeilenHtml + "</tbody></table></div>";

    elKiImportierenKnopf.disabled = mitName.length === 0;
  }

  document.getElementById("kundenImportKnopf").addEventListener("click", function () {
    elKiText.value = "";
    elKiKopfzeile.checked = false;
    elKiVorschau.innerHTML = "";
    elKiFehler.hidden = true;
    elKiImportierenKnopf.disabled = true;
    kiEintraegeAlle = [];
    elKiDialog.showModal();
    elKiText.focus();
  });
  document.getElementById("ki-abbrechen").addEventListener("click", function () { elKiDialog.close(); });
  elKiText.addEventListener("input", debounce(function () { kiNeuVerarbeiten(false); }, 150));
  elKiKopfzeile.addEventListener("change", function () { kiNeuVerarbeiten(true); });
  document.getElementById("ki-datei").addEventListener("click", function () {
    elKiFehler.hidden = true;
    dateiFuerImportLesen(
      elKiText,
      function () { kiNeuVerarbeiten(false); },
      function (meldung) { elKiFehler.textContent = meldung; elKiFehler.hidden = false; }
    );
  });

  document.getElementById("ki-importieren").addEventListener("click", function () {
    var eintraege = kiEintraegeAlle
      .filter(function (e) { return e.name; })
      .map(function (e) {
        return {
          nummer: e.nummer ? Number(e.nummer) : null,
          name: e.name, vorname: e.vorname, telefon: e.telefon,
          ort: e.ort, adresse: e.adresse, email: e.email, notiz: e.notiz,
        };
      });
    if (!eintraege.length) return;
    elKiFehler.hidden = true;
    knopfSperren(elKiImportierenKnopf, true);
    invoke("kunden_importieren", { eingaben: eintraege })
      .then(function (anzahl) {
        elKiDialog.close();
        elSuche.value = "";
        suchtextSuchen();
        alert(anzahl + (anzahl === 1 ? " Kunde wurde importiert." : " Kunden wurden importiert."));
      })
      .catch(function (e) {
        elKiFehler.textContent = fehlerText(e);
        elKiFehler.hidden = false;
      })
      .finally(function () { knopfSperren(elKiImportierenKnopf, false); });
  });

  // ================= POSTEN-ERFASSUNG (gemeinsam) =================
  // Dieselbe Eingabe fuer "Arbeiten" (Stueck, Bezeichnung, Preis) im
  // Kundenblatt und im Reiter "Auftraege". Das Feld "Arbeit" schlaegt die
  // aktiven Eintraege der Preisliste vor - wird einer ausgewaehlt, kommt
  // sein Preis automatisch mit (danach frei aenderbar).
  var preislisteAktiv = [];

  function preislisteVorschlaegeLaden() {
    return invoke("preisliste_lesen", { inaktive_zeigen: false })
      .then(function (liste) {
        preislisteAktiv = liste;
        document.getElementById("preislisteVorschlaege").innerHTML = liste
          .map(function (e) {
            return '<option value="' + escapeHtml(e.bezeichnung) + '">' +
              escapeHtml((e.kategorie ? e.kategorie + " · " : "") + "CHF " + chf(e.preis)) + "</option>";
          })
          .join("");
      })
      .catch(function () {});
  }

  function preisAusListe(bezeichnung) {
    var gesucht = String(bezeichnung || "").trim().toLowerCase();
    if (!gesucht) return null;
    for (var i = 0; i < preislisteAktiv.length; i++) {
      if (preislisteAktiv[i].bezeichnung.trim().toLowerCase() === gesucht) return preislisteAktiv[i];
    }
    return null;
  }

  function neuePostenzeile() { return { bezeichnung: "", stueck: 1, preis: 0 }; }

  var POSTEN_KOPF_HTML =
    '<div class="posten posten-kopf"><span>Stück</span><span>Arbeit</span><span style="text-align:right">à CHF</span>' +
    '<span style="text-align:right">Total</span><span></span></div>';

  function postenZeilenHtml(liste) {
    return liste
      .map(function (p, i) {
        return (
          '<div class="posten">' +
          '<input type="number" min="1" value="' + p.stueck + '" data-i="' + i + '" data-f="stueck" aria-label="Stück">' +
          '<input type="text" value="' + escapeHtml(p.bezeichnung) + '" data-i="' + i + '" data-f="bezeichnung" aria-label="Arbeit" placeholder="z. B. Hose kürzen" list="preislisteVorschlaege">' +
          '<input type="number" min="0" step="0.05" value="' + p.preis + '" data-i="' + i + '" data-f="preis" aria-label="Preis">' +
          '<span class="summe zahl">' + chf(p.stueck * p.preis) + "</span>" +
          '<button type="button" class="weg" data-weg="' + i + '" aria-label="Zeile entfernen">&times;</button>' +
          "</div>"
        );
      })
      .join("");
  }

  function postenSummeVon(liste) {
    return liste.reduce(function (s, p) { return s + p.stueck * p.preis; }, 0);
  }

  // Eingaben einer gezeichneten Posten-Liste verdrahten. "nachAenderung"
  // laeuft nach jeder Eingabe (Summen auffrischen), "neuZeichnen" wenn
  // eine Zeile entfernt wurde.
  function postenVerdrahten(container, liste, nachAenderung, neuZeichnen) {
    container.querySelectorAll(".posten input").forEach(function (inp) {
      inp.addEventListener("input", function (ev) {
        var i = +inp.dataset.i, f = inp.dataset.f;
        if (f === "bezeichnung") {
          liste[i].bezeichnung = inp.value;
          // Nur bei Auswahl aus der Vorschlagsliste den Preis uebernehmen,
          // nicht beim normalen Tippen - sonst wuerde ein schon von Hand
          // eingetragener Preis ueberschrieben.
          var treffer = !ev.inputType || ev.inputType === "insertReplacementText" ? preisAusListe(inp.value) : null;
          if (treffer) {
            liste[i].preis = treffer.preis;
            var preisFeld = container.querySelector('.posten input[data-i="' + i + '"][data-f="preis"]');
            if (preisFeld) preisFeld.value = treffer.preis;
          }
        } else {
          liste[i][f] = Math.max(0, parseFloat(inp.value) || 0);
        }
        container.querySelectorAll(".posten .summe").forEach(function (s, j) {
          if (liste[j]) s.textContent = chf(liste[j].stueck * liste[j].preis);
        });
        nachAenderung();
      });
    });
    container.querySelectorAll("[data-weg]").forEach(function (b) {
      b.addEventListener("click", function () {
        liste.splice(+b.dataset.weg, 1);
        if (!liste.length) liste.push(neuePostenzeile());
        neuZeichnen();
      });
    });
  }

  var STATUS_LAUFEND = ["Angenommen", "In Arbeit", "Abholbereit"];

  function statusWahlHtml(a) {
    return '<select class="status-wahl" data-status-id="' + a.id + '" aria-label="Status">' +
      STATUS_LAUFEND.map(function (s) {
        return "<option" + (s === a.status ? " selected" : "") + ">" + s + "</option>";
      }).join("") + "</select>";
  }

  function statusWahlVerdrahten(container, danach) {
    container.querySelectorAll("select[data-status-id]").forEach(function (sel) {
      sel.addEventListener("change", function () {
        invoke("auftrag_status_setzen", { auftrag_id: Number(sel.dataset.statusId), status: sel.value })
          .then(danach)
          .catch(function (e) { alert(fehlerText(e)); danach(); });
      });
    });
  }

  function istUeberfaellig(a) {
    return a.status !== "Abgeholt" && a.abholdatum && a.abholdatum < datumHeute();
  }

  function reiterOeffnen(id) { document.getElementById(id).click(); }

  // Springt ins Kundenblatt (Reiter "Arbeiten") - mit "abrechnenId" direkt
  // im Abrechnen-Modus fuer diesen laufenden Auftrag.
  function kundeOeffnen(kundeId, abrechnenId) {
    reiterOeffnen("r-arbeit");
    gewaehlteId = kundeId;
    listeZeichnen(kundenCache);
    kundeLaden(kundeId, abrechnenId);
  }

  // ================= KUNDENBLATT =================
  var aktuellerKunde = null;
  var aktuelleAuftraege = [];
  var posten = [];
  var zahlart = "Bar";
  var letzteQuittung = null; // gebuchter Auftrag, dessen Beleg gerade angezeigt/gedruckt wird
  var abrechnenAuftrag = null; // laufender Auftrag, der gerade abgerechnet wird (sonst: neuer Auftrag)

  function abrechnenStarten(a) {
    abrechnenAuftrag = a;
    posten = a.posten.map(function (p) { return { bezeichnung: p.bezeichnung, stueck: p.stueck, preis: p.preis }; });
    if (!posten.length) posten = [neuePostenzeile()];
    zahlart = "Bar";
    letzteQuittung = null;
    blattZeichnen();
    var editor = document.getElementById("auftragEditor");
    if (editor) editor.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  function kundeLaden(id, abrechnenId) {
    posten = [neuePostenzeile()];
    zahlart = "Bar";
    letzteQuittung = null;
    abrechnenAuftrag = null;
    elBlatt.innerHTML = '<p class="leer" style="padding:40px">Lade…</p>';

    Promise.all([invoke("kunde_holen", { id: id }), invoke("auftraege_von_kunde", { kunde_id: id })])
      .then(function (ergebnisse) {
        aktuellerKunde = ergebnisse[0];
        aktuelleAuftraege = ergebnisse[1];
        var zuAbrechnen = abrechnenId
          ? aktuelleAuftraege.filter(function (a) { return a.id === abrechnenId && a.status !== "Abgeholt"; })[0]
          : null;
        if (zuAbrechnen) abrechnenStarten(zuAbrechnen);
        else blattZeichnen();
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
      "<div><strong>" + escapeHtml(aktuelleEinstellungen.geschaeft_name) + "</strong>" +
      "<p>" + escapeHtml(aktuelleEinstellungen.geschaeft_zeile2) + "<br>" + escapeHtml(aktuelleEinstellungen.geschaeft_adresse) +
      "<br>" + escapeHtml(aktuelleEinstellungen.geschaeft_telefon) + " · " + escapeHtml(aktuelleEinstellungen.geschaeft_web) + "</p></div>" +
      '<div style="text-align:right"><strong>' + (auftrag.bezahlt ? "Quittung " : "Rechnung ") + auftrag.rechnungsnummer + "</strong>" +
      "<p>" + datumKurz(auftrag.datum) + "<br>" + escapeHtml(kunde.vorname) + " " + escapeHtml(kunde.name) + "<br>" + escapeHtml(kunde.ort) + "</p></div>" +
      "</div>" +
      "<table><tbody>" + zeilen + "</tbody>" +
      '<tfoot><tr><td colspan="3">' +
      (auftrag.bezahlt ? "Total · bezahlt " + escapeHtml(auftrag.zahlart) : "Total · zahlbar per Rechnung") +
      '</td><td class="re">CHF ' + chf(auftrag.summe) + "</td></tr></tfoot></table>" +
      '<p class="kleingedruckt">' + escapeHtml(aktuelleEinstellungen.quittung_hinweis1) + "</p>" +
      '<p class="kleingedruckt">' + escapeHtml(aktuelleEinstellungen.quittung_hinweis2) + "</p>" +
      "</div>"
    );
  }

  function blattZeichnen() {
    var k = aktuellerKunde;
    var summe = postenSumme();

    function arbeitText(a) {
      var erste = a.posten && a.posten[0] ? a.posten[0].bezeichnung : "";
      var mehr = a.posten && a.posten.length > 1 ? " +" + (a.posten.length - 1) : "";
      return escapeHtml(erste) + escapeHtml(mehr);
    }

    var laufende = aktuelleAuftraege.filter(function (a) { return a.status !== "Abgeholt"; });
    var laufendZeilen = laufende
      .map(function (a) {
        var abholen = a.abholdatum
          ? '<span class="' + (istUeberfaellig(a) ? "ueberfaellig" : "") + '">abholen ' + datumKurz(a.abholdatum) + "</span>"
          : "kein Abholdatum";
        var inArbeit = abrechnenAuftrag && abrechnenAuftrag.id === a.id;
        return (
          "<tr><td>" + arbeitText(a) + '<br><span style="font-size:12px;color:var(--tinte-3)">Nr. ' + a.rechnungsnummer +
          " · angenommen " + datumKurz(a.angenommen_am || a.datum) + " · " + abholen + "</span></td>" +
          '<td class="re">' + statusWahlHtml(a) + "</td>" +
          '<td class="re">' + chf(a.summe) + "</td>" +
          '<td class="re">' + (inArbeit
            ? '<span style="font-size:12px;color:var(--tinte-2)">wird abgerechnet</span>'
            : '<button type="button" class="knopf knopf-voll knopf-klein" data-abrechnen="' + a.id + '">Abrechnen</button>') +
          "</td></tr>"
        );
      })
      .join("");

    var verlaufZeilen = aktuelleAuftraege
      .filter(function (a) { return a.status === "Abgeholt"; })
      .map(function (a) {
        var offen = !a.bezahlt
          ? ' <span class="zahlart za-offen">offen</span> <button type="button" class="knopf knopf-klein" data-bezahlt="' + a.id + '">bezahlt ✓</button>'
          : "";
        return (
          '<tr><td class="zahl" style="white-space:nowrap;color:var(--tinte-2)">' + datumKurz(a.datum) + "</td>" +
          "<td>" + arbeitText(a) + '<br><span style="font-size:12px;color:var(--tinte-3)">Nr. ' + a.rechnungsnummer + "</span></td>" +
          '<td class="re"><span class="zahlart' + (a.zahlart === "Rechnung" ? " za-offen" : "") + '">' + escapeHtml(a.zahlart) + "</span>" + offen + "</td>" +
          '<td class="re">' + chf(a.summe) + "</td></tr>"
        );
      })
      .join("");

    var postenZeilen = postenZeilenHtml(posten);

    var karteKey = k.kartensatz === null || k.kartensatz === undefined ? "" : String(k.kartensatz);
    var ZAHLARTEN = ["Bar", "Twint", "Karte", "Rechnung"];

    // Normalerweise genau die zwei aktuell eingestellten Saetze zur Wahl -
    // falls der gespeicherte Satz dieser Kundin aber von frueher stammt und
    // nicht mehr zu den aktuellen Saetzen passt (z.B. nach einer Aenderung
    // in den Einstellungen), zusaetzlich als dritte Option zeigen statt ihn
    // unsichtbar verschwinden zu lassen.
    var karteOptionen = [String(aktuelleEinstellungen.kartensatz_a), String(aktuelleEinstellungen.kartensatz_b), ""];
    if (karteKey !== "" && karteOptionen.indexOf(karteKey) === -1) karteOptionen.splice(2, 0, karteKey);

    elBlatt.innerHTML =
      '<div class="blatt-kopf">' +
      "<div><h2>" + escapeHtml(k.vorname) + " " + escapeHtml(k.name) + ' <span class="kundennr">Nr. ' + k.nummer + "</span> " +
      '<button type="button" class="knopf" id="kundeBearbeitenKnopf" style="font-size:12px;padding:3px 10px">Bearbeiten</button>' +
      // Loeschen nur anbieten, wenn es ueberhaupt moeglich ist (kein
      // Auftrag vorhanden) - sonst lehnt es das Programm sowieso ab (siehe
      // kunde_loeschen in geschaeft.rs), damit kein Umsatz verschwindet.
      (k.anzahl_auftraege === 0
        ? ' <button type="button" class="knopf" id="kundeLoeschenKnopf" style="font-size:12px;padding:3px 10px;color:var(--faden)">Löschen</button>'
        : "") +
      "</h2>" +
      '<p class="kontakt"><span>' + escapeHtml(k.telefon) + "</span> · <span>" + escapeHtml(k.ort) + "</span>" +
      (k.letzter_besuch ? " · <span>zuletzt " + datumKurz(k.letzter_besuch) + "</span>" : "") + "</p>" +
      '<div class="kartenblock"><span>Kartensatz</span><div class="kartewahl" id="kartewahl">' +
      karteOptionen
        .map(function (satz) {
          var text = satz === "" ? "noch nie" : kartensatzText(satz);
          return '<button type="button" data-satz="' + satz + '" aria-pressed="' + (satz === karteKey) + '">' + text + "</button>";
        })
        .join("") +
      "</div></div></div>" +
      '<div class="kennzahlen">' +
      '<div class="kennzahl"><b class="zahl">' + chf(k.jahresumsatz) + "</b><span>dieses Jahr</span></div>" +
      '<div class="kennzahl"><b class="zahl">' + k.anzahl_auftraege + "</b><span>Aufträge</span></div>" +
      (k.offen_summe > 0
        ? '<div class="kennzahl"><b class="zahl" style="color:var(--faden)">' + chf(k.offen_summe) + "</b><span>noch offen</span></div>"
        : "") +
      "</div>" +
      "</div>" +
      (laufendZeilen
        ? '<div class="abschnitt"><h3>Laufende Aufträge</h3>' +
          '<table class="verlauf"><thead><tr><th>Arbeit</th><th class="re">Status</th><th class="re">CHF</th><th></th></tr></thead>' +
          "<tbody>" + laufendZeilen + "</tbody></table></div>"
        : "") +
      '<div class="abschnitt" id="auftragEditor"><h3>' +
      (abrechnenAuftrag ? "Auftrag Nr. " + abrechnenAuftrag.rechnungsnummer + " abrechnen" : "Neuer Auftrag") + "</h3>" +
      (abrechnenAuftrag
        ? '<p style="margin:-4px 0 10px;font-size:13px;color:var(--tinte-2)">Arbeiten und Preise bei Bedarf anpassen, Zahlart wählen. ' +
          '<button type="button" class="knopf knopf-klein" id="abrechnenAbbrechen">Abbrechen</button></p>'
        : "") +
      POSTEN_KOPF_HTML +
      postenZeilen +
      '<div class="knopfreihe"><button type="button" class="knopf" id="zeilePlus">+ Zeile</button></div>' +
      '<div class="knopfreihe" style="margin-top:15px"><span style="font-size:13px;color:var(--tinte-2);font-weight:600">Bezahlt mit</span></div>' +
      '<div class="zahlwahl" id="zahlwahl">' +
      ZAHLARTEN.map(function (z) { return '<button type="button" data-z="' + z + '" aria-pressed="' + (z === zahlart) + '">' + z + "</button>"; }).join("") +
      "</div>" +
      (zahlart === "Karte" ? kartehinweisHtml(k) : "") +
      '<div class="endsumme"><span>Total</span><b class="zahl">CHF ' + chf(summe) + "</b></div>" +
      '<div class="knopfreihe">' +
      '<button type="button" class="knopf knopf-voll" id="abschliessenKnopf"' + (summe <= 0 ? " disabled" : "") + ">" +
      (abrechnenAuftrag ? "Abrechnen &amp; Beleg" : "Auftrag abschliessen &amp; Beleg") + "</button>" +
      '<span id="fertigFehler" class="fehler"></span>' +
      "</div>" +
      (letzteQuittung
        ? quittungHtml(letzteQuittung, k) +
          '<div class="knopfreihe"><button type="button" class="knopf" id="druckenKnopf">Beleg drucken</button>' +
          '<span id="druckenFehler" class="fehler" hidden></span></div>'
        : "") +
      "</div>" +
      '<div class="abschnitt"><h3>Bisher bei uns</h3>' +
      (verlaufZeilen
        ? '<table class="verlauf"><thead><tr><th>Datum</th><th>Arbeit</th><th class="re">Zahlart</th><th class="re">CHF</th></tr></thead><tbody>' + verlaufZeilen + "</tbody></table>"
        : '<p style="color:var(--tinte-2);font-size:14px;margin:0">Noch keine Aufträge erfasst.</p>') +
      "</div>";

    verdrahten();
  }

  function verdrahten() {
    var bearbeitenKnopf = elBlatt.querySelector("#kundeBearbeitenKnopf");
    if (bearbeitenKnopf) bearbeitenKnopf.addEventListener("click", function () { kundeBearbeitenOeffnen(aktuellerKunde); });

    var loeschenKnopf = elBlatt.querySelector("#kundeLoeschenKnopf");
    if (loeschenKnopf) {
      loeschenKnopf.addEventListener("click", function () {
        var k = aktuellerKunde;
        if (!confirm("Kunde „" + k.vorname + " " + k.name + "“ (Nr. " + k.nummer + ") wirklich endgültig löschen?")) return;
        invoke("kunde_loeschen", { kunde_id: k.id })
          .then(function () {
            gewaehlteId = null;
            elSuche.value = "";
            suchtextSuchen();
            elBlatt.innerHTML = '<p class="leer" style="padding:40px">Links einen Kunden wählen oder „+ Neuer Kunde".</p>';
          })
          .catch(function (e) { alert(fehlerText(e)); });
      });
    }

    postenVerdrahten(
      elBlatt,
      posten,
      function () {
        var e = elBlatt.querySelector(".endsumme b");
        if (e) e.textContent = "CHF " + chf(postenSumme());
        var knopf = document.getElementById("abschliessenKnopf");
        if (knopf) knopf.disabled = postenSumme() <= 0;
      },
      blattZeichnen
    );

    function kundeNeuLaden() {
      return Promise.all([invoke("kunde_holen", { id: aktuellerKunde.id }), invoke("auftraege_von_kunde", { kunde_id: aktuellerKunde.id })])
        .then(function (ergebnisse) {
          aktuellerKunde = ergebnisse[0];
          aktuelleAuftraege = ergebnisse[1];
          blattZeichnen();
          suchtextSuchen();
        });
    }

    elBlatt.querySelectorAll("[data-abrechnen]").forEach(function (b) {
      b.addEventListener("click", function () {
        var id = Number(b.dataset.abrechnen);
        var a = aktuelleAuftraege.filter(function (x) { return x.id === id; })[0];
        if (a) abrechnenStarten(a);
      });
    });
    var abrechnenAbbrechen = elBlatt.querySelector("#abrechnenAbbrechen");
    if (abrechnenAbbrechen) {
      abrechnenAbbrechen.addEventListener("click", function () {
        abrechnenAuftrag = null;
        posten = [neuePostenzeile()];
        blattZeichnen();
      });
    }
    elBlatt.querySelectorAll("[data-bezahlt]").forEach(function (b) {
      b.addEventListener("click", function () {
        knopfSperren(b, true);
        invoke("auftrag_bezahlt_markieren", { auftrag_id: Number(b.dataset.bezahlt) })
          .then(kundeNeuLaden)
          .catch(function (e) { alert(fehlerText(e)); knopfSperren(b, false); });
      });
    });
    statusWahlVerdrahten(elBlatt, function () {
      invoke("auftraege_von_kunde", { kunde_id: aktuellerKunde.id }).then(function (liste) {
        aktuelleAuftraege = liste;
        // Abrechnen-Modus und bereits eingetippte Posten bleiben erhalten
        if (abrechnenAuftrag) {
          abrechnenAuftrag = liste.filter(function (x) { return x.id === abrechnenAuftrag.id; })[0] || abrechnenAuftrag;
        }
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
        var aufruf = abrechnenAuftrag
          ? invoke("auftrag_abrechnen", { auftrag_id: abrechnenAuftrag.id, zahlart: zahlart, posten: gueltig })
          : invoke("auftrag_anlegen", { eingabe: { kunde_id: aktuellerKunde.id, zahlart: zahlart, posten: gueltig } });
        aufruf
          .then(function (auftrag) {
            letzteQuittung = auftrag;
            abrechnenAuftrag = null;
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

    // Beleg als PDF statt ueber window.print(): der native Windows-
    // Druckdialog fuegt sonst eine Kopf-/Fusszeile (Datum, Seitentitel,
    // "tauri.localhost", Seitenzahl) hinzu, die sich von hier aus nicht
    // abschalten laesst (siehe quittung.rs). Das PDF wird direkt mit dem
    // Standardprogramm des PCs geoeffnet, von dort druckt man ganz normal.
    var drucken = document.getElementById("druckenKnopf");
    if (drucken) {
      drucken.addEventListener("click", function () {
        var fehlerEl = document.getElementById("druckenFehler");
        if (fehlerEl) fehlerEl.hidden = true;
        knopfSperren(drucken, true);
        invoke("quittung_als_pdf_oeffnen", { auftrag: letzteQuittung, kunde: aktuellerKunde })
          .catch(function (e) {
            if (fehlerEl) { fehlerEl.textContent = fehlerText(e); fehlerEl.hidden = false; }
            else alert(fehlerText(e));
          })
          .finally(function () { knopfSperren(drucken, false); });
      });
    }
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
    var zeilen = kundenCache
      .slice()
      .sort(function (a, b) { return a.nummer - b.nummer; })
      .map(function (k) {
        var karte = k.kartensatz ? kartensatzText(k.kartensatz) : "–";
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

  // ================= STUNDEN =================
  // Eigener Reiter, bewusst getrennt von "Arbeiten" (Kunden) - Stefans
  // Wunsch war ausdruecklich, das Erfassen der Stunden nicht mit den
  // Kunden zu vermischen. Rechnet keinen Lohn aus - das passiert weiterhin
  // in Stefans eigenem Excel, hier gibt's nur die rohen Stunden (auch als
  // stunden.csv in der Sicherung, siehe "Jetzt sichern"). Erfassung wie in
  // Stefans bisheriger Excel-Vorlage: pro Tag zwei Zeitbloecke (Vormittag/
  // Nachmittag), die Stunden werden daraus berechnet statt eingetippt.
  var elStDatum = document.getElementById("st-datum");
  var elStVmBeginn = document.getElementById("st-vm-beginn");
  var elStVmEnde = document.getElementById("st-vm-ende");
  var elStNmBeginn = document.getElementById("st-nm-beginn");
  var elStNmEnde = document.getElementById("st-nm-ende");
  var elStNotiz = document.getElementById("st-notiz");
  var elStFehler = document.getElementById("st-fehler");
  var elStEintragenKnopf = document.getElementById("stundenEintragenKnopf");
  var elStMonatTitel = document.getElementById("stMonatTitel");
  var elStEigeneListe = document.getElementById("stEigeneListe");
  var elStAlleTafel = document.getElementById("stAlleTafel");
  var elStAlleListe = document.getElementById("stAlleListe");

  var heuteFuerStunden = new Date();
  var stJahr = heuteFuerStunden.getFullYear();
  var stMonat = heuteFuerStunden.getMonth() + 1; // 1-12, wie auf der Rust-Seite

  function datumHeute() {
    var h = new Date();
    return h.getFullYear() + "-" + String(h.getMonth() + 1).padStart(2, "0") + "-" + String(h.getDate()).padStart(2, "0");
  }
  elStDatum.value = datumHeute();

  // Zeigt einen Eintrag wie "13:30–16:00 (2.5 Std.)" bzw. mit beiden
  // Bloecken "08:30–11:00 + 13:30–16:00 (5.5 Std.)".
  function stZeitAnzeige(e) {
    var teile = [];
    if (e.vm_beginn && e.vm_ende) teile.push(e.vm_beginn + "–" + e.vm_ende);
    if (e.nm_beginn && e.nm_ende) teile.push(e.nm_beginn + "–" + e.nm_ende);
    return teile.join(" + ") + " (" + e.stunden.toLocaleString("de-CH") + " Std.)";
  }

  function stStundenZeile(e, loeschbar) {
    var notiz = e.notiz ? " <span style=\"color:var(--tinte-3)\">– " + escapeHtml(e.notiz) + "</span>" : "";
    var loeschKnopf = loeschbar
      ? '<button type="button" class="weg" data-id="' + e.id + '" title="Eintrag löschen">×</button>'
      : "";
    return "<tr><td>" + datumKurz(e.datum) + "</td><td>" + stZeitAnzeige(e) + notiz + "</td><td>" + loeschKnopf + "</td></tr>";
  }

  function stEigeneZeichnen(eintraege) {
    if (!eintraege.length) {
      elStEigeneListe.innerHTML = '<p class="leer">Noch keine Stunden in diesem Monat erfasst.</p>';
      return;
    }
    var total = eintraege.reduce(function (s, e) { return s + e.stunden; }, 0);
    elStEigeneListe.innerHTML =
      '<div class="tabellenrahmen"><table class="auflistung"><tbody>' +
      eintraege.map(function (e) { return stStundenZeile(e, true); }).join("") +
      "</tbody></table></div>" +
      '<p style="margin:10px 0 0;font-weight:700">Total: ' + total.toLocaleString("de-CH") + " Stunden</p>";

    elStEigeneListe.querySelectorAll("button[data-id]").forEach(function (knopf) {
      knopf.addEventListener("click", function () {
        invoke("stunden_loeschen", { id: Number(knopf.dataset.id), benutzer_id: aktuellerBenutzer.id })
          .then(stMonatLaden)
          .catch(function (e) { alert(fehlerText(e)); });
      });
    });
  }

  function stAlleZeichnen(eintraege) {
    if (!eintraege.length) {
      elStAlleListe.innerHTML = '<p class="leer">Noch keine Stunden in diesem Monat erfasst.</p>';
      return;
    }
    // Server liefert schon nach Anzeigename sortiert - pro Person eine
    // kleine Zwischenueberschrift mit Subtotal, am Ende der Gesamttotal.
    var html = '<div class="tabellenrahmen"><table class="auflistung"><tbody>';
    var aktuellerName = null;
    var subtotal = 0;
    var gesamt = 0;
    eintraege.forEach(function (e, i) {
      if (e.anzeigename !== aktuellerName) {
        if (aktuellerName !== null) {
          html += '<tr><td colspan="2" style="text-align:right;font-weight:700">Total ' + escapeHtml(aktuellerName) + "</td><td style=\"font-weight:700\">" + subtotal.toLocaleString("de-CH") + " Std.</td></tr>";
        }
        aktuellerName = e.anzeigename;
        subtotal = 0;
        html += '<tr><td colspan="3" style="padding-top:14px;font-weight:700;color:var(--gruen-tief)">' + escapeHtml(e.anzeigename) + "</td></tr>";
      }
      subtotal += e.stunden;
      gesamt += e.stunden;
      html += stStundenZeile(e, false);
      if (i === eintraege.length - 1) {
        html += '<tr><td colspan="2" style="text-align:right;font-weight:700">Total ' + escapeHtml(aktuellerName) + "</td><td style=\"font-weight:700\">" + subtotal.toLocaleString("de-CH") + " Std.</td></tr>";
      }
    });
    html += "</tbody></table></div>" +
      '<p style="margin:10px 0 0;font-weight:700">Alle zusammen: ' + gesamt.toLocaleString("de-CH") + " Stunden</p>";
    elStAlleListe.innerHTML = html;
  }

  function stMonatLaden() {
    elStMonatTitel.textContent = MONATSNAMEN[stMonat - 1].replace(".", "") + " " + stJahr;
    invoke("eigene_stunden", { benutzer_id: aktuellerBenutzer.id, jahr: stJahr, monat: stMonat })
      .then(stEigeneZeichnen)
      .catch(function (e) { elStEigeneListe.innerHTML = '<p class="leer">' + fehlerText(e) + "</p>"; });

    if (!elStAlleTafel.hidden) {
      invoke("alle_stunden", { jahr: stJahr, monat: stMonat })
        .then(stAlleZeichnen)
        .catch(function (e) { elStAlleListe.innerHTML = '<p class="leer">' + fehlerText(e) + "</p>"; });
    }
  }

  document.getElementById("stVorKnopf").addEventListener("click", function () {
    stMonat -= 1;
    if (stMonat < 1) { stMonat = 12; stJahr -= 1; }
    stMonatLaden();
  });
  document.getElementById("stNachKnopf").addEventListener("click", function () {
    stMonat += 1;
    if (stMonat > 12) { stMonat = 1; stJahr += 1; }
    stMonatLaden();
  });
  document.getElementById("stZurueckKnopf").addEventListener("click", function () {
    var h = new Date();
    stJahr = h.getFullYear();
    stMonat = h.getMonth() + 1;
    stMonatLaden();
  });

  document.getElementById("treuhandExportKnopf").addEventListener("click", function () {
    var echo = document.getElementById("treuhandExportEcho");
    echo.style.color = "var(--gruen)";
    echo.textContent = "Exportiere …";
    invoke("stunden_fuer_treuhand_exportieren", { jahr: stJahr, monat: stMonat })
      .then(function (pfad) { echo.textContent = "Exportiert nach: " + pfad; })
      .catch(function (e) { echo.textContent = fehlerText(e); echo.style.color = "var(--faden)"; });
  });

  elStEintragenKnopf.addEventListener("click", function () {
    elStFehler.hidden = true;
    var eingabe = {
      datum: elStDatum.value,
      vm_beginn: elStVmBeginn.value,
      vm_ende: elStVmEnde.value,
      nm_beginn: elStNmBeginn.value,
      nm_ende: elStNmEnde.value,
      notiz: elStNotiz.value.trim(),
    };

    if (!eingabe.datum) {
      elStFehler.textContent = "Bitte ein Datum wählen.";
      elStFehler.hidden = false;
      return;
    }
    if (!!eingabe.vm_beginn !== !!eingabe.vm_ende || !!eingabe.nm_beginn !== !!eingabe.nm_ende) {
      elStFehler.textContent = "Bei einem Zeitblock fehlt Beginn oder Ende.";
      elStFehler.hidden = false;
      return;
    }
    if (!eingabe.vm_beginn && !eingabe.nm_beginn) {
      elStFehler.textContent = "Bitte mindestens einen Zeitblock (Vormittag oder Nachmittag) ausfüllen.";
      elStFehler.hidden = false;
      return;
    }

    knopfSperren(elStEintragenKnopf, true);
    invoke("stunden_erfassen", { benutzer_id: aktuellerBenutzer.id, eingabe: eingabe })
      .then(function () {
        elStVmBeginn.value = "";
        elStVmEnde.value = "";
        elStNmBeginn.value = "";
        elStNmEnde.value = "";
        elStNotiz.value = "";
        elStDatum.value = datumHeute();
        // Zur Sicherheit auf den Monat des gerade erfassten Datums springen -
        // sonst sieht man den neuen Eintrag nicht, falls man gerade einen
        // anderen Monat betrachtet hat.
        var teile = eingabe.datum.split("-");
        stJahr = Number(teile[0]);
        stMonat = Number(teile[1]);
        stMonatLaden();
      })
      .catch(function (e) {
        elStFehler.textContent = fehlerText(e);
        elStFehler.hidden = false;
      })
      .finally(function () { knopfSperren(elStEintragenKnopf, false); });
  });

  // ================= STUNDEN IMPORTIEREN =================
  // Fester Spalten-Reihenfolge (keine Kopfzeilen-Erkennung wie beim
  // Kunden-Import): Datum, Vormittag-Beginn, Vormittag-Ende,
  // Nachmittag-Beginn, Nachmittag-Ende, Notiz. Eine automatische Erkennung
  // waere hier riskant, weil "Beginn"/"Ende" in Stefans Excel-Vorlage
  // zweimal vorkommen (einmal pro Zeitblock) und sich nicht eindeutig
  // zuordnen liessen.
  var elStiDialog = document.getElementById("stundenImportDialog");
  var elStiFuerWenZeile = document.getElementById("sti-fuer-wen-zeile");
  var elStiFuerWen = document.getElementById("sti-fuer-wen");
  var elStiText = document.getElementById("sti-text");
  var elStiVorschau = document.getElementById("sti-vorschau");
  var elStiFehler = document.getElementById("sti-fehler");
  var elStiImportierenKnopf = document.getElementById("sti-importieren");
  var stiGueltigeEintraege = [];

  function stiDatumNormalisieren(s) {
    s = String(s || "").trim();
    if (/^\d{4}-\d{2}-\d{2}$/.test(s)) return s;
    var ch = s.match(/^(\d{1,2})\.(\d{1,2})\.(\d{2,4})$/);
    if (ch) {
      var jahr = ch[3].length === 2 ? "20" + ch[3] : ch[3];
      return jahr + "-" + ch[2].padStart(2, "0") + "-" + ch[1].padStart(2, "0");
    }
    return "";
  }

  function stiZeitNormalisieren(s) {
    var m = String(s || "").trim().match(/^(\d{1,2}):(\d{2})/);
    return m ? m[1].padStart(2, "0") + ":" + m[2] : "";
  }

  // Nutzt dieselben Zerlege-Hilfsfunktionen wie "Kunden importieren"
  // (kiZeilenAufteilen/kiTrennzeichenErkennen/kiZeileSpalten) - die
  // Tab/Semikolon/Komma-Erkennung ist dieselbe, egal was eingefuegt wird.
  function stiEintraegeAnalysieren(text) {
    var zeilen = kiZeilenAufteilen(text);
    if (!zeilen.length) return [];
    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    var tabelle = zeilen.map(function (z) { return kiZeileSpalten(z, trenner); });
    return tabelle
      .map(function (spalten) {
        return {
          datum: stiDatumNormalisieren(spalten[0]),
          vm_beginn: stiZeitNormalisieren(spalten[1]),
          vm_ende: stiZeitNormalisieren(spalten[2]),
          nm_beginn: stiZeitNormalisieren(spalten[3]),
          nm_ende: stiZeitNormalisieren(spalten[4]),
          notiz: String((spalten[5] || "")).trim(),
        };
      })
      .filter(function (e) { return e.datum; }); // kein erkennbares Datum -> z.B. die Kopfzeile selbst
  }

  function stiVorschauZeichnen() {
    var eintraege = stiEintraegeAnalysieren(elStiText.value);
    if (!eintraege.length) {
      elStiVorschau.innerHTML = "";
      elStiImportierenKnopf.disabled = true;
      stiGueltigeEintraege = [];
      return;
    }

    stiGueltigeEintraege = eintraege.filter(function (e) {
      return (e.vm_beginn && e.vm_ende) || (e.nm_beginn && e.nm_ende);
    });
    var ungueltig = eintraege.length - stiGueltigeEintraege.length;

    var zeilenHtml = eintraege.slice(0, 50).map(function (e) {
      var hatZeit = (e.vm_beginn && e.vm_ende) || (e.nm_beginn && e.nm_ende);
      var klasse = hatZeit ? "" : ' class="zeile-uebersprungen"';
      var vm = e.vm_beginn && e.vm_ende ? e.vm_beginn + "–" + e.vm_ende : "–";
      var nm = e.nm_beginn && e.nm_ende ? e.nm_beginn + "–" + e.nm_ende : "–";
      return "<tr" + klasse + "><td>" + escapeHtml(e.datum) + "</td><td>" + vm + "</td><td>" + nm + "</td><td>" + escapeHtml(e.notiz) + "</td></tr>";
    }).join("");
    var mehrHinweis = eintraege.length > 50 ? " (zeigt die ersten 50 von " + eintraege.length + ")" : "";

    elStiVorschau.innerHTML =
      '<div class="import-zusammenfassung">' + stiGueltigeEintraege.length + " Einträge werden importiert" +
      (ungueltig ? ", " + ungueltig + " ohne erkennbaren Zeitblock werden übersprungen" : "") + mehrHinweis + "</div>" +
      '<div class="tabellenrahmen"><table class="auflistung"><thead><tr>' +
      "<th>Datum</th><th>Vormittag</th><th>Nachmittag</th><th>Notiz</th>" +
      "</tr></thead><tbody>" + zeilenHtml + "</tbody></table></div>";

    elStiImportierenKnopf.disabled = stiGueltigeEintraege.length === 0;
  }

  document.getElementById("stundenImportKnopf").addEventListener("click", function () {
    elStiText.value = "";
    elStiVorschau.innerHTML = "";
    elStiFehler.hidden = true;
    elStiImportierenKnopf.disabled = true;
    stiGueltigeEintraege = [];

    // Nur Papa/Mama duerfen Stunden fuer eine andere Person nachtragen -
    // eine Mitarbeiterin importiert immer nur fuer sich selbst.
    if (aktuellerBenutzer.rolle === "inhaber") {
      elStiFuerWenZeile.hidden = false;
      invoke("alle_benutzer")
        .then(function (benutzer) {
          elStiFuerWen.innerHTML = benutzer
            .map(function (b) {
              var ausgewaehlt = b.id === aktuellerBenutzer.id ? " selected" : "";
              return '<option value="' + b.id + '"' + ausgewaehlt + ">" + escapeHtml(b.anzeigename) + "</option>";
            })
            .join("");
        })
        .catch(function () { elStiFuerWen.innerHTML = '<option value="' + aktuellerBenutzer.id + '">' + escapeHtml(aktuellerBenutzer.anzeigename) + "</option>"; });
    } else {
      elStiFuerWenZeile.hidden = true;
    }

    elStiDialog.showModal();
    elStiText.focus();
  });
  document.getElementById("sti-abbrechen").addEventListener("click", function () { elStiDialog.close(); });
  elStiText.addEventListener("input", debounce(stiVorschauZeichnen, 150));
  document.getElementById("sti-datei").addEventListener("click", function () {
    elStiFehler.hidden = true;
    dateiFuerImportLesen(
      elStiText,
      stiVorschauZeichnen,
      function (meldung) { elStiFehler.textContent = meldung; elStiFehler.hidden = false; }
    );
  });

  document.getElementById("sti-importieren").addEventListener("click", function () {
    if (!stiGueltigeEintraege.length) return;
    var fuerWenId = aktuellerBenutzer.rolle === "inhaber" && elStiFuerWen.value
      ? Number(elStiFuerWen.value)
      : aktuellerBenutzer.id;

    elStiFehler.hidden = true;
    knopfSperren(elStiImportierenKnopf, true);
    invoke("stunden_importieren", { benutzer_id: fuerWenId, eingaben: stiGueltigeEintraege })
      .then(function (anzahl) {
        elStiDialog.close();
        if (fuerWenId === aktuellerBenutzer.id) stMonatLaden();
        alert(anzahl + (anzahl === 1 ? " Eintrag wurde importiert." : " Einträge wurden importiert."));
      })
      .catch(function (e) {
        elStiFehler.textContent = fehlerText(e);
        elStiFehler.hidden = false;
      })
      .finally(function () { knopfSperren(elStiImportierenKnopf, false); });
  });

  document.getElementById("sicherungKnopf").addEventListener("click", function () {
    var echo = document.getElementById("sicherungEcho");
    echo.textContent = "Sichere …";
    invoke("jetzt_sichern")
      .then(function (pfad) { echo.textContent = "Gesichert nach: " + pfad; })
      .catch(function (e) { echo.textContent = fehlerText(e); echo.style.color = "var(--faden)"; });
  });

  // ================= MITARBEITERIN ANLEGEN / BEARBEITEN =================
  // Bewusst ohne Login-Option: eine Mitarbeiterin bekommt hier nur ein
  // Lohnprofil (fuer die Treuhand-Lohnabrechnung), keinen Zugang zum
  // Programm - Stefan traegt ihre Stunden selbst ein (ueber "Fuer wen?").
  // Derselbe Dialog fuer beides, genau wie bei "Kunde bearbeiten" -
  // "maBearbeitenId" entscheidet, ob neu angelegt oder aktualisiert wird.
  var elMaDialog = document.getElementById("mitarbeiterinDialog");
  var elMaFehler = document.getElementById("ma-fehler");
  var elMaErfolg = document.getElementById("ma-erfolg");
  var elMaKnopf = document.getElementById("ma-anlegen");
  var elMaListe = document.getElementById("maListe");
  var maBearbeitenId = null;

  function maListeLaden() {
    invoke("alle_benutzer").then(function (benutzer) {
      var mitarbeiterinnen = benutzer.filter(function (b) { return b.rolle === "mitarbeiterin"; });
      if (!mitarbeiterinnen.length) { elMaListe.innerHTML = ""; return; }
      elMaListe.innerHTML =
        '<div class="tabellenrahmen" style="margin:12px 0"><table class="auflistung"><tbody>' +
        mitarbeiterinnen
          .map(function (b) {
            var lohn = b.stundenlohn ? chf(b.stundenlohn) + "/Std." : "kein Stundenlohn hinterlegt";
            return "<tr><td>" + escapeHtml(b.anzeigename) + '</td><td style="color:var(--tinte-2)">' + lohn +
              '</td><td><button type="button" class="knopf" data-id="' + b.id + '" style="font-size:12px;padding:3px 10px">Bearbeiten</button></td></tr>';
          })
          .join("") +
        "</tbody></table></div>";
      elMaListe.querySelectorAll("button[data-id]").forEach(function (btn) {
        btn.addEventListener("click", function () {
          var b = mitarbeiterinnen.filter(function (m) { return m.id === Number(btn.dataset.id); })[0];
          if (b) maDialogOeffnen(b);
        });
      });
    });
  }

  function maDialogOeffnen(bestehende) {
    document.getElementById("mitarbeiterinFormular").reset();
    maBearbeitenId = bestehende ? bestehende.id : null;
    document.getElementById("ma-titel").textContent = bestehende ? "Mitarbeiterin bearbeiten" : "Mitarbeiterin anlegen";
    elMaKnopf.textContent = bestehende ? "Speichern" : "Anlegen";
    document.getElementById("ma-anzeigename").value = bestehende ? bestehende.anzeigename : "Mitarbeiterin 1";
    document.getElementById("ma-strasse").value = bestehende ? bestehende.strasse : "";
    document.getElementById("ma-plz-ort").value = bestehende ? bestehende.plz_ort : "";
    document.getElementById("ma-ahv-nummer").value = bestehende ? bestehende.ahv_nummer : "";
    document.getElementById("ma-stundenlohn").value = bestehende && bestehende.stundenlohn ? bestehende.stundenlohn : "";
    elMaFehler.hidden = true;
    elMaErfolg.hidden = true;
    elMaDialog.showModal();
    document.getElementById("ma-anzeigename").focus();
  }

  document.getElementById("mitarbeiterinAnlegenKnopf").addEventListener("click", function () { maDialogOeffnen(null); });
  document.getElementById("ma-abbrechen").addEventListener("click", function () { elMaDialog.close(); maListeLaden(); });

  elMaKnopf.addEventListener("click", function () {
    elMaFehler.hidden = true;
    elMaErfolg.hidden = true;
    var anzeigename = document.getElementById("ma-anzeigename").value.trim();
    var stundenlohnText = document.getElementById("ma-stundenlohn").value.trim();
    var eingabe = {
      anzeigename: anzeigename,
      strasse: document.getElementById("ma-strasse").value.trim(),
      plz_ort: document.getElementById("ma-plz-ort").value.trim(),
      ahv_nummer: document.getElementById("ma-ahv-nummer").value.trim(),
      stundenlohn: stundenlohnText ? Number(stundenlohnText) : null,
    };

    if (!anzeigename) {
      elMaFehler.textContent = "Bitte mindestens den Anzeigenamen ausfüllen.";
      elMaFehler.hidden = false;
      return;
    }

    var aufruf = maBearbeitenId
      ? invoke("mitarbeiterin_profil_aktualisieren", {
          benutzer_id: maBearbeitenId,
          anzeigename: eingabe.anzeigename,
          strasse: eingabe.strasse,
          plz_ort: eingabe.plz_ort,
          ahv_nummer: eingabe.ahv_nummer,
          stundenlohn: eingabe.stundenlohn,
        })
      : invoke("mitarbeiterin_anlegen", { eingabe: eingabe });

    knopfSperren(elMaKnopf, true);
    aufruf
      .then(function () {
        elMaErfolg.textContent = maBearbeitenId
          ? "Gespeichert."
          : "Angelegt - Stunden trägst du für sie unter „Für wen?“ ein.";
        elMaErfolg.hidden = false;
        maListeLaden();
      })
      .catch(function (e) {
        elMaFehler.textContent = fehlerText(e);
        elMaFehler.hidden = false;
      })
      .finally(function () { knopfSperren(elMaKnopf, false); });
  });
  document.getElementById("mitarbeiterinFormular").querySelectorAll("input").forEach(function (f) {
    f.addEventListener("keydown", enterLoest(function () { elMaKnopf.click(); }));
  });

  // ================= TREUHAND =================
  // Geschaeftsausgaben erfassen (feste Kategorie-Liste, siehe treuhand.rs)
  // und daraus den jaehrlichen Einnahmen/Ausgaben-Bericht fuer die Treuhand
  // exportieren - ersetzt Stefans bisherige "Treuhand - Umsatz"-Excel.
  // Dazu die monatliche Lohnabrechnung einer Mitarbeiterin, berechnet aus
  // den schon erfassten Stunden und ihrem hinterlegten Stundenlohn.
  var thJahr = new Date().getFullYear();
  var elThListe = document.getElementById("thListe");
  var elThKategorie = document.getElementById("th-kategorie");
  var elThFehler = document.getElementById("th-fehler");
  var elThBelegName = document.getElementById("th-beleg-name");
  var thKategorienGeladen = false;
  var thBelegQuelle = null; // Pfad der ausgewaehlten Beleg-Datei, bis zum naechsten Erfassen/Zuruecksetzen

  function thJahrLaden() {
    document.getElementById("thJahrTitel").textContent = "Jahr " + thJahr;
    invoke("ausgaben_eines_jahres", { jahr: thJahr })
      .then(thListeZeichnen)
      .catch(function (e) { elThListe.innerHTML = '<tr><td colspan="6" class="leer">' + fehlerText(e) + "</td></tr>"; });
  }

  function thListeZeichnen(ausgaben) {
    if (!ausgaben.length) {
      elThListe.innerHTML = '<tr><td colspan="6" class="leer">Noch keine Ausgaben erfasst.</td></tr>';
      return;
    }
    elThListe.innerHTML = ausgaben
      .map(function (a) {
        var beleg = a.beleg_pfad
          ? '<button type="button" class="knopf" data-beleg="' + escapeHtml(a.beleg_pfad) + '" style="font-size:12px;padding:3px 10px">📎 Beleg</button>'
          : "";
        return "<tr><td>" + datumKurz(a.datum) + "</td><td>" + escapeHtml(a.kategorie) + '</td><td class="re">' +
          chf(a.betrag) + "</td><td>" + escapeHtml(a.notiz) + "</td><td>" + beleg +
          '</td><td><button type="button" class="weg" data-id="' + a.id + '" title="Ausgabe löschen">×</button></td></tr>';
      })
      .join("");
    elThListe.querySelectorAll("button[data-id]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        invoke("ausgabe_loeschen", { id: Number(btn.getAttribute("data-id")) })
          .then(thJahrLaden)
          .catch(function (e) { alert(fehlerText(e)); });
      });
    });
    elThListe.querySelectorAll("button[data-beleg]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        invoke("beleg_oeffnen", { pfad: btn.getAttribute("data-beleg") }).catch(function (e) { alert(fehlerText(e)); });
      });
    });
  }

  function thLaden() {
    thJahrLaden();
    if (!thKategorienGeladen) {
      thKategorienGeladen = true;
      invoke("ausgaben_kategorien").then(function (kategorien) {
        elThKategorie.innerHTML = kategorien.map(function (k) { return "<option>" + escapeHtml(k) + "</option>"; }).join("");
      });
    }
    document.getElementById("th-datum").value = datumHeute();

    invoke("alle_benutzer").then(function (benutzer) {
      var mitarbeiterinnen = benutzer.filter(function (b) { return b.rolle === "mitarbeiterin"; });
      var elPerson = document.getElementById("th-lohn-person");
      elPerson.innerHTML = mitarbeiterinnen.length
        ? mitarbeiterinnen.map(function (b) { return '<option value="' + b.id + '">' + escapeHtml(b.anzeigename) + "</option>"; }).join("")
        : '<option value="">(keine Mitarbeiterin angelegt)</option>';
    });
    var elLohnMonat = document.getElementById("th-lohn-monat");
    if (!elLohnMonat.options.length) {
      elLohnMonat.innerHTML = MONATSNAMEN.map(function (name, i) { return '<option value="' + (i + 1) + '">' + name + "</option>"; }).join("");
      elLohnMonat.value = new Date().getMonth() + 1;
    }
    document.getElementById("th-lohn-jahr").value = thJahr;
  }

  document.getElementById("thVorKnopf").addEventListener("click", function () { thJahr -= 1; thJahrLaden(); });
  document.getElementById("thNachKnopf").addEventListener("click", function () { thJahr += 1; thJahrLaden(); });
  document.getElementById("thZurueckKnopf").addEventListener("click", function () { thJahr = new Date().getFullYear(); thJahrLaden(); });

  // Beleg (Foto/Scan/PDF der Quittung) ueber denselben nativen Datei-
  // Dialog wie beim Kunden-/Stunden-Import auswaehlen - der Pfad wird erst
  // beim Erfassen mitgeschickt, die Datei landet dann serverseitig im
  // Sicherungsordner (siehe treuhand.rs).
  document.getElementById("th-beleg-waehlen").addEventListener("click", function () {
    if (!window.__TAURI__.dialog || !window.__TAURI__.dialog.open) {
      elThFehler.textContent = "Dateiauswahl ist in dieser Programmversion nicht verfügbar.";
      elThFehler.hidden = false;
      return;
    }
    window.__TAURI__.dialog
      .open({ multiple: false, filters: [{ name: "Beleg", extensions: ["jpg", "jpeg", "png", "pdf"] }] })
      .then(function (pfad) {
        if (!pfad) return; // Dialog abgebrochen
        thBelegQuelle = pfad;
        elThBelegName.textContent = pfad.split(/[\\/]/).pop();
      });
  });

  document.getElementById("th-erfassen").addEventListener("click", function () {
    elThFehler.hidden = true;
    var eingabe = {
      datum: document.getElementById("th-datum").value,
      kategorie: elThKategorie.value,
      betrag: Number(document.getElementById("th-betrag").value),
      notiz: document.getElementById("th-notiz").value.trim(),
      beleg_quelle: thBelegQuelle,
    };
    if (!eingabe.datum || !(eingabe.betrag > 0)) {
      elThFehler.textContent = "Bitte Datum und einen Betrag grösser als 0 eingeben.";
      elThFehler.hidden = false;
      return;
    }
    var knopf = document.getElementById("th-erfassen");
    knopfSperren(knopf, true);
    invoke("ausgabe_erfassen", { eingabe: eingabe })
      .then(function () {
        document.getElementById("th-betrag").value = "";
        document.getElementById("th-notiz").value = "";
        thBelegQuelle = null;
        elThBelegName.textContent = "";
        thJahrLaden();
      })
      .catch(function (e) { elThFehler.textContent = fehlerText(e); elThFehler.hidden = false; })
      .finally(function () { knopfSperren(knopf, false); });
  });

  document.getElementById("thBerichtKnopf").addEventListener("click", function () {
    var echo = document.getElementById("thBerichtEcho");
    echo.style.color = "var(--gruen)";
    echo.textContent = "Exportiere …";
    invoke("treuhand_bericht_exportieren", { jahr: thJahr })
      .then(function (pfad) { echo.textContent = "Exportiert nach: " + pfad; })
      .catch(function (e) { echo.textContent = fehlerText(e); echo.style.color = "var(--faden)"; });
  });

  document.getElementById("th-lohn-exportieren").addEventListener("click", function () {
    var elFehler = document.getElementById("th-lohn-fehler");
    elFehler.hidden = true;
    var benutzerId = Number(document.getElementById("th-lohn-person").value);
    if (!benutzerId) {
      elFehler.textContent = "Bitte zuerst eine Mitarbeiterin anlegen.";
      elFehler.hidden = false;
      return;
    }
    var monat = Number(document.getElementById("th-lohn-monat").value);
    var jahr = Number(document.getElementById("th-lohn-jahr").value);
    var echo = document.getElementById("thLohnEcho");
    echo.style.color = "var(--gruen)";
    echo.textContent = "Exportiere …";
    invoke("lohnabrechnung_exportieren", { benutzer_id: benutzerId, jahr: jahr, monat: monat })
      .then(function (pfad) { echo.textContent = "Exportiert nach: " + pfad; })
      .catch(function (e) { echo.textContent = ""; elFehler.textContent = fehlerText(e); elFehler.hidden = false; });
  });

  // ================= AUSGABEN IMPORTIEREN =================
  // Fuer Stefans bestehende Treuhand-Excel: dieselben Zerlege-/Datei-
  // Hilfsfunktionen wie beim Kunden-/Stunden-Import (kiZeilenAufteilen,
  // kiTrennzeichenErkennen, kiZeileSpalten, dateiFuerImportLesen,
  // kiTextNormalisieren, stiDatumNormalisieren), nur mit eigener
  // Spalten-/Kategorie-Erkennung.
  var elThiDialog = document.getElementById("ausgabenImportDialog");
  var elThiText = document.getElementById("thi-text");
  var elThiKopfzeile = document.getElementById("thi-kopfzeile");
  var elThiVorschau = document.getElementById("thi-vorschau");
  var elThiFehler = document.getElementById("thi-fehler");
  var elThiImportierenKnopf = document.getElementById("thi-importieren");
  var thiGueltigeEintraege = [];
  var thiKategorienGeladen = [];
  var thiKopfzeileErkannt = false;

  var THI_FELD_SYNONYME = {
    datum: ["datum", "tag"],
    kategorie: ["kategorie", "art", "grund", "rubrik", "bereich"],
    betrag: ["betrag", "kosten", "chf", "ausgabe", "summe", "preis"],
    notiz: ["notiz", "bemerkung", "anmerkung", "beschreibung", "text"],
  };
  // Hilft bei Kategorien, die inhaltlich passen, aber nicht wortgleich mit
  // der festen Kategorie-Liste sind (z. B. "Lohn" -> "Mitarbeiterin").
  var THI_KATEGORIE_SYNONYME = {
    "Mitarbeiterin": ["lohn", "personal", "angestellte", "mitarbeiter"],
    "Telefon": ["handy", "internet", "mobile", "natel", "swisscom"],
    "Miete / Strom": ["miete", "strom", "nebenkosten", "energie"],
    "Auto": ["benzin", "fahrzeug", "tanken", "garage"],
    "Kleinmaterial / Atelier": ["stoff", "naehmaterial", "garn", "zubehoer"],
    "Einrichten / Investition": ["investition", "maschine", "einrichtung", "moebel"],
    "Reparaturen / Service Arbeitsgeräte": ["reparatur", "service", "wartung"],
    "Büromaterialien": ["buero", "papier", "drucker"],
    "Werbung": ["werbung", "inserat", "marketing"],
  };

  function thiKopfzeileZuordnen(spalten) {
    var zuordnung = {};
    spalten.forEach(function (roh, i) {
      var text = kiTextNormalisieren(roh);
      if (!text) return;
      Object.keys(THI_FELD_SYNONYME).forEach(function (feld) {
        if (zuordnung[feld] !== undefined) return;
        if (THI_FELD_SYNONYME[feld].some(function (s) { return text.indexOf(s) !== -1; })) {
          zuordnung[feld] = i;
        }
      });
    });
    return zuordnung;
  }

  // Findet die passende Kategorie aus der festen Liste (ausgaben_kategorien) -
  // zuerst wortgleich, dann als Teilstring in beide Richtungen, zuletzt ueber
  // die Synonym-Liste oben. Kein eindeutiger Treffer -> null (Zeile wird in
  // der Vorschau als unbekannt markiert und beim Import uebersprungen).
  function thiKategoriePassend(text) {
    var norm = kiTextNormalisieren(text);
    if (!norm) return null;
    var treffer = thiKategorienGeladen.filter(function (k) { return kiTextNormalisieren(k) === norm; });
    if (treffer.length === 1) return treffer[0];
    treffer = thiKategorienGeladen.filter(function (k) {
      var kn = kiTextNormalisieren(k);
      return kn.indexOf(norm) !== -1 || norm.indexOf(kn) !== -1;
    });
    if (treffer.length === 1) return treffer[0];
    treffer = thiKategorienGeladen.filter(function (k) {
      var synonyme = THI_KATEGORIE_SYNONYME[k];
      return synonyme && synonyme.some(function (s) { return norm.indexOf(s) !== -1; });
    });
    return treffer.length === 1 ? treffer[0] : null;
  }

  // Versteht sowohl "1234.50" als auch Schweizer/Excel-Schreibweisen wie
  // "1'234.50" oder "1234,50" (Komma als Dezimaltrennzeichen).
  function thiBetragNormalisieren(s) {
    // "Fr. 25.00" / "25.-" / "25.–": Waehrungszeichen und Strich am Ende weg
    s = String(s || "").trim().replace(/[^0-9.,\-]/g, "").replace(/^[.,]+/, "").replace(/[.,]?-+$/, "");
    if (!s) return NaN;
    var hatKomma = s.indexOf(",") !== -1, hatPunkt = s.indexOf(".") !== -1;
    if (hatKomma && hatPunkt) {
      s = s.lastIndexOf(",") > s.lastIndexOf(".") ? s.replace(/\./g, "").replace(",", ".") : s.replace(/,/g, "");
    } else if (hatKomma) {
      s = s.replace(",", ".");
    }
    var n = Number(s);
    return isFinite(n) ? n : NaN;
  }

  function thiEintraegeBauen(tabelle, zuordnung, kopfzeileUeberspringen) {
    var zeilen = kopfzeileUeberspringen ? tabelle.slice(1) : tabelle;
    var hatZuordnung = Object.keys(zuordnung).length > 0;
    var di = hatZuordnung ? zuordnung.datum : 0;
    var ki = hatZuordnung ? zuordnung.kategorie : 1;
    var bi = hatZuordnung ? zuordnung.betrag : 2;
    var ni = hatZuordnung ? zuordnung.notiz : 3;
    return zeilen.map(function (spalten) {
      var kategorieRoh = ki !== undefined ? spalten[ki] : "";
      return {
        datum: stiDatumNormalisieren(di !== undefined ? spalten[di] : ""),
        kategorie_roh: String(kategorieRoh || "").trim(),
        kategorie: thiKategoriePassend(kategorieRoh),
        betrag: thiBetragNormalisieren(bi !== undefined ? spalten[bi] : ""),
        notiz: String((ni !== undefined ? spalten[ni] : "") || "").trim(),
      };
    });
  }

  function thiVorschauZeichnen(kopfzeileCheckboxVonHand) {
    var zeilen = kiZeilenAufteilen(elThiText.value);
    if (!zeilen.length) {
      elThiVorschau.innerHTML = "";
      elThiImportierenKnopf.disabled = true;
      thiGueltigeEintraege = [];
      return;
    }
    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    var tabelle = zeilen.map(function (z) { return kiZeileSpalten(z, trenner); });
    var zuordnung = thiKopfzeileZuordnen(tabelle[0]);
    // Mindestens Datum, Kategorie und Betrag erkannt -> das ist eine Kopfzeile.
    thiKopfzeileErkannt = ["datum", "kategorie", "betrag"].filter(function (f) { return zuordnung[f] !== undefined; }).length >= 2;
    // Nur automatisch setzen, wenn nicht gerade von Hand umgeschaltet wurde -
    // sonst wuerde jeder Tastendruck die manuelle Wahl sofort ueberschreiben.
    if (!kopfzeileCheckboxVonHand) elThiKopfzeile.checked = thiKopfzeileErkannt;

    var eintraege = thiEintraegeBauen(tabelle, zuordnung, elThiKopfzeile.checked);
    thiGueltigeEintraege = eintraege.filter(function (e) { return e.datum && e.kategorie && e.betrag > 0; });
    var ungueltig = eintraege.length - thiGueltigeEintraege.length;

    var zeilenHtml = eintraege.slice(0, 50).map(function (e) {
      var gueltig = e.datum && e.kategorie && e.betrag > 0;
      var klasse = gueltig ? "" : ' class="zeile-uebersprungen"';
      var kategorieAnzeige = e.kategorie || (e.kategorie_roh ? "? " + escapeHtml(e.kategorie_roh) : "–");
      return "<tr" + klasse + "><td>" + escapeHtml(e.datum || "–") + "</td><td>" + escapeHtml(kategorieAnzeige) + '</td><td class="re">' +
        (e.betrag > 0 ? chf(e.betrag) : "–") + "</td><td>" + escapeHtml(e.notiz) + "</td></tr>";
    }).join("");
    var mehrHinweis = eintraege.length > 50 ? " (zeigt die ersten 50 von " + eintraege.length + ")" : "";
    var hinweisKopf = thiKopfzeileErkannt
      ? "Kopfzeile erkannt – Spalten automatisch zugeordnet."
      : "Keine Kopfzeile erkannt – Reihenfolge Datum, Kategorie, Betrag, Notiz angenommen.";

    elThiVorschau.innerHTML =
      '<div class="import-zusammenfassung">' + hinweisKopf + " " + thiGueltigeEintraege.length + " Einträge werden importiert" +
      (ungueltig ? ", " + ungueltig + " mit fehlendem Datum/Betrag oder unbekannter Kategorie werden übersprungen (mit „?“ markiert)" : "") +
      mehrHinweis + "</div>" +
      '<div class="tabellenrahmen"><table class="auflistung"><thead><tr>' +
      "<th>Datum</th><th>Kategorie</th><th class=\"re\">Betrag</th><th>Notiz</th>" +
      "</tr></thead><tbody>" + zeilenHtml + "</tbody></table></div>";

    elThiImportierenKnopf.disabled = thiGueltigeEintraege.length === 0;
  }

  document.getElementById("thImportKnopf").addEventListener("click", function () {
    elThiText.value = "";
    elThiVorschau.innerHTML = "";
    elThiFehler.hidden = true;
    elThiKopfzeile.checked = false;
    elThiImportierenKnopf.disabled = true;
    thiGueltigeEintraege = [];
    var weiter = function () { elThiDialog.showModal(); elThiText.focus(); };
    if (thiKategorienGeladen.length) {
      weiter();
    } else {
      invoke("ausgaben_kategorien").then(function (k) { thiKategorienGeladen = k; weiter(); }).catch(weiter);
    }
  });
  document.getElementById("thi-abbrechen").addEventListener("click", function () { elThiDialog.close(); });
  elThiText.addEventListener("input", debounce(function () { thiVorschauZeichnen(false); }, 150));
  elThiKopfzeile.addEventListener("change", function () { thiVorschauZeichnen(true); });
  document.getElementById("thi-datei").addEventListener("click", function () {
    elThiFehler.hidden = true;
    dateiFuerImportLesen(
      elThiText,
      thiVorschauZeichnen,
      function (meldung) { elThiFehler.textContent = meldung; elThiFehler.hidden = false; }
    );
  });

  document.getElementById("thi-importieren").addEventListener("click", function () {
    if (!thiGueltigeEintraege.length) return;
    var eingaben = thiGueltigeEintraege.map(function (e) {
      return { datum: e.datum, kategorie: e.kategorie, betrag: e.betrag, notiz: e.notiz, beleg_quelle: null };
    });
    elThiFehler.hidden = true;
    knopfSperren(elThiImportierenKnopf, true);
    invoke("ausgaben_importieren", { eingaben: eingaben })
      .then(function (anzahl) {
        elThiDialog.close();
        thJahrLaden();
        alert(anzahl + (anzahl === 1 ? " Ausgabe wurde importiert." : " Ausgaben wurden importiert."));
      })
      .catch(function (e) { elThiFehler.textContent = fehlerText(e); elThiFehler.hidden = false; })
      .finally(function () { knopfSperren(elThiImportierenKnopf, false); });
  });

  // ================= EINSTELLUNGEN =================
  function einstellungenFormularFuellen() {
    document.getElementById("ei-geschaeft-name").value = aktuelleEinstellungen.geschaeft_name;
    document.getElementById("ei-geschaeft-zeile2").value = aktuelleEinstellungen.geschaeft_zeile2;
    document.getElementById("ei-geschaeft-adresse").value = aktuelleEinstellungen.geschaeft_adresse;
    document.getElementById("ei-geschaeft-telefon").value = aktuelleEinstellungen.geschaeft_telefon;
    document.getElementById("ei-geschaeft-web").value = aktuelleEinstellungen.geschaeft_web;
    document.getElementById("ei-quittung-hinweis1").value = aktuelleEinstellungen.quittung_hinweis1;
    document.getElementById("ei-quittung-hinweis2").value = aktuelleEinstellungen.quittung_hinweis2;
    einstellungenLogoAnzeigen();
    document.getElementById("ei-kartensatz-a").value = aktuelleEinstellungen.kartensatz_a;
    document.getElementById("ei-kartensatz-b").value = aktuelleEinstellungen.kartensatz_b;
    document.getElementById("ei-lohn-ferienzuschlag").value = aktuelleEinstellungen.lohn_ferienzuschlag_satz;
    document.getElementById("ei-lohn-ahv").value = aktuelleEinstellungen.lohn_ahv_satz;
    document.getElementById("ei-lohn-alv").value = aktuelleEinstellungen.lohn_alv_satz;
  }

  // --- Quittungs-Logo (eigenes Foto/Logo oben auf der Quittung) ---
  function einstellungenLogoAnzeigen() {
    var vorhanden = !!aktuelleEinstellungen.quittung_logo_pfad;
    document.getElementById("ei-logo-entfernen").hidden = !vorhanden;
    document.getElementById("ei-logo-name").textContent = vorhanden
      ? "Hinterlegt: " + aktuelleEinstellungen.quittung_logo_pfad.split(/[\\/]/).pop()
      : "Kein Logo hinterlegt.";
  }

  document.getElementById("ei-logo-waehlen").addEventListener("click", function () {
    var elFehler = document.getElementById("ei-geschaeft-fehler");
    elFehler.hidden = true;
    if (!window.__TAURI__.dialog || !window.__TAURI__.dialog.open) {
      elFehler.textContent = "Dateiauswahl ist in dieser Programmversion nicht verfügbar.";
      elFehler.hidden = false;
      return;
    }
    window.__TAURI__.dialog
      .open({ multiple: false, filters: [{ name: "Logo/Foto", extensions: ["jpg", "jpeg", "png"] }] })
      .then(function (pfad) {
        if (!pfad) return; // Dialog abgebrochen
        return invoke("quittung_logo_setzen", { quelle: pfad }).then(function (zielPfad) {
          aktuelleEinstellungen.quittung_logo_pfad = zielPfad;
          einstellungenLogoAnzeigen();
        });
      })
      .catch(function (e) { elFehler.textContent = fehlerText(e); elFehler.hidden = false; });
  });

  document.getElementById("ei-logo-entfernen").addEventListener("click", function () {
    invoke("quittung_logo_entfernen").then(function () {
      aktuelleEinstellungen.quittung_logo_pfad = null;
      einstellungenLogoAnzeigen();
    });
  });

  // --- Mein Konto: eigenes Passwort ---
  var elEiPasswort = document.getElementById("ei-passwort");
  var elEiPasswort2 = document.getElementById("ei-passwort2");
  var elEiPasswortFehler = document.getElementById("ei-passwort-fehler");
  var elEiPasswortErfolg = document.getElementById("ei-passwort-erfolg");
  var elEiPasswortKnopf = document.getElementById("eiPasswortKnopf");

  elEiPasswortKnopf.addEventListener("click", function () {
    elEiPasswortFehler.hidden = true;
    elEiPasswortErfolg.hidden = true;
    var p1 = elEiPasswort.value;
    var p2 = elEiPasswort2.value;
    if (!p1 || !p2) {
      elEiPasswortFehler.textContent = "Bitte beide Felder ausfüllen.";
      elEiPasswortFehler.hidden = false;
      return;
    }
    if (p1 !== p2) {
      elEiPasswortFehler.textContent = "Die beiden Passwörter stimmen nicht überein.";
      elEiPasswortFehler.hidden = false;
      return;
    }
    if (p1.length < 6) {
      elEiPasswortFehler.textContent = "Mindestens 6 Zeichen.";
      elEiPasswortFehler.hidden = false;
      return;
    }
    knopfSperren(elEiPasswortKnopf, true);
    invoke("passwort_aendern", { benutzer_id: aktuellerBenutzer.id, neues_passwort: p1 })
      .then(function () {
        elEiPasswortErfolg.textContent = "Passwort geändert.";
        elEiPasswortErfolg.hidden = false;
        elEiPasswort.value = "";
        elEiPasswort2.value = "";
      })
      .catch(function (e) {
        elEiPasswortFehler.textContent = fehlerText(e);
        elEiPasswortFehler.hidden = false;
      })
      .finally(function () { knopfSperren(elEiPasswortKnopf, false); });
  });

  // --- Darstellung: Hell/Dunkel/Automatisch ---
  var elEiDarstellung = document.getElementById("eiDarstellung");
  function darstellungAnwenden(wert) {
    if (wert === "hell") document.documentElement.setAttribute("data-theme", "light");
    else if (wert === "dunkel") document.documentElement.setAttribute("data-theme", "dark");
    else document.documentElement.removeAttribute("data-theme");
    elEiDarstellung.querySelectorAll("button").forEach(function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.wert === wert));
    });
  }
  (function () {
    var gespeichert;
    try { gespeichert = localStorage.getItem("darstellung"); } catch (e) { gespeichert = null; }
    darstellungAnwenden(gespeichert || "automatisch");
  })();
  elEiDarstellung.querySelectorAll("button").forEach(function (b) {
    b.addEventListener("click", function () {
      var wert = b.dataset.wert;
      try { localStorage.setItem("darstellung", wert); } catch (e) {}
      darstellungAnwenden(wert);
    });
  });

  // --- Geschäftsangaben (nur Papa/Mama sichtbar, siehe rolleAnwenden) ---
  document.getElementById("eiGeschaeftKnopf").addEventListener("click", function () {
    var elFehler = document.getElementById("ei-geschaeft-fehler");
    var elErfolg = document.getElementById("ei-geschaeft-erfolg");
    elFehler.hidden = true;
    elErfolg.hidden = true;
    var name = document.getElementById("ei-geschaeft-name").value.trim();
    if (!name) {
      elFehler.textContent = "Der Name darf nicht leer sein.";
      elFehler.hidden = false;
      return;
    }
    var neu = Object.assign({}, aktuelleEinstellungen, {
      geschaeft_name: name,
      geschaeft_zeile2: document.getElementById("ei-geschaeft-zeile2").value.trim(),
      geschaeft_adresse: document.getElementById("ei-geschaeft-adresse").value.trim(),
      geschaeft_telefon: document.getElementById("ei-geschaeft-telefon").value.trim(),
      geschaeft_web: document.getElementById("ei-geschaeft-web").value.trim(),
      quittung_hinweis1: document.getElementById("ei-quittung-hinweis1").value.trim(),
      quittung_hinweis2: document.getElementById("ei-quittung-hinweis2").value.trim(),
    });
    var knopf = document.getElementById("eiGeschaeftKnopf");
    knopfSperren(knopf, true);
    invoke("einstellungen_speichern", { eingabe: neu })
      .then(function () {
        aktuelleEinstellungen = neu;
        elErfolg.textContent = "Gespeichert.";
        elErfolg.hidden = false;
      })
      .catch(function (e) { elFehler.textContent = fehlerText(e); elFehler.hidden = false; })
      .finally(function () { knopfSperren(knopf, false); });
  });

  // --- Kartengebühr-Sätze (nur Papa/Mama sichtbar) ---
  document.getElementById("eiKartenKnopf").addEventListener("click", function () {
    var elFehler = document.getElementById("ei-karten-fehler");
    var elErfolg = document.getElementById("ei-karten-erfolg");
    elFehler.hidden = true;
    elErfolg.hidden = true;
    var a = parseFloat(document.getElementById("ei-kartensatz-a").value);
    var b = parseFloat(document.getElementById("ei-kartensatz-b").value);
    if (!(a >= 0 && a <= 100) || !(b >= 0 && b <= 100)) {
      elFehler.textContent = "Bitte beide Sätze zwischen 0 und 100 eingeben.";
      elFehler.hidden = false;
      return;
    }
    var neu = Object.assign({}, aktuelleEinstellungen, { kartensatz_a: a, kartensatz_b: b });
    var knopf = document.getElementById("eiKartenKnopf");
    knopfSperren(knopf, true);
    invoke("einstellungen_speichern", { eingabe: neu })
      .then(function () {
        aktuelleEinstellungen = neu;
        elErfolg.textContent = "Gespeichert.";
        elErfolg.hidden = false;
      })
      .catch(function (e) { elFehler.textContent = fehlerText(e); elFehler.hidden = false; })
      .finally(function () { knopfSperren(knopf, false); });
  });

  // --- Lohn-Prozentsätze (nur Papa/Mama sichtbar) ---
  document.getElementById("eiLohnKnopf").addEventListener("click", function () {
    var elFehler = document.getElementById("ei-lohn-fehler");
    var elErfolg = document.getElementById("ei-lohn-erfolg");
    elFehler.hidden = true;
    elErfolg.hidden = true;
    var ferienzuschlag = parseFloat(document.getElementById("ei-lohn-ferienzuschlag").value);
    var ahv = parseFloat(document.getElementById("ei-lohn-ahv").value);
    var alv = parseFloat(document.getElementById("ei-lohn-alv").value);
    if (![ferienzuschlag, ahv, alv].every(function (s) { return s >= 0 && s <= 100; })) {
      elFehler.textContent = "Bitte alle drei Sätze zwischen 0 und 100 eingeben.";
      elFehler.hidden = false;
      return;
    }
    var neu = Object.assign({}, aktuelleEinstellungen, {
      lohn_ferienzuschlag_satz: ferienzuschlag,
      lohn_ahv_satz: ahv,
      lohn_alv_satz: alv,
    });
    var knopf = document.getElementById("eiLohnKnopf");
    knopfSperren(knopf, true);
    invoke("einstellungen_speichern", { eingabe: neu })
      .then(function () {
        aktuelleEinstellungen = neu;
        elErfolg.textContent = "Gespeichert.";
        elErfolg.hidden = false;
      })
      .catch(function (e) { elFehler.textContent = fehlerText(e); elFehler.hidden = false; })
      .finally(function () { knopfSperren(knopf, false); });
  });

  // --- Import & Export: alles an einem Ort ---
  // Oeffnet dieselben Dialoge wie die Knoepfe auf den anderen Reitern -
  // keine zweite Kopie der Erfassungs-/Import-Logik noetig.
  document.getElementById("eiKundenImportKnopf").addEventListener("click", function () {
    document.getElementById("kundenImportKnopf").click();
  });
  document.getElementById("eiStundenImportKnopf").addEventListener("click", function () {
    document.getElementById("stundenImportKnopf").click();
  });
  document.getElementById("eiSicherungKnopf").addEventListener("click", function () {
    var echo = document.getElementById("eiSicherungEcho");
    echo.style.color = "var(--gruen)";
    echo.textContent = "Sichere …";
    invoke("jetzt_sichern")
      .then(function (pfad) { echo.textContent = "Gesichert nach: " + pfad; })
      .catch(function (e) { echo.textContent = fehlerText(e); echo.style.color = "var(--faden)"; });
  });

  // ================= UEBERSICHT (Startseite) =================
  var MONATSNAMEN_LANG = ["Januar", "Februar", "März", "April", "Mai", "Juni", "Juli", "August", "September", "Oktober", "November", "Dezember"];
  var WOCHENTAGE = ["Sonntag", "Montag", "Dienstag", "Mittwoch", "Donnerstag", "Freitag", "Samstag"];

  function kurzlisteHtml(zeilen, leerText) {
    if (!zeilen.length) return '<p style="margin:0;font-size:14px;color:var(--tinte-2)">' + leerText + "</p>";
    return '<table class="verlauf kurzliste"><tbody>' + zeilen.map(function (z) {
      return '<tr class="klickzeile" data-kunde="' + z.kunde_id + '">' +
        '<td style="white-space:nowrap">Nr. ' + z.rechnungsnummer + "</td>" +
        "<td>" + escapeHtml(z.kunde_name) + ' <span style="color:var(--tinte-3);font-size:12px">Kd. ' + z.kunde_nummer + "</span><br>" +
        '<span style="font-size:12.5px;color:var(--tinte-2)">' + escapeHtml(z.arbeit) + "</span></td>" +
        '<td class="re"><span class="' + (istUeberfaellig(z) ? "ueberfaellig" : "") + '">' +
        (z.abholdatum ? datumKurz(z.abholdatum) : "") + "</span><br>" +
        '<span class="zahlart' + (z.status === "Abholbereit" ? "" : " za-offen") + '">' + escapeHtml(z.status) + "</span></td>" +
        '<td class="re">' + chf(z.summe) + "</td></tr>";
    }).join("") + "</tbody></table>";
  }

  function uebersichtLaden() {
    var h = new Date();
    var stunde = h.getHours();
    document.getElementById("startTitel").textContent =
      (stunde < 11 ? "Guten Morgen" : stunde < 17 ? "Guten Tag" : "Guten Abend") +
      (aktuellerBenutzer ? ", " + aktuellerBenutzer.anzeigename : "");
    document.getElementById("startDatum").textContent =
      WOCHENTAGE[h.getDay()] + ", " + h.getDate() + ". " + MONATSNAMEN_LANG[h.getMonth()] + " " + h.getFullYear();

    invoke("uebersicht")
      .then(function (u) {
        var istInhaber = !aktuellerBenutzer || aktuellerBenutzer.rolle !== "mitarbeiterin";
        function kachel(wert, text, filter, klasse) {
          return '<button type="button" class="kachel kachel-knopf ' + (klasse || "") + '" data-filter="' + filter + '">' +
            "<b>" + wert + "</b><span>" + text + "</span></button>";
        }
        document.getElementById("startKacheln").innerHTML =
          kachel(u.heute.length, "heute abholen", "laufend", u.heute.length ? "kachel-gut" : "") +
          kachel(u.ueberfaellig.length, "überfällig", "laufend", u.ueberfaellig.length ? "kachel-warn" : "") +
          kachel(u.abholbereit_anzahl, "abholbereit", "abholbereit") +
          kachel(u.laufend_anzahl, "laufende Aufträge", "laufend") +
          kachel(u.unbezahlt_anzahl + " · " + chf(u.unbezahlt_summe), "offene Posten (CHF)", "unbezahlt", u.unbezahlt_anzahl ? "kachel-warn" : "") +
          (istInhaber ? kachel(chf(u.umsatz_monat), "Umsatz " + MONATSNAMEN_LANG[h.getMonth()], "alle") : "");
        document.getElementById("startHeute").innerHTML = kurzlisteHtml(u.heute, "Heute ist nichts zum Abholen eingetragen.");
        document.getElementById("startUeberfaellig").innerHTML = kurzlisteHtml(u.ueberfaellig, "Nichts überfällig.");

        document.querySelectorAll("#startKacheln [data-filter]").forEach(function (b) {
          b.addEventListener("click", function () { auFilter = b.dataset.filter; reiterOeffnen("r-auftraege"); });
        });
        document.querySelectorAll("#t-start .klickzeile").forEach(function (tr) {
          tr.addEventListener("click", function () { kundeOeffnen(Number(tr.dataset.kunde)); });
        });
      })
      .catch(function (e) {
        document.getElementById("startKacheln").innerHTML = '<p class="fehler">' + fehlerText(e) + "</p>";
      });
  }

  document.getElementById("startNeuerAuftrag").addEventListener("click", function () {
    reiterOeffnen("r-auftraege");
    document.getElementById("au-kunde").focus();
  });
  document.getElementById("startKundeSuchen").addEventListener("click", function () {
    reiterOeffnen("r-arbeit");
    elSuche.focus();
  });

  // ================= AUFTRAEGE =================
  // Annahme: Kundin ueber Kunden-Nr. (oder Name) waehlen, Arbeiten und
  // Abholdatum erfassen. Abgerechnet wird beim Abholen im Kundenblatt.
  var auFilter = "laufend";
  var auKunde = null;
  var auPosten = [neuePostenzeile()];
  var elAuKunde = document.getElementById("au-kunde");
  var elAuTreffer = document.getElementById("au-kunde-treffer");
  var elAuGewaehlt = document.getElementById("au-kunde-gewaehlt");
  var elAuPosten = document.getElementById("au-posten");
  var elAuFehler = document.getElementById("au-fehler");
  var elAuErfolg = document.getElementById("au-erfolg");

  function auKundeSetzen(k) {
    auKunde = k;
    elAuTreffer.innerHTML = "";
    if (!k) { elAuGewaehlt.hidden = true; return; }
    elAuKunde.value = String(k.nummer);
    elAuGewaehlt.hidden = false;
    elAuGewaehlt.innerHTML = "<strong>Nr. " + k.nummer + " · " + escapeHtml(k.vorname + " " + k.name) + "</strong>" +
      (k.ort ? " · " + escapeHtml(k.ort) : "") + (k.telefon ? " · " + escapeHtml(k.telefon) : "") +
      (k.offen_summe > 0 ? ' · <span style="color:var(--faden)">offen CHF ' + chf(k.offen_summe) + "</span>" : "");
  }

  var auKundeSuchen = debounce(function () {
    var text = elAuKunde.value.trim();
    auKunde = null;
    elAuGewaehlt.hidden = true;
    elAuTreffer.innerHTML = "";
    if (!text) return;
    if (/^\d+$/.test(text)) {
      invoke("kunde_nach_nummer", { nummer: Number(text) })
        .then(auKundeSetzen)
        .catch(function (e) { elAuTreffer.innerHTML = '<span class="fehler">' + fehlerText(e) + "</span>"; });
      return;
    }
    invoke("kunden_suchen", { suchtext: text, archiv_zeigen: true }).then(function (kunden) {
      if (!kunden.length) { elAuTreffer.innerHTML = '<span class="fehler">Niemand gefunden.</span>'; return; }
      var auswahl = kunden.slice(0, 6);
      elAuTreffer.innerHTML = auswahl.map(function (k, i) {
        return '<button type="button" class="knopf knopf-klein" data-treffer="' + i + '">Nr. ' + k.nummer + " · " +
          escapeHtml(k.vorname + " " + k.name) + (k.ort ? " · " + escapeHtml(k.ort) : "") + "</button>";
      }).join("") + (kunden.length > 6 ? '<span style="font-size:12px;color:var(--tinte-2)">… genauer suchen</span>' : "");
      elAuTreffer.querySelectorAll("[data-treffer]").forEach(function (b) {
        b.addEventListener("click", function () { auKundeSetzen(auswahl[Number(b.dataset.treffer)]); });
      });
    });
  }, 200);
  elAuKunde.addEventListener("input", auKundeSuchen);

  function auPostenZeichnen() {
    elAuPosten.innerHTML = POSTEN_KOPF_HTML + postenZeilenHtml(auPosten) +
      '<div class="knopfreihe"><button type="button" class="knopf" id="au-zeile-plus">+ Zeile</button></div>' +
      '<div class="endsumme"><span>Total</span><b class="zahl">CHF ' + chf(postenSummeVon(auPosten)) + "</b></div>";
    postenVerdrahten(
      elAuPosten,
      auPosten,
      function () { elAuPosten.querySelector(".endsumme b").textContent = "CHF " + chf(postenSummeVon(auPosten)); },
      auPostenZeichnen
    );
    document.getElementById("au-zeile-plus").addEventListener("click", function () {
      auPosten.push(neuePostenzeile());
      auPostenZeichnen();
    });
  }

  document.getElementById("au-speichern").addEventListener("click", function () {
    elAuFehler.hidden = true;
    elAuErfolg.hidden = true;
    var gueltig = auPosten.filter(function (p) { return p.bezeichnung.trim() && p.stueck > 0; });
    if (!auKunde) { elAuFehler.textContent = "Bitte zuerst eine Kundin wählen (Kunden-Nr. oder Name)."; elAuFehler.hidden = false; return; }
    if (!gueltig.length) { elAuFehler.textContent = "Mindestens eine Arbeit mit Bezeichnung nötig."; elAuFehler.hidden = false; return; }

    var knopf = document.getElementById("au-speichern");
    knopfSperren(knopf, true);
    var kunde = auKunde;
    invoke("auftrag_annehmen", {
      eingabe: { kunde_id: kunde.id, posten: gueltig, abholdatum: document.getElementById("au-abholdatum").value || null },
    })
      .then(function (a) {
        elAuErfolg.textContent = "Auftrag Nr. " + a.rechnungsnummer + " für " + kunde.vorname + " " + kunde.name +
          " gespeichert (CHF " + chf(a.summe) + ")" + (a.abholdatum ? ", abholen am " + datumKurz(a.abholdatum) : "") + ".";
        elAuErfolg.hidden = false;
        auPosten = [neuePostenzeile()];
        auPostenZeichnen();
        elAuKunde.value = "";
        auKundeSetzen(null);
        document.getElementById("au-abholdatum").value = "";
        auListeLaden();
      })
      .catch(function (e) { elAuFehler.textContent = fehlerText(e); elAuFehler.hidden = false; })
      .finally(function () { knopfSperren(knopf, false); });
  });

  // "+ Neue Kundin": derselbe Dialog wie im Reiter Arbeiten, danach wird
  // die neue Kundin hier direkt uebernommen.
  document.getElementById("au-neue-kundin").addEventListener("click", function () {
    document.getElementById("neuerKundeKnopf").click();
    nkNachSpeichern = auKundeSetzen;
  });

  function auListeLaden() {
    document.querySelectorAll("#auFilter button").forEach(function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.filter === auFilter));
    });
    var elListe = document.getElementById("auListe");
    var elZusammenfassung = document.getElementById("auZusammenfassung");
    invoke("auftraege_liste", { filter: auFilter })
      .then(function (zeilen) {
        if (auFilter === "unbezahlt") {
          var total = zeilen.reduce(function (s, z) { return s + z.summe; }, 0);
          elZusammenfassung.textContent = zeilen.length + (zeilen.length === 1 ? " offener Posten" : " offene Posten") +
            " · total CHF " + chf(total);
          elZusammenfassung.hidden = false;
        } else {
          elZusammenfassung.hidden = true;
        }
        if (!zeilen.length) {
          elListe.innerHTML = '<tr><td colspan="8" class="leer">Keine Aufträge in dieser Ansicht.</td></tr>';
          return;
        }
        elListe.innerHTML = zeilen.map(function (z) {
          var laufend = z.status !== "Abgeholt";
          var aktion = laufend
            ? '<button type="button" class="knopf knopf-voll knopf-klein" data-abrechnen-kunde="' + z.kunde_id + '" data-abrechnen-id="' + z.id + '">Abrechnen</button>'
            : !z.bezahlt
              ? '<button type="button" class="knopf knopf-klein" data-bezahlt-id="' + z.id + '">bezahlt ✓</button>'
              : '<span class="zahlart">' + escapeHtml(z.zahlart) + "</span>";
          var alter = auFilter === "unbezahlt"
            ? ' <span style="font-size:12px;color:var(--tinte-3)">(' + z.alter_tage + (z.alter_tage === 1 ? " Tag" : " Tage") + ")</span>"
            : "";
          return "<tr>" +
            "<td>" + z.rechnungsnummer + "</td>" +
            '<td><button type="button" class="knopf knopf-klein" data-kunde-id="' + z.kunde_id + '" title="Kundenblatt öffnen">' +
            "Kd. " + z.kunde_nummer + " · " + escapeHtml(z.kunde_name) + "</button></td>" +
            "<td>" + escapeHtml(z.arbeit) + "</td>" +
            "<td>" + datumKurz(z.angenommen_am || z.datum) + alter + "</td>" +
            '<td class="' + (istUeberfaellig(z) ? "ueberfaellig" : "") + '">' + (z.abholdatum ? datumKurz(z.abholdatum) : "–") + "</td>" +
            "<td>" + (laufend ? statusWahlHtml(z) : (z.bezahlt ? "Abgeholt" : '<span class="zahlart za-offen">offen</span>')) + "</td>" +
            '<td class="re">' + chf(z.summe) + "</td>" +
            "<td>" + aktion + "</td></tr>";
        }).join("");

        elListe.querySelectorAll("[data-kunde-id]").forEach(function (b) {
          b.addEventListener("click", function () { kundeOeffnen(Number(b.dataset.kundeId)); });
        });
        elListe.querySelectorAll("[data-abrechnen-id]").forEach(function (b) {
          b.addEventListener("click", function () {
            kundeOeffnen(Number(b.dataset.abrechnenKunde), Number(b.dataset.abrechnenId));
          });
        });
        elListe.querySelectorAll("[data-bezahlt-id]").forEach(function (b) {
          b.addEventListener("click", function () {
            knopfSperren(b, true);
            invoke("auftrag_bezahlt_markieren", { auftrag_id: Number(b.dataset.bezahltId) })
              .then(auListeLaden)
              .catch(function (e) { alert(fehlerText(e)); knopfSperren(b, false); });
          });
        });
        statusWahlVerdrahten(elListe, auListeLaden);
      })
      .catch(function (e) { elListe.innerHTML = '<tr><td colspan="8" class="leer">' + fehlerText(e) + "</td></tr>"; });
  }

  document.querySelectorAll("#auFilter button").forEach(function (b) {
    b.addEventListener("click", function () { auFilter = b.dataset.filter; auListeLaden(); });
  });

  // ================= PREISLISTE =================
  var plAlle = [];
  var plBearbeitenId = null;
  var elPlListe = document.getElementById("plListe");
  var elPlSuche = document.getElementById("plSuche");
  var elPlInaktive = document.getElementById("plInaktive");
  var elPreisDialog = document.getElementById("preisDialog");

  function plLaden() {
    invoke("preisliste_lesen", { inaktive_zeigen: elPlInaktive.checked })
      .then(function (liste) {
        plAlle = liste;
        plZeichnen();
        var kategorien = [];
        liste.forEach(function (e) { if (e.kategorie && kategorien.indexOf(e.kategorie) === -1) kategorien.push(e.kategorie); });
        document.getElementById("plKategorien").innerHTML = kategorien.map(function (k) {
          return '<option value="' + escapeHtml(k) + '">';
        }).join("");
      })
      .catch(function (e) { elPlListe.innerHTML = '<tr><td colspan="4" class="leer">' + fehlerText(e) + "</td></tr>"; });
    preislisteVorschlaegeLaden();
  }

  function plZeichnen() {
    var such = elPlSuche.value.trim().toLowerCase();
    var liste = plAlle.filter(function (e) {
      return !such || e.bezeichnung.toLowerCase().indexOf(such) !== -1 || e.kategorie.toLowerCase().indexOf(such) !== -1;
    });
    document.getElementById("plTreffer").textContent = liste.length + (liste.length === 1 ? " Eintrag" : " Einträge");
    if (!plAlle.length) {
      elPlListe.innerHTML = '<tr><td colspan="4" class="leer">Die Preisliste ist noch leer – über „Preisliste importieren“ ' +
        "eine Excel-/CSV-Datei einlesen oder einzeln mit „+ Eintrag“ anlegen.</td></tr>";
      return;
    }
    if (!liste.length) {
      elPlListe.innerHTML = '<tr><td colspan="4" class="leer">Nichts gefunden.</td></tr>';
      return;
    }
    elPlListe.innerHTML = liste.map(function (e) {
      return "<tr" + (e.aktiv ? "" : ' style="opacity:.5"') + ">" +
        "<td>" + escapeHtml(e.kategorie || "–") + "</td>" +
        "<td>" + escapeHtml(e.bezeichnung) + (e.aktiv ? "" : ' <span class="merkmal m-archiv">deaktiviert</span>') + "</td>" +
        '<td class="re">' + chf(e.preis) + "</td>" +
        '<td><button type="button" class="knopf knopf-klein" data-pl-bearbeiten="' + e.id + '">Bearbeiten</button> ' +
        '<button type="button" class="knopf knopf-klein" data-pl-aktiv="' + e.id + '" data-wert="' + (e.aktiv ? "0" : "1") + '">' +
        (e.aktiv ? "Deaktivieren" : "Aktivieren") + "</button></td></tr>";
    }).join("");

    elPlListe.querySelectorAll("[data-pl-bearbeiten]").forEach(function (b) {
      b.addEventListener("click", function () {
        var id = Number(b.dataset.plBearbeiten);
        plDialogOeffnen(plAlle.filter(function (e) { return e.id === id; })[0]);
      });
    });
    elPlListe.querySelectorAll("[data-pl-aktiv]").forEach(function (b) {
      b.addEventListener("click", function () {
        invoke("preis_eintrag_aktiv_setzen", { id: Number(b.dataset.plAktiv), aktiv: b.dataset.wert === "1" })
          .then(plLaden)
          .catch(function (e) { alert(fehlerText(e)); });
      });
    });
  }

  function plDialogOeffnen(e) {
    plBearbeitenId = e ? e.id : null;
    document.getElementById("pd-titel").textContent = e ? "Eintrag bearbeiten" : "Neuer Eintrag";
    document.getElementById("pd-bezeichnung").value = e ? e.bezeichnung : "";
    document.getElementById("pd-kategorie").value = e ? e.kategorie : "";
    document.getElementById("pd-preis").value = e ? e.preis : "";
    document.getElementById("pd-fehler").hidden = true;
    elPreisDialog.showModal();
    document.getElementById("pd-bezeichnung").focus();
  }

  document.getElementById("plNeuKnopf").addEventListener("click", function () { plDialogOeffnen(null); });
  document.getElementById("pd-abbrechen").addEventListener("click", function () { elPreisDialog.close(); });
  document.getElementById("pd-speichern").addEventListener("click", function () {
    var elFehler = document.getElementById("pd-fehler");
    var preisText = document.getElementById("pd-preis").value;
    var eingabe = {
      bezeichnung: document.getElementById("pd-bezeichnung").value.trim(),
      kategorie: document.getElementById("pd-kategorie").value.trim(),
      preis: preisText === "" ? NaN : Number(preisText),
    };
    if (!eingabe.bezeichnung || !(eingabe.preis >= 0)) {
      elFehler.textContent = "Bitte Arbeit und einen Preis (0 oder mehr) eingeben.";
      elFehler.hidden = false;
      return;
    }
    invoke("preis_eintrag_speichern", { id: plBearbeitenId, eingabe: eingabe })
      .then(function () { elPreisDialog.close(); plLaden(); })
      .catch(function (e) { elFehler.textContent = fehlerText(e); elFehler.hidden = false; });
  });
  elPlSuche.addEventListener("input", debounce(plZeichnen, 120));
  elPlInaktive.addEventListener("change", plLaden);

  // --- Preisliste importieren (gleiche Erkennung wie bei Kunden/Ausgaben) ---
  var elPliDialog = document.getElementById("preislisteImportDialog");
  var elPliText = document.getElementById("pli-text");
  var elPliKopfzeile = document.getElementById("pli-kopfzeile");
  var elPliVorschau = document.getElementById("pli-vorschau");
  var elPliFehler = document.getElementById("pli-fehler");
  var elPliImportierenKnopf = document.getElementById("pli-importieren");
  var pliGueltige = [];

  var PLI_FELD_SYNONYME = {
    bezeichnung: ["bezeichnung", "arbeit", "leistung", "beschreibung", "artikel", "taetigkeit", "tatigkeit", "position"],
    kategorie: ["kategorie", "gruppe", "rubrik", "bereich", "art"],
    preis: ["preis", "chf", "betrag", "kosten", "tarif", "fr"],
  };

  function pliKopfzeileZuordnen(spalten) {
    var zuordnung = {};
    spalten.forEach(function (roh, i) {
      var text = kiTextNormalisieren(roh);
      if (!text) return;
      // Kurze Begriffe ("Fr", "Art", "CHF") nur als ganze Ueberschrift, sonst
      // wuerde z.B. "Artikel" als Kategorie oder "für" als Preis erkannt.
      ["preis", "bezeichnung", "kategorie"].forEach(function (feld) {
        if (zuordnung[feld] !== undefined) return;
        if (Object.keys(zuordnung).some(function (f) { return zuordnung[f] === i; })) return;
        var passt = PLI_FELD_SYNONYME[feld].some(function (s) {
          return text === s || (s.length > 3 && text.indexOf(s) !== -1);
        });
        if (passt) zuordnung[feld] = i;
      });
    });
    return zuordnung;
  }

  function pliVorschauZeichnen(kopfzeileVonHand) {
    var zeilen = kiZeilenAufteilen(elPliText.value);
    if (!zeilen.length) {
      elPliVorschau.innerHTML = "";
      elPliImportierenKnopf.disabled = true;
      pliGueltige = [];
      return;
    }
    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    var tabelle = zeilen.map(function (z) { return kiZeileSpalten(z, trenner); });
    var zuordnung = pliKopfzeileZuordnen(tabelle[0]);
    var kopfErkannt = zuordnung.bezeichnung !== undefined && zuordnung.preis !== undefined;
    if (!kopfzeileVonHand) elPliKopfzeile.checked = kopfErkannt;

    // Ohne Kopfzeile: 2 Spalten = Arbeit, Preis; sonst Arbeit, Kategorie, Preis.
    var zweiSpaltig = tabelle[0].length <= 2;
    var bi = kopfErkannt ? zuordnung.bezeichnung : 0;
    var ki = kopfErkannt ? zuordnung.kategorie : (zweiSpaltig ? undefined : 1);
    var pi = kopfErkannt ? zuordnung.preis : (zweiSpaltig ? 1 : 2);

    var eintraege = (elPliKopfzeile.checked ? tabelle.slice(1) : tabelle).map(function (sp) {
      return {
        bezeichnung: String(sp[bi] || "").trim(),
        kategorie: ki !== undefined ? String(sp[ki] || "").trim() : "",
        preis: thiBetragNormalisieren(sp[pi]),
      };
    });
    pliGueltige = eintraege.filter(function (e) { return e.bezeichnung && e.preis >= 0; });
    var ungueltig = eintraege.length - pliGueltige.length;

    var zeilenHtml = eintraege.slice(0, 50).map(function (e) {
      var ok = e.bezeichnung && e.preis >= 0;
      return "<tr" + (ok ? "" : ' class="zeile-uebersprungen"') + "><td>" + escapeHtml(e.kategorie || "–") + "</td><td>" +
        escapeHtml(e.bezeichnung || "–") + "</td><td>" + (e.preis >= 0 ? chf(e.preis) : "–") + "</td></tr>";
    }).join("");
    elPliVorschau.innerHTML =
      '<div class="import-zusammenfassung">' +
      (kopfErkannt ? "Kopfzeile erkannt – Spalten automatisch zugeordnet. "
        : "Keine Kopfzeile erkannt – Reihenfolge " + (zweiSpaltig ? "Arbeit, Preis" : "Arbeit, Kategorie, Preis") + " angenommen. ") +
      pliGueltige.length + " Einträge werden übernommen" +
      (ungueltig ? ", " + ungueltig + " ohne Arbeit oder gültigen Preis werden übersprungen" : "") +
      (eintraege.length > 50 ? " (zeigt die ersten 50 von " + eintraege.length + ")" : "") + "</div>" +
      '<div class="tabellenrahmen"><table class="auflistung"><thead><tr><th>Kategorie</th><th>Arbeit</th><th>CHF</th></tr></thead><tbody>' +
      zeilenHtml + "</tbody></table></div>";
    elPliImportierenKnopf.disabled = pliGueltige.length === 0;
  }

  document.getElementById("plImportKnopf").addEventListener("click", function () {
    elPliText.value = "";
    elPliVorschau.innerHTML = "";
    elPliFehler.hidden = true;
    elPliKopfzeile.checked = false;
    elPliImportierenKnopf.disabled = true;
    pliGueltige = [];
    elPliDialog.showModal();
    elPliText.focus();
  });
  document.getElementById("pli-abbrechen").addEventListener("click", function () { elPliDialog.close(); });
  elPliText.addEventListener("input", debounce(function () { pliVorschauZeichnen(false); }, 150));
  elPliKopfzeile.addEventListener("change", function () { pliVorschauZeichnen(true); });
  document.getElementById("pli-datei").addEventListener("click", function () {
    elPliFehler.hidden = true;
    dateiFuerImportLesen(
      elPliText,
      function () { pliVorschauZeichnen(false); },
      function (meldung) { elPliFehler.textContent = meldung; elPliFehler.hidden = false; }
    );
  });
  elPliImportierenKnopf.addEventListener("click", function () {
    if (!pliGueltige.length) return;
    elPliFehler.hidden = true;
    knopfSperren(elPliImportierenKnopf, true);
    invoke("preisliste_importieren", { eingaben: pliGueltige })
      .then(function (r) {
        elPliDialog.close();
        plLaden();
        alert(r.neu + " neu übernommen, " + r.aktualisiert + " bestehende aktualisiert.");
      })
      .catch(function (e) { elPliFehler.textContent = fehlerText(e); elPliFehler.hidden = false; })
      .finally(function () { knopfSperren(elPliImportierenKnopf, false); });
  });

  // ================= REITER =================
  document.querySelectorAll('[role="tab"]').forEach(function (t) {
    t.addEventListener("click", function () {
      document.querySelectorAll('[role="tab"]').forEach(function (x) {
        var an = x === t;
        x.setAttribute("aria-selected", an);
        document.getElementById(x.getAttribute("aria-controls")).hidden = !an;
      });
      if (t.id === "r-start") uebersichtLaden();
      if (t.id === "r-auftraege") auListeLaden();
      if (t.id === "r-preisliste") plLaden();
      if (t.id === "r-monat") { monatLaden(); maListeLaden(); }
      if (t.id === "r-stunden") stMonatLaden();
      if (t.id === "r-treuhand") thLaden();
    });
  });

  // ================= START =================
  function programmStarten() {
    suchtextSuchen();
    elBlatt.innerHTML = '<p class="leer" style="padding:40px">Links einen Kunden wählen oder „+ Neuer Kunde".</p>';
    auPostenZeichnen();
    preislisteVorschlaegeLaden();
    reiterOeffnen("r-start");
  }
})();
