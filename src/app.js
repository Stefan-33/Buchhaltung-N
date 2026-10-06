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
    var schrift;
    try { schrift = localStorage.getItem("schrift"); } catch (e) { schrift = null; }
    if (schrift === "gross") document.documentElement.classList.add("schrift-gross");
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
    quittung_logo_pfad: null,
    geschaeft_email: "",
    beleg_vorlage: "klassisch",
    beleg_format: "A5",
    beleg_farbe: "#92D050",
    beleg_titel_zusatz: "für Aenderungen / Reparaturen",
    beleg_dank: "Besten Dank",
    beleg_zahlungshinweis: "",
    beleg_logo_zeigen: true,
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

  // Eine Mitarbeiterin sieht nur "Kunden" (Kunden suchen/anlegen/
  // importieren, Aufträge buchen) und unter "Mitarbeiter" nur ihre eigenen
  // Stunden (keine Personenwahl, kein Lohn, keine Formulare) - Umsatz,
  // Auswertung, Sicherung, das Anlegen weiterer Konten und die
  // Stunden-Uebersicht aller Mitarbeiterinnen bleiben Papa/Mama (Rolle
  // "inhaber") vorbehalten. Reine Oberflaechen-Einschraenkung, keine
  // scharfe Zugriffssperre - passt zum ueberschaubaren familiaeren Rahmen
  // hier.
  function rolleAnwenden(rolle) {
    var istMitarbeiterin = rolle === "mitarbeiterin";
    document.getElementById("r-monat").hidden = istMitarbeiterin;
    document.getElementById("r-treuhand").hidden = istMitarbeiterin;
    document.getElementById("gruppeAuswertung").hidden = istMitarbeiterin;
    document.getElementById("stAlleTafel").hidden = istMitarbeiterin;
    document.getElementById("maPersonZeile").hidden = istMitarbeiterin;
    document.getElementById("maUnterreiter").hidden = istMitarbeiterin;
    document.getElementById("r-stunden").textContent = istMitarbeiterin ? "Meine Stunden" : "Mitarbeiter";
    maPersonId = null;
    maAnsicht = "stunden";
    // Geschaeftsangaben und Kartengebuehr-Saetze gelten fuer den ganzen
    // Betrieb, nicht fuer eine einzelne Person - nur Papa/Mama aendern die.
    document.getElementById("eiGeschaeftTafel").hidden = istMitarbeiterin;
    document.getElementById("eiKartenTafel").hidden = istMitarbeiterin;
    document.getElementById("eiLohnTafel").hidden = istMitarbeiterin;
    document.getElementById("eiUebergabeTafel").hidden = istMitarbeiterin;
    document.getElementById("eiBelegTafel").hidden = istMitarbeiterin;
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
  // alleBlaetter: bei einer Excel-Datei jedes Tabellenblatt lesen (z. B.
  // ein Blatt pro Jahr), nicht nur das erste.
  // blattnamen: vor jedes Blatt eine Zeile "#BLATT <Name>" (Treuhand-Import
  // liest daraus den Monat, z. B. "Juni" / "Juni A").
  function dateiFuerImportLesen(zielTextarea, nachErfolg, aufFehler, alleBlaetter, blattnamen) {
    if (!window.__TAURI__.dialog || !window.__TAURI__.dialog.open) {
      aufFehler("Dateiauswahl ist in dieser Programmversion nicht verfügbar.");
      return;
    }
    window.__TAURI__.dialog
      .open({ multiple: false, filters: [{ name: "Tabellen", extensions: ["xlsx", "xls", "csv"] }] })
      .then(function (pfad) {
        if (!pfad) return; // Dialog abgebrochen
        return invoke("datei_als_tabelle_lesen", { pfad: pfad, alle_blaetter: !!alleBlaetter, blattnamen: !!blattnamen }).then(function (tabelle) {
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
    namevoll_nv: ["namevorname", "nachnamevorname"],
    namevoll_vn: ["vornamename", "vornamenachname", "vollername"],
    nummer: ["nummer", "kundennummer", "knr", "kdnr"],
    name: ["name", "nachname", "familienname"],
    vorname: ["vorname"],
    ort: ["ort", "wohnort", "stadt"],
    adresse: ["adresse", "strasse", "straße", "wohnadresse"],
    email: ["email", "mail"],
    notiz: ["notiz", "bemerkung", "anmerkung", "spez", "spezial", "hinweis"],
    plz: ["plz", "postleitzahl"],
  };
  // Auswahl pro Spalte in der Vorschau ("Spalten zuordnen") - damit sich
  // eine falsch erkannte Spalte von Hand korrigieren laesst.
  var KI_FELDER = [
    ["", "– nicht übernehmen –"], ["nummer", "Kunden-Nr."], ["name", "Name"], ["vorname", "Vorname"],
    ["namevoll_vn", "Vorname Name (zusammen)"], ["namevoll_nv", "Name Vorname (zusammen)"],
    ["telefon", "Telefon"], ["adresse", "Strasse / Adresse"], ["plz", "PLZ"], ["ort", "Ort"],
    ["email", "E-Mail"], ["notiz", "Notiz"],
  ];
  var kiTabelle = [];   // eingefuegte/gelesene Tabelle, Zeilen von Zellen
  var kiKopfIndex = -1; // Zeile mit den Spaltenueberschriften, -1 = keine
  var kiSpaltenFeld = []; // Feld pro Spalte (automatisch erkannt)
  var kiVonHand = {};   // Spalte -> Feld, von Hand gewaehlt
  var KI_STANDARD_REIHENFOLGE = ["name", "vorname", "telefon", "ort", "adresse", "email"];
  // Reihenfolge = Prioritaet: die erste gefundene, nicht-leere Nummer
  // einer Zeile wird das Telefon-Hauptfeld, alle weiteren vorhandenen
  // landen beschriftet in der Notiz (siehe Stefans Telefonliste: Mobil,
  // Privat, Geschäft, Ausland in eigenen Spalten).
  var KI_TELEFON_PRIORITAET = ["mobil", "handy", "natel", "telefon", "tel", "privat", "festnetz", "geschaeft", "gesch", "business", "ausland", "fax"];

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

  // Wie sehr eine Zeile nach Kopfzeile aussieht: nur Zellen, die exakt
  // eine bekannte Ueberschrift sind ("Name", "Vorname", "Telefon" ...).
  // Teiltreffer zaehlen hier bewusst nicht - sonst saehe eine Datenzeile
  // mit "Seestrasse 1" und "anna@mail.ch" auch wie eine Kopfzeile aus.
  var KI_KOPF_EXAKT = [].concat.apply(["tel", "telefon", "mobil", "natel", "handy", "privat"],
    Object.keys(KI_FELD_SYNONYME).map(function (f) { return KI_FELD_SYNONYME[f]; }));
  function kiKopfTreffer(spalten) {
    var anzahl = spalten.filter(function (z) { return KI_KOPF_EXAKT.indexOf(kiTextNormalisieren(z)) !== -1; }).length;
    return { anzahl: anzahl, ergebnis: anzahl >= 2 ? kiKopfzeileZuordnen(spalten) : null };
  }

  // Sucht in den ersten 30 Zeilen die Kopfzeile (die mit den meisten
  // erkannten Ueberschriften, mindestens 2) - davor stehen oft ein Titel
  // oder leere Zeilen, und die Tabelle beginnt nicht immer in Spalte A.
  function kiKopfSuchen(tabelle) {
    var beste = -1, besteAnzahl = 1;
    for (var i = 0; i < Math.min(tabelle.length, 30); i++) {
      var n = kiKopfTreffer(tabelle[i]).anzahl;
      if (n > besteAnzahl) { beste = i; besteAnzahl = n; }
    }
    return beste;
  }

  function kiFeldFuer(i) {
    return Object.prototype.hasOwnProperty.call(kiVonHand, i) ? kiVonHand[i] : (kiSpaltenFeld[i] || "");
  }

  function kiEintragBauen(spalten, kopf) {
    var e = { nummer: "", name: "", vorname: "", telefon: "", ort: "", adresse: "", email: "", notiz: "" };
    var plz = "", telefone = [], notizen = [];
    spalten.forEach(function (roh, i) {
      var wert = String(roh || "").trim();
      var feld = kiFeldFuer(i);
      if (!wert || !feld) return;
      if (feld === "telefon") {
        telefone.push({ label: String((kopf && kopf[i]) || "Tel.").trim(), wert: wert });
      } else if (feld === "notiz") {
        notizen.push(wert);
      } else if (feld === "plz") {
        plz = wert;
      } else if (feld === "namevoll_vn" || feld === "namevoll_nv") {
        var teile = wert.split(/\s+/);
        if (teile.length === 1) { e.name = e.name || teile[0]; return; }
        if (feld === "namevoll_vn") { e.name = e.name || teile.pop(); e.vorname = e.vorname || teile.join(" "); }
        else { e.name = e.name || teile.shift(); e.vorname = e.vorname || teile.join(" "); }
      } else if (!e[feld]) {
        e[feld] = wert;
      }
    });
    if (plz) e.ort = (plz + " " + e.ort).trim();
    if (telefone.length) {
      e.telefon = telefone[0].wert;
      telefone.slice(1).forEach(function (t) { notizen.push(t.label + ": " + t.wert); });
    }
    e.notiz = notizen.join(" · ");
    return e;
  }

  // Zerlegt den eingefuegten Text neu und erkennt die Spalten. "vonHand"
  // bleibt bei einer reinen Neuberechnung (Auswahl geaendert) erhalten,
  // bei neuem Text wird sie verworfen.
  function kiNeuVerarbeiten(zuordnungBehalten) {
    if (!zuordnungBehalten) kiVonHand = {};
    var zeilen = kiZeilenAufteilen(elKiText.value);
    if (!zeilen.length) { kiTabelle = []; kiEintraegeAlle = []; kiVorschauZeichnen(); return; }
    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    kiTabelle = zeilen.map(function (z) { return kiZeileSpalten(z, trenner); });
    kiKopfIndex = kiKopfSuchen(kiTabelle);
    kiSpaltenFeld = [];
    if (kiKopfIndex >= 0) {
      var e = kiKopfTreffer(kiTabelle[kiKopfIndex]).ergebnis;
      Object.keys(e.zuordnung).forEach(function (i) { kiSpaltenFeld[i] = e.zuordnung[i]; });
      e.telefonSpalten.forEach(function (t) { kiSpaltenFeld[t.index] = "telefon"; });
    } else {
      // Ohne Kopfzeile: Name, Vorname, Telefon, Ort, Adresse, E-Mail ab der
      // ersten Spalte, in der ueberhaupt etwas steht.
      var start = Math.min.apply(null, kiTabelle.map(function (z) {
        var i = z.findIndex(function (w) { return String(w || "").trim(); });
        return i < 0 ? 999 : i;
      }));
      KI_STANDARD_REIHENFOLGE.forEach(function (feld, i) { kiSpaltenFeld[start + i] = feld; });
    }
    var kopf = kiKopfIndex >= 0 ? kiTabelle[kiKopfIndex] : null;
    kiEintraegeAlle = kiTabelle
      .filter(function (z, i) {
        if (i <= kiKopfIndex) return false; // Titel und Kopfzeile
        return kiKopfTreffer(z).anzahl < 2; // wiederholte Kopfzeile (weiteres Blatt)
      })
      .map(function (z) { return kiEintragBauen(z, kopf); })
      .filter(function (e) { return Object.keys(e).some(function (k) { return e[k]; }); });
    kiVorschauZeichnen();
  }

  function kiSpaltenBuchstabe(i) {
    return i < 26 ? String.fromCharCode(65 + i) : "A" + String.fromCharCode(65 + i - 26);
  }

  function kiZuordnungHtml() {
    var breite = kiTabelle.reduce(function (m, z) { return Math.max(m, z.length); }, 0);
    var spalten = [];
    for (var i = 0; i < breite; i++) {
      var belegt = kiTabelle.some(function (z, zi) { return zi > kiKopfIndex && String(z[i] || "").trim(); });
      if (belegt) spalten.push(i);
    }
    if (!spalten.length) return "";
    var beispiel = function (i) {
      var z = kiTabelle.filter(function (zz, zi) { return zi > kiKopfIndex && String(zz[i] || "").trim(); })[0];
      return z ? String(z[i]).trim() : "";
    };
    return '<div class="ki-zuordnung"><b>Spalten zuordnen</b> – stimmt etwas nicht, hier von Hand ändern:' +
      '<div class="tabellenrahmen"><table><tr>' +
      spalten.map(function (i) {
        var titel = kiKopfIndex >= 0 ? String(kiTabelle[kiKopfIndex][i] || "").trim() : "";
        return "<th>Spalte " + kiSpaltenBuchstabe(i) + (titel ? "<br><span>" + escapeHtml(titel) + "</span>" : "") + "</th>";
      }).join("") + "</tr><tr>" +
      spalten.map(function (i) {
        var feld = kiFeldFuer(i);
        return '<td><select data-ki-spalte="' + i + '" aria-label="Spalte ' + kiSpaltenBuchstabe(i) + '">' +
          KI_FELDER.map(function (f) { return '<option value="' + f[0] + '"' + (f[0] === feld ? " selected" : "") + ">" + f[1] + "</option>"; }).join("") +
          '</select><small>z. B. ' + escapeHtml(beispiel(i).slice(0, 30)) + "</small></td>";
      }).join("") + "</tr></table></div></div>";
  }

  function kiVorschauZeichnen() {
    if (!kiEintraegeAlle.length) {
      elKiVorschau.innerHTML = elKiText.value.trim()
        ? '<div class="import-zusammenfassung">Keine Kundenzeilen gefunden.</div>' + kiZuordnungHtml()
        : "";
      elKiImportierenKnopf.disabled = true;
      kiZuordnungVerdrahten();
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

    var hinweisKopf = kiKopfIndex >= 0
      ? "Kopfzeile erkannt (" + escapeHtml(kiTabelle[kiKopfIndex].map(function (z) { return String(z || "").trim(); })
          .filter(Boolean).slice(0, 5).join(", ")) + " …) – Spalten automatisch zugeordnet."
      : "Keine Kopfzeile erkannt – Reihenfolge Name, Vorname, Telefon, Ort, Adresse, E-Mail angenommen.";
    var mehrHinweis = kiEintraegeAlle.length > 50 ? " (zeigt die ersten 50 von " + kiEintraegeAlle.length + ")" : "";
    var namenHinweis = "<b>" + mitName.length + (mitName.length === 1 ? " Kundin" : " Kundinnen") + "</b> erkannt";
    if (ohneName) {
      namenHinweis += ", " + ohneName + " Zeile" + (ohneName === 1 ? "" : "n") +
        " ohne Namen wird" + (ohneName === 1 ? "" : "en") + " übersprungen (durchgestrichen)";
    }

    elKiVorschau.innerHTML =
      '<div class="import-zusammenfassung">' + hinweisKopf + "<br>" + namenHinweis + mehrHinweis +
      ". Wer schon im Programm ist, wird nicht doppelt angelegt.</div>" +
      kiZuordnungHtml() +
      '<div class="tabellenrahmen"><table class="auflistung"><thead><tr>' +
      "<th>Nr.</th><th>Name</th><th>Vorname</th><th>Telefon</th><th>Ort</th><th>Adresse</th><th>E-Mail</th><th>Notiz</th>" +
      "</tr></thead><tbody>" + zeilenHtml + "</tbody></table></div>";

    elKiImportierenKnopf.disabled = mitName.length === 0;
    kiZuordnungVerdrahten();
  }

  function kiZuordnungVerdrahten() {
    elKiVorschau.querySelectorAll("select[data-ki-spalte]").forEach(function (sel) {
      sel.addEventListener("change", function () {
        kiVonHand[Number(sel.dataset.kiSpalte)] = sel.value;
        kiNeuVerarbeiten(true);
      });
    });
  }

  document.getElementById("kundenImportKnopf").addEventListener("click", function () {
    elKiText.value = "";
    kiVonHand = {};
    elKiVorschau.innerHTML = "";
    elKiFehler.hidden = true;
    elKiImportierenKnopf.disabled = true;
    kiEintraegeAlle = [];
    elKiDialog.showModal();
    elKiText.focus();
  });
  document.getElementById("ki-abbrechen").addEventListener("click", function () { elKiDialog.close(); });
  elKiText.addEventListener("input", debounce(function () { kiNeuVerarbeiten(false); }, 150));
  document.getElementById("ki-datei").addEventListener("click", function () {
    elKiFehler.hidden = true;
    dateiFuerImportLesen(
      elKiText,
      function () { kiNeuVerarbeiten(false); },
      function (meldung) { elKiFehler.textContent = meldung; elKiFehler.hidden = false; },
      true
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
      .then(function (r) {
        elKiDialog.close();
        elSuche.value = "";
        suchtextSuchen();
        alert(r.neu + (r.neu === 1 ? " Kundin wurde importiert." : " Kundinnen wurden importiert.") +
          (r.doppelt ? "\n" + r.doppelt + " waren schon im Programm und wurden nicht doppelt angelegt." : ""));
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

  // Feste Statusfarben, ueberall gleich: Angenommen grau, In Arbeit blau,
  // Abholbereit gruen, ueberfaellig oder noch offen (unbezahlt) orange.
  function statusArt(a) {
    if (istUeberfaellig(a)) return "warn";
    if (a.status === "Abgeholt") return a.bezahlt ? "" : "warn";
    return { "In Arbeit": "arbeit", "Abholbereit": "bereit" }[a.status] || "angenommen";
  }

  function statusChip(a) {
    var text = istUeberfaellig(a) ? "überfällig" : a.status === "Abgeholt" ? (a.bezahlt ? "bezahlt" : "offen") : a.status;
    return '<span class="status st-' + (statusArt(a) || "angenommen") + '">' + escapeHtml(text) + "</span>";
  }

  // Farbstreifen links an einer Auftragszeile (Klassen fuer <tr>).
  function streifenKlasse(a) {
    var art = statusArt(a);
    return art ? "streifen sf-" + art : "";
  }

  function statusWahlHtml(a) {
    var art = { "In Arbeit": "arbeit", "Abholbereit": "bereit" }[a.status] || "angenommen";
    // "Abgeholt" laesst sich nicht einfach setzen - abgeschlossen ist ein
    // Auftrag erst mit dem Abrechnen (Zahlart, Beleg). Die letzte Option
    // fuehrt deshalb direkt dorthin.
    return '<select class="status-wahl st-' + art + '" data-status-id="' + a.id + '" data-status-kunde="' + a.kunde_id + '" aria-label="Status">' +
      STATUS_LAUFEND.map(function (s) {
        return "<option" + (s === a.status ? " selected" : "") + ">" + s + "</option>";
      }).join("") + '<option value="__abrechnen">Abgeholt → abrechnen …</option></select>';
  }

  function statusWahlVerdrahten(container, danach) {
    container.querySelectorAll("select[data-status-id]").forEach(function (sel) {
      sel.addEventListener("change", function () {
        if (sel.value === "__abrechnen") {
          kundeOeffnen(Number(sel.dataset.statusKunde), Number(sel.dataset.statusId));
          return;
        }
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

  // Springt ins Kundenblatt (Reiter "Kunden") - mit "abrechnenId" direkt
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
  var blattAnsicht = "neu"; // Unterreiter im Kundenblatt: "neu", "laufend" oder "verlauf"

  // Auftragsschein (wie die Rechnung, Titel "Auftrag") fuer einen noch
  // nicht abgeholten Auftrag als PDF oeffnen - zum Ausdrucken bei der
  // Annahme. Holt Kundin und Auftrag frisch, damit der Schein stimmt.
  function auftragsscheinDrucken(auftragId, kundeId, knopf) {
    knopfSperren(knopf, true);
    return Promise.all([invoke("kunde_holen", { id: kundeId }), invoke("auftraege_von_kunde", { kunde_id: kundeId })])
      .then(function (e) {
        var auftrag = e[1].filter(function (a) { return a.id === auftragId; })[0];
        if (!auftrag) throw "Auftrag nicht gefunden.";
        return invoke("quittung_als_pdf_oeffnen", { auftrag: auftrag, kunde: e[0] });
      })
      .catch(function (e) { alert(fehlerText(e)); })
      .finally(function () { knopfSperren(knopf, false); });
  }

  function abrechnenStarten(a) {
    abrechnenAuftrag = a;
    blattAnsicht = "neu";
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
        // Hat die Kundin laufende Auftraege, kommt sie meist zum Abholen -
        // dann zuerst diese zeigen, sonst gleich die Erfassung.
        var hatLaufende = aktuelleAuftraege.some(function (a) { return a.status !== "Abgeholt"; });
        blattAnsicht = hatLaufende ? "laufend" : "neu";
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

  function unterreiterHtml(laufende, anzahlVerlauf) {
    var ueberfaellig = laufende.some(istUeberfaellig);
    var reiter = [
      ["neu", abrechnenAuftrag ? "Abrechnen Nr. " + abrechnenAuftrag.rechnungsnummer : "Neuer Auftrag", ""],
      ["laufend", "Laufend", '<span class="unterreiter-zahl' + (ueberfaellig ? " warn" : "") + '">' + laufende.length + "</span>"],
      ["verlauf", "Verlauf", '<span class="unterreiter-zahl">' + anzahlVerlauf + "</span>"],
    ];
    return '<div class="unterreiter" role="tablist" aria-label="Kundenblatt">' +
      reiter.map(function (r) {
        return '<button type="button" role="tab" data-ansicht="' + r[0] + '" aria-selected="' + (blattAnsicht === r[0]) + '">' +
          escapeHtml(r[1]) + " " + r[2] + "</button>";
      }).join("") + "</div>";
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
          '<tr class="' + streifenKlasse(a) + '"><td>' + arbeitText(a) + '<br><span style="font-size:12px;color:var(--tinte-3)">Nr. ' + a.rechnungsnummer +
          " · angenommen " + datumKurz(a.angenommen_am || a.datum) + " · " + abholen + "</span></td>" +
          '<td class="re">' + statusWahlHtml(a) + "</td>" +
          '<td class="re">' + chf(a.summe) + "</td>" +
          '<td class="re">' + (inArbeit
            ? '<span style="font-size:12px;color:var(--tinte-2)">wird abgerechnet</span>'
            : '<button type="button" class="knopf knopf-klein" data-schein="' + a.id + '" title="Auftragsschein drucken">🖨 Auftrag</button> ' +
              '<button type="button" class="knopf knopf-voll knopf-klein" data-abrechnen="' + a.id + '">Abrechnen</button>') +
          "</td></tr>"
        );
      })
      .join("");

    var verlaufZeilen = aktuelleAuftraege
      .filter(function (a) { return a.status === "Abgeholt"; })
      .map(function (a) {
        var offen = !a.bezahlt
          ? " " + statusChip(a) + ' <button type="button" class="knopf knopf-klein" data-bezahlt="' + a.id + '">bezahlt ✓</button>'
          : "";
        return (
          '<tr class="' + streifenKlasse(a) + '"><td class="zahl" style="white-space:nowrap;color:var(--tinte-2)">' + datumKurz(a.datum) + "</td>" +
          "<td>" + arbeitText(a) + '<br><span style="font-size:12px;color:var(--tinte-3)">Nr. ' + a.rechnungsnummer + "</span></td>" +
          '<td class="re"><span class="zahlart">' + escapeHtml(a.zahlart) + "</span>" + offen + "</td>" +
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
      unterreiterHtml(laufende, aktuelleAuftraege.length - laufende.length) +
      (blattAnsicht === "laufend"
        ? '<div class="abschnitt">' +
          (laufendZeilen
            ? '<table class="verlauf"><thead><tr><th>Arbeit</th><th class="re">Status</th><th class="re">CHF</th><th></th></tr></thead>' +
              "<tbody>" + laufendZeilen + "</tbody></table>"
            : '<p style="color:var(--tinte-2);font-size:14px;margin:0">Keine laufenden Aufträge. Neue nimmst du unter „Neuer Auftrag“ ' +
              "oder im Reiter „Aufträge“ an.</p>") +
          "</div>"
        : "") +
      (blattAnsicht === "verlauf"
        ? '<div class="abschnitt">' +
          (verlaufZeilen
            ? '<table class="verlauf"><thead><tr><th>Datum</th><th>Arbeit</th><th class="re">Zahlart</th><th class="re">CHF</th></tr></thead><tbody>' + verlaufZeilen + "</tbody></table>"
            : '<p style="color:var(--tinte-2);font-size:14px;margin:0">Noch keine abgerechneten Aufträge.</p>') +
          '<div id="altesArchiv"></div>' +
          "</div>"
        : "") +
      (blattAnsicht === "neu"
        ? '<div class="abschnitt" id="auftragEditor">' +
          (abrechnenAuftrag
            ? '<p style="margin:0 0 10px;font-size:13px;color:var(--tinte-2)">Auftrag Nr. ' + abrechnenAuftrag.rechnungsnummer +
              " abrechnen: Arbeiten und Preise bei Bedarf anpassen, Zahlart wählen. " +
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
          // Mitlaufende Leiste: Total und Abschliessen bleiben sichtbar,
          // auch wenn die Liste der Arbeiten lang wird.
          '<div class="summenleiste">' +
          '<button type="button" class="knopf knopf-voll" id="abschliessenKnopf"' + (summe <= 0 ? " disabled" : "") + ">" +
          (abrechnenAuftrag ? "Abrechnen &amp; Beleg" : "Auftrag abschliessen &amp; Beleg") + "</button>" +
          '<span id="fertigFehler" class="fehler"></span>' +
          '<span class="summenleiste-total"><span>Total</span><b class="zahl">CHF ' + chf(summe) + "</b></span>" +
          "</div>" +
          "</div>" +
          (letzteQuittung
            ? '<div class="abschnitt">' + quittungHtml(letzteQuittung, k) +
              '<div class="knopfreihe"><button type="button" class="knopf" id="druckenKnopf">Beleg drucken</button>' +
              '<span id="druckenFehler" class="fehler" hidden></span></div></div>'
            : "")
        : "");

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
        var e = elBlatt.querySelector(".summenleiste-total b");
        if (e) e.textContent = "CHF " + chf(postenSumme());
        var knopf = document.getElementById("abschliessenKnopf");
        if (knopf) knopf.disabled = postenSumme() <= 0;
      },
      blattZeichnen
    );

    elBlatt.querySelectorAll(".unterreiter [data-ansicht]").forEach(function (b) {
      b.addEventListener("click", function () { blattAnsicht = b.dataset.ansicht; blattZeichnen(); });
    });

    function kundeNeuLaden() {
      return Promise.all([invoke("kunde_holen", { id: aktuellerKunde.id }), invoke("auftraege_von_kunde", { kunde_id: aktuellerKunde.id })])
        .then(function (ergebnisse) {
          aktuellerKunde = ergebnisse[0];
          aktuelleAuftraege = ergebnisse[1];
          blattZeichnen();
          suchtextSuchen();
        });
    }

    if (blattAnsicht === "verlauf" && aktuellerKunde) altesArchivLaden(aktuellerKunde.id);
    elBlatt.querySelectorAll("[data-schein]").forEach(function (b) {
      b.addEventListener("click", function () { auftragsscheinDrucken(Number(b.dataset.schein), aktuellerKunde.id, b); });
    });
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
        blattAnsicht = "laufend";
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
            zaehlerAktualisieren();
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

  // ================= MITARBEITER =================
  // Ein Reiter fuer alles, was Papa fuer eine Mitarbeiterin eintragen muss:
  // Stunden, Personalien, Lohn und Formulare - umgeschaltet ueber die
  // Unterreiter, die oben gewaehlte "Person" gilt fuer alle vier. Eine
  // angemeldete Mitarbeiterin sieht nur "Stunden" (ihre eigenen).
  var elMaPerson = document.getElementById("maPerson");
  var maPersonen = [];   // alle_benutzer, nur fuer Papa/Mama geladen
  var maPersonId = null; // gewaehlte Person, null = noch nichts gewaehlt
  var maAnsicht = "stunden";

  function istInhaber() { return !!aktuellerBenutzer && aktuellerBenutzer.rolle === "inhaber"; }

  function maPersonIdAktuell() {
    return istInhaber() && maPersonId ? maPersonId : aktuellerBenutzer.id;
  }

  function maPersonAktuell() {
    var id = maPersonIdAktuell();
    return maPersonen.filter(function (b) { return b.id === id; })[0] || aktuellerBenutzer;
  }

  // Volles Datum "17.05.1980" - beim Geburtsdatum waere "17.05.80" zweideutig.
  function datumVoll(iso) {
    return iso ? iso.split("-").reverse().join(".") : "";
  }

  function maMitarbeiterinnen() {
    return maPersonen.filter(function (b) { return b.rolle === "mitarbeiterin"; });
  }

  function maPersonAuswahlZeichnen() {
    var ma = maMitarbeiterinnen();
    var andere = maPersonen.filter(function (b) { return b.rolle !== "mitarbeiterin"; });
    if (!maPersonId || !maPersonen.some(function (b) { return b.id === maPersonId; })) {
      maPersonId = ma.length ? ma[0].id : aktuellerBenutzer.id;
    }
    function optionen(liste) {
      return liste.map(function (b) {
        var zusatz = b.austritt ? " (ausgetreten)" : "";
        return '<option value="' + b.id + '"' + (b.id === maPersonId ? " selected" : "") + ">" + escapeHtml(b.anzeigename) + zusatz + "</option>";
      }).join("");
    }
    elMaPerson.innerHTML =
      (ma.length ? '<optgroup label="Mitarbeiterinnen">' + optionen(ma) + "</optgroup>" : "") +
      (andere.length ? '<optgroup label="Inhaber">' + optionen(andere) + "</optgroup>" : "");
  }

  // Laedt die Personen neu (z. B. nach Anlegen/Bearbeiten) und zeichnet die
  // aktuelle Ansicht.
  function maLaden() {
    if (!istInhaber()) {
      maPersonen = [aktuellerBenutzer];
      maAnsichtZeigen("stunden");
      return Promise.resolve();
    }
    return invoke("alle_benutzer")
      .then(function (benutzer) { maPersonen = benutzer; })
      .catch(function () { maPersonen = [aktuellerBenutzer]; })
      .then(function () {
        maPersonAuswahlZeichnen();
        maAnsichtZeigen(maAnsicht);
        maFormulareZaehlen();
      });
  }

  function maAnsichtZeigen(ansicht) {
    if (!istInhaber()) ansicht = "stunden";
    maAnsicht = ansicht;
    document.querySelectorAll("#maUnterreiter [data-ma-ansicht]").forEach(function (b) {
      b.setAttribute("aria-selected", b.dataset.maAnsicht === ansicht);
    });
    document.querySelectorAll("[data-ma-teil]").forEach(function (teil) {
      teil.hidden = teil.dataset.maTeil !== ansicht;
    });
    if (ansicht === "stunden") stMonatLaden();
    if (ansicht === "personalien") maListeZeichnen();
    if (ansicht === "lohn") maLohnLaden();
    if (ansicht === "formulare") maFormLaden();
  }

  document.querySelectorAll("#maUnterreiter [data-ma-ansicht]").forEach(function (b) {
    b.addEventListener("click", function () { maAnsichtZeigen(b.dataset.maAnsicht); });
  });
  elMaPerson.addEventListener("change", function () {
    maPersonId = Number(elMaPerson.value);
    maAnsichtZeigen(maAnsicht);
    maFormulareZaehlen();
  });

  // Was fuer Lohnabrechnung/Lohnausweis noch fehlt - als Hinweis in "Lohn"
  // und "Formulare", damit Papa es nicht erst beim Ausfuellen merkt.
  function maFehlendeAngaben(p) {
    var fehlt = [];
    if (!p.strasse || !p.plz_ort) fehlt.push("Adresse");
    if (!p.ahv_nummer) fehlt.push("AHV-Nummer");
    if (!p.geburtsdatum) fehlt.push("Geburtsdatum");
    if (!p.eintritt) fehlt.push("Eintritt");
    if (!p.stundenlohn) fehlt.push("Stundenlohn");
    return fehlt;
  }

  function maHinweisSetzen(el, p) {
    var fehlt = p.rolle === "mitarbeiterin" ? maFehlendeAngaben(p) : [];
    el.hidden = !fehlt.length;
    if (fehlt.length) {
      el.innerHTML = "Bei " + escapeHtml(p.anzeigename) + " fehlt noch: <b>" + escapeHtml(fehlt.join(", ")) +
        '</b> – <button type="button" class="link-knopf" data-ma-bearbeiten="' + p.id + '">jetzt ergänzen</button>';
      el.querySelector("[data-ma-bearbeiten]").addEventListener("click", function () { maDialogOeffnen(p); });
    }
  }

  // ---- Stunden ----
  // Bewusst getrennt von den Kunden (Stefans Wunsch). Erfassung wie in
  // Stefans bisheriger Excel-Vorlage: pro Tag zwei Zeitbloecke (Vormittag/
  // Nachmittag), die Stunden werden daraus berechnet statt eingetippt.
  // Papa/Mama erfassen fuer die oben gewaehlte Person.
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
        invoke("stunden_loeschen", { id: Number(knopf.dataset.id), benutzer_id: maPersonIdAktuell() })
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
    var person = maPersonAktuell();
    var eigene = person.id === aktuellerBenutzer.id;
    document.getElementById("stErfassenTitel").textContent = eigene ? "Stunden erfassen" : "Stunden erfassen für " + person.anzeigename;
    document.getElementById("stPersonText").textContent = eigene ? "Deine eigenen Stunden" : "Stunden von " + person.anzeigename;
    elStMonatTitel.textContent = MONATSNAMEN_LANG[stMonat - 1] + " " + stJahr;
    invoke("eigene_stunden", { benutzer_id: person.id, jahr: stJahr, monat: stMonat })
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
    invoke("stunden_erfassen", { benutzer_id: maPersonIdAktuell(), eingabe: eingabe })
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
  // Liest Stefans bestehende Stunden-Excel so, wie sie ist: sucht die
  // Kopfzeile ("Datum", "Beginn", "Ende", "Beginn", "Ende") selbst - egal
  // in welcher Spalte sie steht und auch wenn sie pro Monat neu vorkommt -
  // und nimmt die Spalten von dort. Ohne Kopfzeile gilt die feste
  // Reihenfolge ab der Datums-Spalte: Datum, Beginn/Ende Vormittag,
  // Beginn/Ende Nachmittag, Notiz.
  //
  // Excel-Eigenheit: eine als "hh:mm" formatierte Zelle mit einer ganzen
  // Zahl drin zeigt in Excel "00:00", kommt aber als "1900-01-12" an -
  // zaehlt deshalb nur die Uhrzeit, und ein Block 00:00-00:00 ist leer.
  var elStiDialog = document.getElementById("stundenImportDialog");
  var elStiFuerWenZeile = document.getElementById("sti-fuer-wen-zeile");
  var elStiFuerWen = document.getElementById("sti-fuer-wen");
  var elStiText = document.getElementById("sti-text");
  var elStiVorschau = document.getElementById("sti-vorschau");
  var elStiFehler = document.getElementById("sti-fehler");
  var elStiImportierenKnopf = document.getElementById("sti-importieren");
  var stiGueltigeEintraege = [];

  // Ein echtes Arbeitsdatum (nicht Excels 1900-Platzhalter) -> "JJJJ-MM-TT".
  function stiDatumNormalisieren(s) {
    s = String(s || "").trim();
    var iso = s.match(/^(\d{4})-(\d{2})-(\d{2})(?:[ T]\d{1,2}:\d{2}(?::\d{2})?)?$/);
    if (iso) return Number(iso[1]) >= 1990 ? iso[1] + "-" + iso[2] + "-" + iso[3] : "";
    var ch = s.match(/^(\d{1,2})\.(\d{1,2})\.(\d{2}|\d{4})$/);
    if (ch) {
      var jahr = ch[3].length === 2 ? "20" + ch[3] : ch[3];
      if (Number(jahr) < 1990 || Number(ch[2]) > 12 || Number(ch[1]) > 31) return "";
      return jahr + "-" + ch[2].padStart(2, "0") + "-" + ch[1].padStart(2, "0");
    }
    return "";
  }

  // Uhrzeit "HH:MM" aus "8:30", "08:30:00", "08.30", "1900-01-12 02:30"
  // oder einem Excel-Bruchteil wie "0.35416"; Excels Platzhalter-Datum
  // ohne Uhrzeit ("1900-01-12") = 00:00. Sonst "".
  function stiZeitNormalisieren(s) {
    s = String(s || "").trim();
    var m = s.match(/(?:^|[ T])(\d{1,2})[:.](\d{2})(?::\d{2})?$/);
    if (m && Number(m[1]) < 24 && Number(m[2]) < 60) return m[1].padStart(2, "0") + ":" + m[2];
    if (/^(18|19)\d{2}-\d{2}-\d{2}$/.test(s)) return "00:00";
    if (/^0?[.,]\d+$/.test(s)) {
      var minuten = Math.round(parseFloat(s.replace(",", ".")) * 1440);
      return String(Math.floor(minuten / 60)).padStart(2, "0") + ":" + String(minuten % 60).padStart(2, "0");
    }
    return "";
  }

  function stiMinuten(zeit) { var t = zeit.split(":"); return Number(t[0]) * 60 + Number(t[1]); }

  // Ein Zeitblock zaehlt nur, wenn Ende nach Beginn liegt; 00:00-00:00
  // (leer in Excel) ist einfach kein Block.
  function stiBlock(beginn, ende) {
    if (!beginn || !ende || stiMinuten(ende) <= stiMinuten(beginn)) return null;
    return [beginn, ende];
  }

  // Kopfzeile erkennen: eine Zelle "Datum" und mindestens ein Paar
  // Beginn/Ende (auch "von"/"bis") rechts davon.
  function stiKopfErkennen(spalten) {
    var normal = spalten.map(function (z) { return String(z || "").trim().toLowerCase(); });
    var datum = normal.indexOf("datum");
    if (datum < 0) return null;
    var beginn = [], ende = [], notiz = -1;
    normal.forEach(function (z, i) {
      if (i <= datum) return;
      if (/^(beginn|von|start|anfang)/.test(z)) beginn.push(i);
      else if (/^(ende|bis|schluss)/.test(z)) ende.push(i);
      else if (/^(notiz|bemerkung|kommentar)/.test(z)) notiz = i;
    });
    if (!beginn.length || !ende.length) return null;
    var vmE = ende.filter(function (i) { return i > beginn[0]; })[0];
    var nmB = beginn.filter(function (i) { return i > vmE; })[0];
    var nmE = nmB === undefined ? undefined : ende.filter(function (i) { return i > nmB; })[0];
    return { datum: datum, vmB: beginn[0], vmE: vmE, nmB: nmB, nmE: nmE, notiz: notiz };
  }

  // Nutzt dieselben Zerlege-Hilfsfunktionen wie "Kunden importieren"
  // (kiZeilenAufteilen/kiTrennzeichenErkennen/kiZeileSpalten).
  // Liefert { eintraege, ohneZeit }: Tage mit mindestens einem Zeitblock
  // bzw. Tage mit Datum, aber ohne Zeiten (frei, Ferien, ...). Zeilen ganz
  // ohne Datum (Kopf, Totale, Unterschriften) werden still uebergangen.
  function stiEintraegeAnalysieren(text) {
    var ergebnis = { eintraege: [], ohneZeit: [] };
    var zeilen = kiZeilenAufteilen(text);
    if (!zeilen.length) return ergebnis;
    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    var kopf = null;
    zeilen.forEach(function (zeile) {
      var spalten = kiZeileSpalten(zeile, trenner);
      var neuerKopf = stiKopfErkennen(spalten);
      if (neuerKopf) { kopf = neuerKopf; return; }

      // Ohne Kopfzeile: erste Zelle mit einem echten Datum, die Zeiten
      // folgen direkt rechts davon.
      var spalte = kopf;
      if (!spalte) {
        var d = spalten.findIndex(function (z) { return !!stiDatumNormalisieren(z); });
        if (d < 0) return;
        spalte = { datum: d, vmB: d + 1, vmE: d + 2, nmB: d + 3, nmE: d + 4, notiz: d + 5 };
      }
      var datum = stiDatumNormalisieren(spalten[spalte.datum]);
      if (!datum) return;
      var tag = stiTagBauen(datum, spalte, function (i) { return i === undefined || i < 0 ? "" : spalten[i]; });
      (tag.vm_beginn || tag.nm_beginn ? ergebnis.eintraege : ergebnis.ohneZeit).push(tag);
    });
    return ergebnis;
  }

  function stiTagBauen(datum, spalte, zelle) {
    var vm = stiBlock(stiZeitNormalisieren(zelle(spalte.vmB)), stiZeitNormalisieren(zelle(spalte.vmE)));
    var nm = stiBlock(stiZeitNormalisieren(zelle(spalte.nmB)), stiZeitNormalisieren(zelle(spalte.nmE)));
    // Nur ein Block, und der liegt am Nachmittag -> als Nachmittag fuehren.
    if (vm && !nm && stiMinuten(vm[0]) >= 12 * 60) { nm = vm; vm = null; }
    var notiz = String(zelle(spalte.notiz) || "").trim();
    return {
      datum: datum,
      vm_beginn: vm ? vm[0] : "", vm_ende: vm ? vm[1] : "",
      nm_beginn: nm ? nm[0] : "", nm_ende: nm ? nm[1] : "",
      notiz: stiZeitNormalisieren(notiz) || stiDatumNormalisieren(notiz) ? "" : notiz,
    };
  }

  function stiStunden(e) {
    var min = 0;
    if (e.vm_beginn) min += stiMinuten(e.vm_ende) - stiMinuten(e.vm_beginn);
    if (e.nm_beginn) min += stiMinuten(e.nm_ende) - stiMinuten(e.nm_beginn);
    return min / 60;
  }

  // Vorschau pro Monat (Tage und Stunden) - zum Vergleichen mit den
  // Monatstotalen in der eigenen Excel, statt hunderte Zeilen zu zeigen.
  function stiVorschauZeichnen() {
    var analyse = stiEintraegeAnalysieren(elStiText.value);
    stiGueltigeEintraege = analyse.eintraege;
    elStiImportierenKnopf.disabled = stiGueltigeEintraege.length === 0;
    if (!stiGueltigeEintraege.length && !analyse.ohneZeit.length) {
      elStiVorschau.innerHTML = elStiText.value.trim()
        ? '<div class="import-zusammenfassung">Keine Zeile mit Datum gefunden – ist es die richtige Datei?</div>'
        : "";
      return;
    }

    var monate = {};
    stiGueltigeEintraege.forEach(function (e) {
      var m = e.datum.slice(0, 7);
      monate[m] = monate[m] || { tage: 0, stunden: 0 };
      monate[m].tage += 1;
      monate[m].stunden += stiStunden(e);
    });
    var schluessel = Object.keys(monate).sort();
    var total = schluessel.reduce(function (s, m) { return s + monate[m].stunden; }, 0);
    function monatName(m) { return MONATSNAMEN_LANG[Number(m.slice(5, 7)) - 1] + " " + m.slice(0, 4); }
    function std(x) { return (Math.round(x * 100) / 100).toLocaleString("de-CH"); }

    elStiVorschau.innerHTML =
      '<div class="import-zusammenfassung">' +
      (stiGueltigeEintraege.length
        ? "<b>" + stiGueltigeEintraege.length + (stiGueltigeEintraege.length === 1 ? " Arbeitstag" : " Arbeitstage") + "</b> mit zusammen <b>" + std(total) + " Stunden</b> erkannt" +
          (schluessel.length > 1 ? " (" + monatName(schluessel[0]) + " bis " + monatName(schluessel[schluessel.length - 1]) + ")" : "")
        : "Keine Arbeitszeiten erkannt") +
      (analyse.ohneZeit.length ? " · " + analyse.ohneZeit.length + (analyse.ohneZeit.length === 1 ? " Tag" : " Tage") + " ohne Zeiten wird übersprungen".replace("wird", analyse.ohneZeit.length === 1 ? "wird" : "werden") : "") +
      ". Schon vorhandene Tage werden nicht doppelt eingetragen.</div>" +
      (schluessel.length
        ? '<div class="tabellenrahmen" style="max-height:260px;overflow:auto"><table class="auflistung"><thead><tr>' +
          '<th>Monat</th><th class="re">Tage</th><th class="re">Stunden</th></tr></thead><tbody>' +
          schluessel.map(function (m) {
            return "<tr><td>" + monatName(m) + '</td><td class="re">' + monate[m].tage + '</td><td class="re">' + std(monate[m].stunden) + "</td></tr>";
          }).join("") +
          '</tbody><tfoot><tr><td><b>Total</b></td><td class="re"><b>' + stiGueltigeEintraege.length + '</b></td><td class="re"><b>' + std(total) + "</b></td></tr></tfoot></table></div>"
        : "");
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
              var ausgewaehlt = b.id === maPersonIdAktuell() ? " selected" : "";
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
      function (meldung) { elStiFehler.textContent = meldung; elStiFehler.hidden = false; },
      true
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
      .then(function (ergebnis) {
        elStiDialog.close();
        if (fuerWenId === maPersonIdAktuell()) stMonatLaden();
        alert(ergebnis.neu + (ergebnis.neu === 1 ? " Tag wurde importiert." : " Tage wurden importiert.") +
          (ergebnis.doppelt ? "\n" + ergebnis.doppelt + " waren schon vorhanden und wurden übersprungen." : ""));
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

  // ---- Personalien: Mitarbeiterin anlegen / bearbeiten ----
  // Bewusst ohne Login-Option: eine Mitarbeiterin bekommt hier nur ein
  // Lohnprofil, keinen Zugang zum Programm - Stefan traegt ihre Stunden
  // selbst ein. Derselbe Dialog fuer beides, genau wie bei "Kunde
  // bearbeiten" - "maBearbeitenId" entscheidet, ob neu oder aktualisiert.
  var elMaDialog = document.getElementById("mitarbeiterinDialog");
  var elMaFehler = document.getElementById("ma-fehler");
  var elMaErfolg = document.getElementById("ma-erfolg");
  var elMaKnopf = document.getElementById("ma-anlegen");
  var elMaListe = document.getElementById("maListe");
  var maBearbeitenId = null;

  function maListeZeichnen() {
    var ma = maMitarbeiterinnen();
    if (!ma.length) {
      elMaListe.innerHTML = '<p class="leer">Noch keine Mitarbeiterin angelegt.</p>';
      return;
    }
    elMaListe.innerHTML =
      '<div class="tabellenrahmen" style="margin-bottom:12px"><table class="auflistung"><thead><tr>' +
      '<th>Name</th><th>Geburtsdatum</th><th>AHV-Nummer</th><th>Eintritt</th><th class="re">Stundenlohn</th><th></th>' +
      "</tr></thead><tbody>" +
      ma.map(function (b) {
        var name = escapeHtml(b.anzeigename) +
          (b.austritt ? ' <span class="status st-abgeholt">ausgetreten ' + datumKurz(b.austritt) + "</span>" : "");
        var fehlt = '<span style="color:var(--tinte-3)">–</span>';
        return '<tr' + (b.id === maPersonId ? ' class="ma-gewaehlt"' : "") + "><td>" +
          '<button type="button" class="link-knopf" data-ma-waehlen="' + b.id + '">' + name + "</button>" +
          (b.strasse || b.plz_ort ? '<br><small style="color:var(--tinte-2)">' + escapeHtml([b.strasse, b.plz_ort].filter(Boolean).join(", ")) + "</small>" : "") +
          "</td><td>" + (b.geburtsdatum ? datumVoll(b.geburtsdatum) : fehlt) +
          "</td><td>" + (b.ahv_nummer ? escapeHtml(b.ahv_nummer) : fehlt) +
          "</td><td>" + (b.eintritt ? datumKurz(b.eintritt) : fehlt) +
          '</td><td class="re">' + (b.stundenlohn ? chf(b.stundenlohn) : fehlt) +
          '</td><td><button type="button" class="knopf" data-id="' + b.id + '" style="font-size:12px;padding:3px 10px">Bearbeiten</button></td></tr>';
      }).join("") +
      "</tbody></table></div>";
    elMaListe.querySelectorAll("button[data-id]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        var b = ma.filter(function (m) { return m.id === Number(btn.dataset.id); })[0];
        if (b) maDialogOeffnen(b);
      });
    });
    // Name anklicken = diese Person oben auswaehlen und ihre Stunden zeigen.
    elMaListe.querySelectorAll("button[data-ma-waehlen]").forEach(function (btn) {
      btn.addEventListener("click", function () {
        maPersonId = Number(btn.dataset.maWaehlen);
        maPersonAuswahlZeichnen();
        maFormulareZaehlen();
        maAnsichtZeigen("stunden");
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
    document.getElementById("ma-geburtsdatum").value = bestehende ? bestehende.geburtsdatum || "" : "";
    document.getElementById("ma-eintritt").value = bestehende ? bestehende.eintritt || "" : "";
    document.getElementById("ma-austritt").value = bestehende ? bestehende.austritt || "" : "";
    elMaFehler.hidden = true;
    elMaErfolg.hidden = true;
    elMaDialog.showModal();
    document.getElementById("ma-anzeigename").focus();
  }

  document.getElementById("mitarbeiterinAnlegenKnopf").addEventListener("click", function () { maDialogOeffnen(null); });
  document.getElementById("ma-abbrechen").addEventListener("click", function () { elMaDialog.close(); maLaden(); });

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
      geburtsdatum: document.getElementById("ma-geburtsdatum").value,
      eintritt: document.getElementById("ma-eintritt").value,
      austritt: document.getElementById("ma-austritt").value,
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
          geburtsdatum: eingabe.geburtsdatum,
          eintritt: eingabe.eintritt,
          austritt: eingabe.austritt,
        })
      : invoke("mitarbeiterin_anlegen", { eingabe: eingabe });

    knopfSperren(elMaKnopf, true);
    aufruf
      .then(function (person) {
        elMaErfolg.textContent = maBearbeitenId
          ? "Gespeichert."
          : "Angelegt – oben bei „Person“ ist sie jetzt ausgewählt.";
        elMaErfolg.hidden = false;
        if (!maBearbeitenId) {
          // Ab jetzt "bearbeiten" - ein zweiter Klick legt sie nicht doppelt an.
          maPersonId = person.id;
          maBearbeitenId = person.id;
          elMaKnopf.textContent = "Speichern";
          document.getElementById("ma-titel").textContent = "Mitarbeiterin bearbeiten";
        }
        maLaden();
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

  // ---- Lohn ----
  // Pro Monat mit Stunden eine Zeile (Knopf "Abrechnung" = die bisherige
  // monatliche Lohnabrechnung als Excel), darunter die Jahreszahlen fuer
  // Lohnausweis und AHV-Lohnbescheinigung (mitarbeiter.rs).
  var maLohnJahr = new Date().getFullYear();
  var elMaLohnInhalt = document.getElementById("maLohnInhalt");
  var elThLohnFehler = document.getElementById("th-lohn-fehler");
  var elThLohnEcho = document.getElementById("thLohnEcho");

  function maLohnLaden() {
    var p = maPersonAktuell();
    document.getElementById("maLohnTitel").textContent = "Lohn " + maLohnJahr + " – " + p.anzeigename;
    elThLohnFehler.hidden = true;
    elThLohnEcho.textContent = "";
    maHinweisSetzen(document.getElementById("maLohnHinweis"), p);
    var exportKnopf = document.getElementById("maJahrExportKnopf");
    exportKnopf.hidden = true;
    if (p.rolle !== "mitarbeiterin") {
      elMaLohnInhalt.innerHTML = '<p class="leer">Lohn wird nur für Mitarbeiterinnen berechnet – bitte oben eine Mitarbeiterin wählen.</p>';
      return;
    }
    elMaLohnInhalt.innerHTML = '<p class="leer">Lade …</p>';
    invoke("lohn_jahresuebersicht", { benutzer_id: p.id, jahr: maLohnJahr })
      .then(function (j) {
        if (!j.monate.length) {
          elMaLohnInhalt.innerHTML = '<p class="leer">Keine Stunden im Jahr ' + maLohnJahr + " erfasst.</p>";
          return;
        }
        exportKnopf.hidden = false;
        var t = j.total;
        elMaLohnInhalt.innerHTML =
          '<div class="tabellenrahmen"><table class="auflistung lohn-tabelle"><thead><tr>' +
          '<th>Monat</th><th class="re">Stunden</th><th class="re">Bruttolohn</th><th class="re">AHV/IV/EO</th>' +
          '<th class="re">ALV</th><th class="re">Nettolohn</th><th></th></tr></thead><tbody>' +
          j.monate.map(function (m) {
            return "<tr><td>" + MONATSNAMEN_LANG[m.monat - 1] + '</td><td class="re">' + m.stunden.toLocaleString("de-CH") +
              '</td><td class="re">' + chf(m.bruttolohn) + '</td><td class="re">' + chf(m.ahv) + '</td><td class="re">' + chf(m.alv) +
              '</td><td class="re"><b>' + chf(m.nettolohn) + "</b></td>" +
              '<td class="re"><button type="button" class="knopf" data-lohn-monat="' + m.monat + '" style="font-size:12px;padding:3px 10px">Abrechnung</button></td></tr>';
          }).join("") +
          '</tbody><tfoot><tr><td>Total</td><td class="re">' + t.stunden.toLocaleString("de-CH") +
          '</td><td class="re">' + chf(t.bruttolohn) + '</td><td class="re">' + chf(t.ahv) + '</td><td class="re">' + chf(t.alv) +
          '</td><td class="re">' + chf(t.nettolohn) + "</td><td></td></tr></tfoot></table></div>" +
          '<div class="lohnausweis-kasten">' +
          "<h3>Für den Lohnausweis " + j.jahr + " (Formular 11)</h3>" +
          "<dl>" +
          "<dt>Zeitraum</dt><dd>" + datumVoll(j.von) + " – " + datumVoll(j.bis) + "</dd>" +
          "<dt>Ziffer 1 · Lohn</dt><dd>" + chf(t.bruttolohn) + "</dd>" +
          "<dt>Ziffer 8 · Bruttolohn total</dt><dd>" + chf(t.bruttolohn) + "</dd>" +
          "<dt>Ziffer 9 · Beiträge AHV/IV/EO/ALV</dt><dd>" + chf(t.total_abzuege) + "</dd>" +
          "<dt>Ziffer 11 · Nettolohn</dt><dd><b>" + chf(t.nettolohn) + "</b></dd>" +
          "</dl>" +
          "<h3>Für die AHV-Lohnbescheinigung " + j.jahr + "</h3>" +
          "<dl><dt>AHV-pflichtiger Lohn</dt><dd>" + chf(t.bruttolohn) + "</dd></dl>" +
          '<p class="login-hinweis" style="margin:8px 0 0">Gerechnet mit dem heutigen Stundenlohn (' + chf(j.stundenlohn) +
          ") und den heutigen Prozentsätzen.</p></div>";
        elMaLohnInhalt.querySelectorAll("button[data-lohn-monat]").forEach(function (btn) {
          btn.addEventListener("click", function () {
            var monat = Number(btn.dataset.lohnMonat);
            elThLohnFehler.hidden = true;
            elThLohnEcho.style.color = "var(--gruen)";
            elThLohnEcho.textContent = "Exportiere …";
            invoke("lohnabrechnung_exportieren", { benutzer_id: p.id, jahr: maLohnJahr, monat: monat })
              .then(function (pfad) { elThLohnEcho.textContent = "Lohnabrechnung " + MONATSNAMEN_LANG[monat - 1] + " exportiert nach: " + pfad; })
              .catch(function (e) { elThLohnEcho.textContent = ""; elThLohnFehler.textContent = fehlerText(e); elThLohnFehler.hidden = false; });
          });
        });
      })
      .catch(function (e) { elMaLohnInhalt.innerHTML = '<p class="leer">' + escapeHtml(fehlerText(e)) + "</p>"; });
  }

  document.getElementById("maLohnVorKnopf").addEventListener("click", function () { maLohnJahr -= 1; maLohnLaden(); });
  document.getElementById("maLohnNachKnopf").addEventListener("click", function () { maLohnJahr += 1; maLohnLaden(); });
  document.getElementById("maLohnHeuteKnopf").addEventListener("click", function () { maLohnJahr = new Date().getFullYear(); maLohnLaden(); });
  document.getElementById("maJahrExportKnopf").addEventListener("click", function () {
    elThLohnFehler.hidden = true;
    elThLohnEcho.style.color = "var(--gruen)";
    elThLohnEcho.textContent = "Exportiere …";
    invoke("lohn_jahresuebersicht_exportieren", { benutzer_id: maPersonIdAktuell(), jahr: maLohnJahr })
      .then(function (pfad) { elThLohnEcho.textContent = "Exportiert nach: " + pfad; })
      .catch(function (e) { elThLohnEcho.textContent = ""; elThLohnFehler.textContent = fehlerText(e); elThLohnFehler.hidden = false; });
  });

  // ---- Formulare ----
  // Was ein Arbeitgeber in der Schweiz fuer eine Mitarbeiterin anmelden und
  // melden muss, als Checkliste. Abhaken speichert das Datum (mitarbeiter.rs);
  // jaehrliche Meldungen gelten pro Jahr ("lohnausweis:2026").
  var MA_FORMULARE = {
    eintritt: [
      ["arbeitsvertrag", "Arbeitsvertrag", "Schriftlich mit Stundenlohn, Ferienzuschlag, ungefährem Pensum und Kündigungsfrist – je ein Exemplar für beide."],
      ["ahv_anmeldung", "Anmeldung bei der AHV-Ausgleichskasse", "Innert einem Monat nach Stellenantritt, mit AHV-Nummer und Geburtsdatum – bei der Ausgleichskasse, der das Geschäft angeschlossen ist."],
      ["uvg", "Unfallversicherung (UVG)", "Pflicht ab der ersten Mitarbeiterin – der Versicherung melden. Nichtberufsunfall ist nur mitversichert, wenn sie mindestens 8 Stunden pro Woche arbeitet."],
      ["familienzulagen", "Familienzulagen", "Nur wenn sie Kinder hat: Antrag über die Familienausgleichskasse (meist dieselbe Stelle wie die AHV). Betrifft es sie nicht, einfach abhaken."],
      ["quellensteuer", "Quellensteuer", "Nur bei ausländischen Mitarbeitenden ohne Niederlassungsbewilligung C: beim kantonalen Steueramt anmelden und die Steuer vom Lohn abziehen. Sonst abhaken."],
      ["bvg", "Pensionskasse (BVG) geprüft", "Nur nötig, wenn ihr Jahreslohn über der BVG-Eintrittsschwelle liegt (2025: CHF 22'680) – der Jahreslohn steht unter „Lohn“."],
    ],
    jaehrlich: [
      ["lohnausweis", "Lohnausweis (Formular 11) abgegeben", "Bis Ende Januar für das Vorjahr. Die Zahlen (Ziffern 1, 8, 9, 11) stehen unter „Lohn“."],
      ["ahv_lohnbescheinigung", "AHV-Lohnbescheinigung eingereicht", "Bis 30. Januar an die Ausgleichskasse: Jahreslohn pro Person (= „AHV-pflichtiger Lohn“ unter „Lohn“)."],
      ["uvg_lohnmeldung", "Lohnsumme der Unfallversicherung gemeldet", "Anfang Jahr: die Lohnsumme des Vorjahres an die UVG-Versicherung."],
    ],
    austritt: [
      ["austritt_lohnausweis", "Lohnausweis mit dem letzten Lohn abgegeben", "Bei Austritt unter dem Jahr gleich mit der letzten Lohnzahlung (Zeitraum bis zum Austritt)."],
      ["austritt_arbeitszeugnis", "Arbeitszeugnis ausgestellt", "Sie hat Anspruch darauf – auf Wunsch nur eine Arbeitsbestätigung (Dauer und Tätigkeit)."],
      ["austritt_meldung", "Austritt gemeldet", "Der Ausgleichskasse (wegen Familienzulagen) und – falls verlangt – der Unfallversicherung."],
    ],
  };
  // Im Januar bis Maerz geht es meist um die Meldungen fuers Vorjahr.
  var maFormJahr = new Date().getMonth() < 3 ? new Date().getFullYear() - 1 : new Date().getFullYear();

  function maFormPunktHtml(eintrag, schluessel, erledigt) {
    var am = erledigt[schluessel];
    return '<label class="form-punkt' + (am ? " erledigt" : "") + '">' +
      '<input type="checkbox" data-formular="' + schluessel + '"' + (am ? " checked" : "") + ">" +
      "<span><b>" + escapeHtml(eintrag[1]) + "</b><small>" + escapeHtml(eintrag[2]) + "</small></span>" +
      (am ? "<em>erledigt " + datumKurz(am) + "</em>" : "<em></em>") + "</label>";
  }

  function maFormLaden() {
    var p = maPersonAktuell();
    var elListe = document.getElementById("maFormListe");
    document.getElementById("maFormTitel").textContent = "Formulare – " + p.anzeigename;
    maHinweisSetzen(document.getElementById("maFormHinweis"), p);
    if (p.rolle !== "mitarbeiterin") {
      elListe.innerHTML = '<p class="leer">Formulare gibt es nur für Mitarbeiterinnen – bitte oben eine Mitarbeiterin wählen.</p>';
      return;
    }
    invoke("formulare_lesen", { benutzer_id: p.id })
      .then(function (liste) {
        var erledigt = {};
        liste.forEach(function (f) { erledigt[f.formular] = f.erledigt_am; });
        var jahrOptionen = [];
        for (var j = new Date().getFullYear(); j >= new Date().getFullYear() - 3; j--) {
          jahrOptionen.push('<option value="' + j + '"' + (j === maFormJahr ? " selected" : "") + ">" + j + "</option>");
        }
        elListe.innerHTML =
          '<h3 class="form-gruppe">Bei Eintritt <small>einmalig</small></h3>' +
          MA_FORMULARE.eintritt.map(function (e) { return maFormPunktHtml(e, e[0], erledigt); }).join("") +
          '<h3 class="form-gruppe">Jedes Jahr im Januar <small>für das Jahr <select id="maFormJahr">' + jahrOptionen.join("") + "</select></small></h3>" +
          MA_FORMULARE.jaehrlich.map(function (e) { return maFormPunktHtml(e, e[0] + ":" + maFormJahr, erledigt); }).join("") +
          (p.austritt
            ? '<h3 class="form-gruppe">Bei Austritt <small>' + datumKurz(p.austritt) + "</small></h3>" +
              MA_FORMULARE.austritt.map(function (e) { return maFormPunktHtml(e, e[0], erledigt); }).join("")
            : '<p class="login-hinweis" style="margin-top:16px">Bei einer Kündigung unter „Personalien“ den Austritt eintragen – dann erscheint hier, was dabei zu tun ist.</p>');
        document.getElementById("maFormJahr").addEventListener("change", function (ev) {
          maFormJahr = Number(ev.target.value);
          maFormLaden();
        });
        elListe.querySelectorAll("input[data-formular]").forEach(function (cb) {
          cb.addEventListener("change", function () {
            cb.disabled = true;
            invoke("formular_setzen", { benutzer_id: p.id, formular: cb.dataset.formular, erledigt: cb.checked })
              .then(function () { maFormLaden(); maFormulareZaehlen(); })
              .catch(function (e) { cb.checked = !cb.checked; cb.disabled = false; alert(fehlerText(e)); });
          });
        });
      })
      .catch(function (e) { elListe.innerHTML = '<p class="leer">' + escapeHtml(fehlerText(e)) + "</p>"; });
  }

  // Zahl am Unterreiter "Formulare": offene Punkte bei Eintritt, ab Januar
  // bis Maerz dazu die Jahresmeldungen fuers Vorjahr.
  function maFormulareZaehlen() {
    var el = document.getElementById("maFormulareOffen");
    var p = maPersonAktuell();
    if (!istInhaber() || p.rolle !== "mitarbeiterin") { el.hidden = true; return; }
    invoke("formulare_lesen", { benutzer_id: p.id })
      .then(function (liste) {
        var erledigt = {};
        liste.forEach(function (f) { erledigt[f.formular] = true; });
        var offen = MA_FORMULARE.eintritt.filter(function (e) { return !erledigt[e[0]]; }).length;
        var heute = new Date();
        if (heute.getMonth() < 3) {
          offen += MA_FORMULARE.jaehrlich.filter(function (e) { return !erledigt[e[0] + ":" + (heute.getFullYear() - 1)]; }).length;
        }
        if (p.austritt) offen += MA_FORMULARE.austritt.filter(function (e) { return !erledigt[e[0]]; }).length;
        el.textContent = offen;
        el.hidden = !offen;
        el.title = offen + " offen";
      })
      .catch(function () { el.hidden = true; });
  }

  // ================= TREUHAND =================
  // Geschaeftsausgaben erfassen (feste Kategorie-Liste, siehe treuhand.rs)
  // und daraus den jaehrlichen Einnahmen/Ausgaben-Bericht fuer die Treuhand
  // exportieren - ersetzt Stefans bisherige "Treuhand - Umsatz"-Excel.
  // (Die Lohnabrechnung ist in den Reiter "Mitarbeiter" umgezogen.)
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
    thUebersichtLaden();
    thEinnahmenLaden();
  }

  // Dieselben Zahlen wie im Treuhand-Bericht - zum Vergleichen mit der
  // eigenen Excel, bevor der Bericht an die Treuhand geht.
  function thUebersichtLaden() {
    var el = document.getElementById("thUebersicht");
    invoke("treuhand_uebersicht", { jahr: thJahr })
      .then(function (u) {
        var kategorien = u.ausgaben_nach_kategorie.filter(function (k) { return k[1] > 0; });
        el.innerHTML =
          '<div class="th-uebersicht">' +
          '<dl class="th-zahlen">' +
          "<dt>Einnahmen aus Aufträgen</dt><dd>" + chf(u.einnahmen_auftraege) + "</dd>" +
          (u.einnahmen_excel ? "<dt>Einnahmen aus Excel übernommen</dt><dd>" + chf(u.einnahmen_excel) + "</dd>" : "") +
          (u.einnahmen_alte_rechnungen
            ? '<dt title="Rechnungen aus den alten Kundenordnern – nur Monate, für die keine Zahlen aus der Treuhand-Excel da sind">Einnahmen aus Kundenordnern</dt><dd>' +
              chf(u.einnahmen_alte_rechnungen) + "</dd>"
            : "") +
          '<dt class="th-total">Total Einnahmen</dt><dd class="th-total">' + chf(u.einnahmen) + "</dd>" +
          '<dt class="th-total">Total Ausgaben</dt><dd class="th-total">' + chf(u.ausgaben_gesamt) + "</dd>" +
          '<dt class="th-netto">Netto</dt><dd class="th-netto">' + chf(u.netto) + "</dd>" +
          "</dl>" +
          (kategorien.length
            ? '<dl class="th-zahlen th-kategorien">' + kategorien.map(function (k) {
                return "<dt>" + escapeHtml(k[0]) + "</dt><dd>" + chf(k[1]) + "</dd>";
              }).join("") + "</dl>"
            : "") +
          "</div>";
      })
      .catch(function (e) { el.innerHTML = '<p class="leer">' + escapeHtml(fehlerText(e)) + "</p>"; });
  }

  function thEinnahmenLaden() {
    invoke("einnahmen_extern_eines_jahres", { jahr: thJahr })
      .then(function (liste) {
        document.getElementById("thEinnahmenAbschnitt").hidden = !liste.length;
        var el = document.getElementById("thEinnahmenListe");
        el.innerHTML = liste.map(function (e) {
          return "<tr><td>" + datumKurz(e.datum) + '</td><td class="re">' + chf(e.betrag) + "</td><td>" +
            escapeHtml(e.zahlart && e.zahlart !== e.notiz ? e.zahlart + " · " + e.notiz : e.notiz) +
            '</td><td><button type="button" class="weg" data-einnahme-id="' + e.id + '" title="Einnahme löschen">×</button></td></tr>';
        }).join("");
        el.querySelectorAll("button[data-einnahme-id]").forEach(function (btn) {
          btn.addEventListener("click", function () {
            invoke("einnahme_extern_loeschen", { id: Number(btn.dataset.einnahmeId) })
              .then(thJahrLaden)
              .catch(function (e) { alert(fehlerText(e)); });
          });
        });
      })
      .catch(function () {});
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

  // ================= TREUHAND AUS EXCEL IMPORTIEREN =================
  // Fuer Stefans laufendes Jahr, das noch in Excel steht. Versteht zwei
  // Aufbauten, je ab einer Kopfzeile irgendwo in der Tabelle (auch pro
  // Monat/Tabellenblatt neu):
  // - Liste: Datum | Kategorie | Betrag | Notiz
  // - Tabelle mit einer Spalte pro Kategorie (Telefon, Werbung, ...), dazu
  //   evtl. Einnahmen-Spalten (Einnahmen, Umsatz, Bar, Karte, Twint)
  // Total-/Summenzeilen werden uebersprungen (sonst zaehlte alles doppelt),
  // unbekannte Kategorien kann man in der Vorschau von Hand zuordnen. Die
  // Kontrollsummen pro Kategorie sind zum Vergleich mit der Excel.
  var elThiDialog = document.getElementById("ausgabenImportDialog");
  var elThiText = document.getElementById("thi-text");
  var elThiVorschau = document.getElementById("thi-vorschau");
  var elThiFehler = document.getElementById("thi-fehler");
  var elThiImportierenKnopf = document.getElementById("thi-importieren");
  var thiErgebnis = { ausgaben: [], einnahmen: [] };
  var thiKategorienGeladen = [];
  var thiVonHand = {}; // normalisierte Excel-Bezeichnung -> Kategorie | "__einnahme" | "" (ueberspringen)
  var thiMonatsUmsatz = {}; // "JJJJ-MM" -> Umsatz aus Auftraegen im Programm

  var THI_EINNAHME = /^(einnahm|umsatz|ertrag|erls|erloes|kundenrechnung|bar$|karte$|kartenzahlung|twint$|rechnung$|rechnungen$)/;
  var THI_TOTAL = /^(total|summe|gesamt|zwischentotal|bertrag|uebertrag|saldo)/;
  var MONATE_ERKENNEN = ["jan", "feb", "mar", "apr", "mai", "jun", "jul", "aug", "sep", "okt", "nov", "dez"];

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

  // Monat aus einer Zelle oder einem Blattnamen: "Juni", "Jun 26",
  // "Sept. 2026", "Juni A" -> { monat: 1-12, jahr: Zahl oder null }.
  function thiMonatLesen(text) {
    var roh = String(text || "").trim().toLowerCase().replace("ä", "a");
    var m = roh.match(/^([a-z]{3})[a-z]*\.?(?:\s*(\d{4}|\d{2}))?(?:\s*a)?\s*\d?$/);
    if (!m) return null;
    var monat = MONATE_ERKENNEN.indexOf(m[1]);
    if (monat < 0) return null;
    var jahr = m[2] ? Number(m[2].length === 2 ? "20" + m[2] : m[2]) : null;
    return { monat: monat + 1, jahr: jahr };
  }

  function thiMonatsende(jahr, monat) {
    return jahr + "-" + String(monat).padStart(2, "0") + "-" + new Date(jahr, monat, 0).getDate();
  }

  // Datum aus einer Zelle: echtes Datum, sonst ein Monatsname ("Januar",
  // "Feb. 2026") -> Letzter des Monats (Jahr aus der Zelle, sonst das im
  // Reiter gewaehlte Jahr).
  function thiDatum(zelle) {
    var d = stiDatumNormalisieren(zelle);
    if (d) return d;
    var m = thiMonatLesen(zelle);
    return m ? thiMonatsende(m.jahr || thJahr, m.monat) : "";
  }

  function thiZiel(rohText) {
    var norm = kiTextNormalisieren(rohText);
    if (!norm) return null;
    if (Object.prototype.hasOwnProperty.call(thiVonHand, norm)) return thiVonHand[norm];
    if (THI_EINNAHME.test(norm)) return "__einnahme";
    return thiKategoriePassend(rohText);
  }

  function thiZahlart(rohText) {
    var n = kiTextNormalisieren(rohText);
    if (/^bar/.test(n)) return "Bar";
    if (/^karte|^kartenzahlung|^ec|^kredit/.test(n)) return "Karte";
    if (/^twint/.test(n)) return "Twint";
    if (/^rechnung/.test(n)) return "Rechnung";
    return "";
  }

  // Kopfzeile erkennen. Liste: Datum + Betrag (+ Kategorie). Breit:
  // mindestens zwei Spalten, die eine Kategorie oder Einnahmen sind - mit
  // Datumsspalte, oder ohne (dann gilt der Monat des Blatts, wie in Stefans
  // Monatsblaettern "Juni" / "Juni A"). Kommt dieselbe Ueberschrift zweimal
  // vor (z. B. "Bar" und rechts daneben die Zusammenfassung "bar"), zaehlt
  // nur die erste Spalte.
  function thiKopfErkennen(spalten) {
    var norm = spalten.map(kiTextNormalisieren);
    var datum = norm.findIndex(function (z) { return /^(datum|tag|monat|zeitraum)$/.test(z); });
    function finde(re) { return norm.findIndex(function (z, i) { return i !== datum && re.test(z); }); }
    var betrag = finde(/^(betrag|kosten|chf|preis|ausgabe$|ausgaben$|summe$)/);
    var kategorie = finde(/^(kategorie|art|konto|rubrik|bereich|grund)$/);
    var notiz = finde(/^(notiz|bemerkung|anmerkung|beschreibung|text|lieferant|was|beleg)/);
    var spaltenZiel = {};
    var gesehen = {};
    var treffer = 0;
    norm.forEach(function (z, i) {
      if (!z || i === datum || i === notiz || i === betrag || i === kategorie || THI_TOTAL.test(z)) return;
      if (gesehen[z]) return;
      gesehen[z] = true;
      var ziel = thiZiel(spalten[i]);
      spaltenZiel[i] = { roh: String(spalten[i]).trim(), ziel: ziel, zahlart: ziel === "__einnahme" ? thiZahlart(spalten[i]) : "" };
      if (ziel) treffer++;
    });
    // Ohne Datumsspalte nur eine reine Textzeile als Kopfzeile nehmen -
    // eine Datenzeile ("Büro", "Papier", 35.50) hat Zahlen oder ein Datum.
    var nurText = spalten.every(function (z) {
      var t = String(z || "").trim();
      return !t || (!(thiBetragNormalisieren(t) >= 0) && !stiDatumNormalisieren(t));
    });
    if (treffer >= 2 && (datum >= 0 || nurText)) return { art: "breit", datum: datum, notiz: notiz, spalten: spaltenZiel };
    if (datum >= 0 && betrag >= 0) return { art: "liste", datum: datum, betrag: betrag, kategorie: kategorie, notiz: notiz };
    return null;
  }

  // Zusammenfassungs-Blaetter nicht importieren - sonst zaehlte alles doppelt.
  var THI_BLATT_UEBERSPRINGEN = /^(jahr|treuhand|total|summe|zusammenfassung|ubersicht|uebersicht|abschluss)/;

  function thiAnalysieren(text) {
    var r = { ausgaben: [], einnahmen: [], unbekannt: {}, totalzeilen: 0, ohneBetrag: 0, art: "",
      blaetter: [], blaetterUebersprungen: [], ohneMonat: 0 };
    var zeilen = kiZeilenAufteilen(text);
    if (!zeilen.length) return r;
    var trenner = kiTrennzeichenErkennen(zeilen[0]);
    var kopf = null;
    var blatt = { name: "", ueberspringen: false, monat: null, jahr: null };
    var summen = {}, anzahl = {}, datenzeilen = 0, mitZaehler = 0;

    function kopfSetzen(k) {
      kopf = k; r.art = r.art || k.art;
      summen = {}; anzahl = {}; datenzeilen = 0; mitZaehler = 0;
    }

    function zuordnen(datum, rohKategorie, betrag, notiz, zahlart) {
      if (!(betrag > 0)) { r.ohneBetrag++; return; }
      var ziel = thiZiel(rohKategorie);
      if (ziel === "__einnahme") {
        r.einnahmen.push({ datum: datum, betrag: betrag, notiz: notiz || String(rohKategorie || "").trim(), zahlart: zahlart || thiZahlart(rohKategorie) });
      } else if (ziel) {
        r.ausgaben.push({ datum: datum, kategorie: ziel, betrag: betrag, notiz: notiz });
      } else if (ziel !== "") {
        var name = String(rohKategorie || "").trim() || "(ohne Kategorie)";
        var u = r.unbekannt[name] = r.unbekannt[name] || { anzahl: 0, summe: 0 };
        u.anzahl++; u.summe += betrag;
      }
    }

    zeilen.forEach(function (zeile) {
      var spalten = kiZeileSpalten(zeile, trenner);
      // Beginn eines neuen Tabellenblatts (Datei-Import)
      if (spalten[0] === "#BLATT") {
        var name = String(spalten[1] || "").trim();
        var m = thiMonatLesen(name);
        blatt = { name: name, ueberspringen: THI_BLATT_UEBERSPRINGEN.test(kiTextNormalisieren(name)),
          monat: m ? m.monat : null, jahr: m ? m.jahr : null };
        (blatt.ueberspringen ? r.blaetterUebersprungen : r.blaetter).push(name);
        kopf = null;
        return;
      }
      if (blatt.ueberspringen) return;
      var neu = thiKopfErkennen(spalten);
      if (neu) { kopfSetzen(neu); return; }
      if (spalten.some(function (z) { return THI_TOTAL.test(kiTextNormalisieren(z)); })) { r.totalzeilen++; return; }

      if (!kopf) {
        // Vor der Kopfzeile: Monat/Jahr des Blatts aus einer Datumszelle
        // ("Jun 26" oben im Blatt) - genauer als nur der Blattname.
        spalten.some(function (z) {
          var d = stiDatumNormalisieren(z), mm = d ? null : thiMonatLesen(z);
          if (d) { blatt.monat = Number(d.slice(5, 7)); blatt.jahr = Number(d.slice(0, 4)); return true; }
          if (mm && mm.jahr) { blatt.monat = mm.monat; blatt.jahr = mm.jahr; return true; }
          return false;
        });
      }

      if (kopf && kopf.art === "breit") {
        var datumB = kopf.datum >= 0 ? thiDatum(spalten[kopf.datum]) : "";
        if (!datumB && kopf.datum < 0 && blatt.monat) datumB = thiMonatsende(blatt.jahr || thJahr, blatt.monat);
        var werte = [];
        Object.keys(kopf.spalten).forEach(function (i) {
          var betrag = thiBetragNormalisieren(spalten[i]);
          if (betrag > 0) werte.push({ i: i, betrag: betrag });
        });
        if (!werte.length) return;
        // Totalzeile ohne Beschriftung (z. B. Zeile 35 im Monatsblatt): jeder
        // Betrag ist genau die Summe der Zeilen darueber.
        // Mehrere Betraege in einer Zeile, alle = Summe darueber: Totalzeile.
        // Ein einzelner Betrag nur, wenn er aus mindestens zwei Zeilen
        // zusammenkommt und die Zeile - anders als die Datenzeilen - keine
        // laufende Nummer/Notiz hat.
        var istSumme = function (w) { return anzahl[w.i] >= 1 && Math.abs(summen[w.i] - w.betrag) < 0.005; };
        var andereInhalte = spalten.some(function (z, i) { return String(z || "").trim() && !kopf.spalten[i] && i !== kopf.datum; });
        if (werte.every(istSumme) &&
            (werte.length >= 2 || (anzahl[werte[0].i] >= 2 && !andereInhalte && mitZaehler > datenzeilen / 2))) {
          r.totalzeilen++;
          return;
        }
        if (!datumB) { r.ohneMonat += werte.length; return; }
        datenzeilen++;
        if (andereInhalte) mitZaehler++;
        var notizB = kopf.notiz >= 0 ? String(spalten[kopf.notiz] || "").trim() : "";
        werte.forEach(function (w) {
          summen[w.i] = (summen[w.i] || 0) + w.betrag;
          anzahl[w.i] = (anzahl[w.i] || 0) + 1;
          zuordnen(datumB, kopf.spalten[w.i].roh, w.betrag, notizB, kopf.spalten[w.i].zahlart);
        });
        return;
      }
      var k = kopf;
      if (!k) {
        // Ohne Kopfzeile: Datum, Kategorie, Betrag, Notiz ab der ersten Datums-Zelle.
        var d = spalten.findIndex(function (z) { return !!stiDatumNormalisieren(z); });
        if (d < 0) return;
        k = { datum: d, kategorie: d + 1, betrag: d + 2, notiz: d + 3 };
        r.art = r.art || "fest";
      }
      var datum = thiDatum(spalten[k.datum]);
      if (!datum) return;
      zuordnen(datum, k.kategorie >= 0 ? spalten[k.kategorie] : "", thiBetragNormalisieren(spalten[k.betrag]),
        k.notiz >= 0 ? String(spalten[k.notiz] || "").trim() : "");
    });
    return r;
  }

  function thiVorschauZeichnen() {
    var r = thiAnalysieren(elThiText.value);
    thiErgebnis = r;
    elThiImportierenKnopf.disabled = !r.ausgaben.length && !r.einnahmen.length;
    var unbekannt = Object.keys(r.unbekannt);
    if (!elThiText.value.trim()) { elThiVorschau.innerHTML = ""; return; }
    if (!r.ausgaben.length && !r.einnahmen.length && !unbekannt.length) {
      elThiVorschau.innerHTML = '<div class="import-zusammenfassung">Keine Zeile mit Datum und Betrag gefunden – ist es die richtige Datei?</div>';
      return;
    }
    function summe(liste) { return liste.reduce(function (s, e) { return s + e.betrag; }, 0); }
    var jahre = {};
    r.ausgaben.concat(r.einnahmen).forEach(function (e) { jahre[e.datum.slice(0, 4)] = true; });

    // Kontrollsummen pro Kategorie
    var proKat = {};
    r.ausgaben.forEach(function (e) {
      var k = proKat[e.kategorie] = proKat[e.kategorie] || { anzahl: 0, summe: 0 };
      k.anzahl++; k.summe += e.betrag;
    });
    var katZeilen = thiKategorienGeladen.filter(function (k) { return proKat[k]; }).map(function (k) {
      return "<tr><td>" + escapeHtml(k) + '</td><td class="re">' + proKat[k].anzahl + '</td><td class="re">' + chf(proKat[k].summe) + "</td></tr>";
    }).join("");

    // Einnahmen pro Monat und Zahlart (wie Stefans "Jahr"-Blatt), mit
    // Warnung, wo im Programm schon Auftraege sind
    var proMonat = {}, proMonatArt = {};
    var ARTEN = ["Bar", "Karte", "Twint", "Rechnung", ""];
    r.einnahmen.forEach(function (e) {
      var m = e.datum.slice(0, 7);
      proMonat[m] = (proMonat[m] || 0) + e.betrag;
      proMonatArt[m] = proMonatArt[m] || {};
      proMonatArt[m][e.zahlart || ""] = (proMonatArt[m][e.zahlart || ""] || 0) + e.betrag;
    });
    var einMonate = Object.keys(proMonat).sort();
    var artenDa = ARTEN.filter(function (a) { return einMonate.some(function (m) { return proMonatArt[m][a]; }); });
    var proMonatAus = {};
    r.ausgaben.forEach(function (e) { var m = e.datum.slice(0, 7); proMonatAus[m] = (proMonatAus[m] || 0) + e.betrag; });
    var ausMonate = Object.keys(proMonatAus).sort();
    function monatText(m) { return MONATSNAMEN_LANG[Number(m.slice(5)) - 1] + " " + m.slice(0, 4); }
    var ueberschneidung = einMonate.filter(function (m) { return thiMonatsUmsatz[m] > 0; });

    var zuordnungHtml = unbekannt.length
      ? '<div class="thi-zuordnung"><b>Nicht erkannte Kategorien</b> – bitte zuordnen (sonst werden sie übersprungen):' +
        unbekannt.map(function (name) {
          var u = r.unbekannt[name];
          return '<label><span>' + escapeHtml(name) + ' <small>(' + u.anzahl + "×, " + chf(u.summe) + ")</small></span>" +
            '<select data-thi-roh="' + escapeHtml(name) + '"><option value="">– überspringen –</option>' +
            '<option value="__einnahme">Einnahme</option>' +
            thiKategorienGeladen.map(function (k) { return '<option value="' + escapeHtml(k) + '">' + escapeHtml(k) + "</option>"; }).join("") +
            "</select></label>";
        }).join("") + "</div>"
      : "";

    elThiVorschau.innerHTML =
      '<div class="import-zusammenfassung">' +
      "<b>" + r.ausgaben.length + (r.ausgaben.length === 1 ? " Ausgabe" : " Ausgaben") + "</b> (" + chf(summe(r.ausgaben)) + ") und <b>" +
      r.einnahmen.length + (r.einnahmen.length === 1 ? " Einnahme" : " Einnahmen") + "</b> (" +
      chf(summe(r.einnahmen)) + ") erkannt" +
      (Object.keys(jahre).length ? " – Jahr " + Object.keys(jahre).sort().join(", ") : "") +
      (r.totalzeilen ? " · " + r.totalzeilen + (r.totalzeilen === 1 ? " Total-Zeile" : " Total-Zeilen") + " übersprungen" : "") +
      ". Schon vorhandene Zeilen werden nicht doppelt eingetragen." +
      (r.blaetter.length ? "<br>Gelesene Blätter: " + escapeHtml(r.blaetter.join(", ")) : "") +
      (r.blaetterUebersprungen.length ? "<br>Übersprungen (Zusammenfassung, sonst doppelt): " + escapeHtml(r.blaetterUebersprungen.join(", ")) : "") +
      (r.ohneMonat ? '<br><b>' + r.ohneMonat + " Beträge ohne erkennbaren Monat</b> wurden übersprungen – Blattname oder Datum oben im Blatt fehlt." : "") +
      "</div>" +
      zuordnungHtml +
      (ueberschneidung.length
        ? '<p class="ma-hinweis">Achtung: Für ' + ueberschneidung.map(function (m) { return MONATSNAMEN_LANG[Number(m.slice(5)) - 1] + " " + m.slice(0, 4); }).join(", ") +
          " sind im Programm schon Aufträge erfasst – diese Einnahmen nur übernehmen, wenn sie nicht dieselben sind, sonst zählen sie doppelt.</p>"
        : "") +
      '<div class="thi-kontrolle">' +
      (katZeilen
        ? '<div class="tabellenrahmen"><table class="auflistung"><thead><tr><th>Ausgaben nach Kategorie</th><th class="re">Anzahl</th><th class="re">CHF</th></tr></thead><tbody>' +
          katZeilen + '</tbody><tfoot><tr><td><b>Total Ausgaben</b></td><td class="re"><b>' + r.ausgaben.length + '</b></td><td class="re"><b>' + chf(summe(r.ausgaben)) +
          "</b></td></tr></tfoot></table></div>"
        : "") +
      (ausMonate.length > 1
        ? '<div class="tabellenrahmen"><table class="auflistung"><thead><tr><th>Ausgaben nach Monat</th><th class="re">CHF</th></tr></thead><tbody>' +
          ausMonate.map(function (m) { return "<tr><td>" + monatText(m) + '</td><td class="re">' + chf(proMonatAus[m]) + "</td></tr>"; }).join("") +
          '</tbody><tfoot><tr><td><b>Total Ausgaben</b></td><td class="re"><b>' + chf(summe(r.ausgaben)) + "</b></td></tr></tfoot></table></div>"
        : "") +
      (einMonate.length
        ? '<div class="tabellenrahmen thi-breit"><table class="auflistung"><thead><tr><th>Einnahmen nach Monat</th>' +
          artenDa.map(function (a) { return '<th class="re">' + (a || "Andere") + "</th>"; }).join("") +
          '<th class="re">Total</th></tr></thead><tbody>' +
          einMonate.map(function (m) {
            return "<tr" + (thiMonatsUmsatz[m] > 0 ? ' class="thi-warn"' : "") + "><td>" + monatText(m) + "</td>" +
              artenDa.map(function (a) { return '<td class="re">' + chf(proMonatArt[m][a] || 0) + "</td>"; }).join("") +
              '<td class="re"><b>' + chf(proMonat[m]) + "</b></td></tr>";
          }).join("") +
          '</tbody><tfoot><tr><td><b>Total Einnahmen</b></td>' +
          artenDa.map(function (a) {
            return '<td class="re"><b>' + chf(einMonate.reduce(function (s2, m) { return s2 + (proMonatArt[m][a] || 0); }, 0)) + "</b></td>";
          }).join("") +
          '<td class="re"><b>' + chf(summe(r.einnahmen)) + "</b></td></tr></tfoot></table></div>"
        : "") +
      "</div>";

    elThiVorschau.querySelectorAll("select[data-thi-roh]").forEach(function (sel) {
      sel.addEventListener("change", function () {
        thiVonHand[kiTextNormalisieren(sel.dataset.thiRoh)] = sel.value;
        thiVorschauZeichnen();
      });
    });
  }

  // Umsatz der Auftraege pro Monat (fuer die Doppelt-Warnung) - fuer das
  // gewaehlte Jahr und das Vorjahr.
  function thiMonatsUmsatzLaden() {
    thiMonatsUmsatz = {};
    // Nur Auftraege aus dem Programm - Excel-Einnahmen und fruehere
    // Kundenrechnungen verdraengen sich ohnehin gegenseitig nicht doppelt.
    return Promise.all([thJahr, thJahr - 1].map(function (jahr) {
      return invoke("auftraege_monatsumsatz", { jahr: jahr }).then(function (zeilen) {
        zeilen.forEach(function (z) { thiMonatsUmsatz[jahr + "-" + String(z.monat).padStart(2, "0")] = z.summe; });
      }).catch(function () {});
    }));
  }


  document.getElementById("thImportKnopf").addEventListener("click", function () {
    elThiText.value = "";
    elThiVorschau.innerHTML = "";
    elThiFehler.hidden = true;
    elThiImportierenKnopf.disabled = true;
    thiErgebnis = { ausgaben: [], einnahmen: [] };
    thiVonHand = {};
    var weiter = function () { elThiDialog.showModal(); elThiText.focus(); };
    thiMonatsUmsatzLaden();
    if (thiKategorienGeladen.length) {
      weiter();
    } else {
      invoke("ausgaben_kategorien").then(function (k) { thiKategorienGeladen = k; weiter(); }).catch(weiter);
    }
  });
  document.getElementById("thi-abbrechen").addEventListener("click", function () { elThiDialog.close(); });
  elThiText.addEventListener("input", debounce(thiVorschauZeichnen, 150));
  document.getElementById("thi-datei").addEventListener("click", function () {
    elThiFehler.hidden = true;
    dateiFuerImportLesen(
      elThiText,
      thiVorschauZeichnen,
      function (meldung) { elThiFehler.textContent = meldung; elThiFehler.hidden = false; },
      true,
      true
    );
  });

  document.getElementById("thi-importieren").addEventListener("click", function () {
    var r = thiErgebnis;
    if (!r.ausgaben.length && !r.einnahmen.length) return;
    var ausgaben = r.ausgaben.map(function (e) {
      return { datum: e.datum, kategorie: e.kategorie, betrag: e.betrag, notiz: e.notiz, beleg_quelle: null };
    });
    elThiFehler.hidden = true;
    knopfSperren(elThiImportierenKnopf, true);
    var leer = Promise.resolve({ neu: 0, doppelt: 0 });
    var a = ausgaben.length ? invoke("ausgaben_importieren", { eingaben: ausgaben }) : leer;
    a.then(function (ra) {
      var b = r.einnahmen.length ? invoke("einnahmen_importieren", { eingaben: r.einnahmen }) : leer;
      return b.then(function (rb) {
        elThiDialog.close();
        thJahrLaden();
        var doppelt = ra.doppelt + rb.doppelt;
        alert("Importiert: " + ra.neu + " Ausgaben und " + rb.neu + " Einnahmen." +
          (doppelt ? "\n" + doppelt + " Zeilen waren schon vorhanden und wurden übersprungen." : ""));
      });
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
    document.getElementById("ei-geschaeft-email").value = aktuelleEinstellungen.geschaeft_email || "";
    belegFormularFuellen();
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
      ? "Eigenes Logo: " + aktuelleEinstellungen.quittung_logo_pfad.split(/[\\/]/).pop()
      : "Standard-Logo Nähservice Straub";
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

  // --- Schriftgroesse: "Gross" vergroessert Schrift und Knoepfe ueberall
  // (Geraete-Einstellung wie Hell/Dunkel, nicht in der Datenbank) ---
  var elEiSchrift = document.getElementById("eiSchrift");
  function schriftAnwenden(wert) {
    document.documentElement.classList.toggle("schrift-gross", wert === "gross");
    elEiSchrift.querySelectorAll("button").forEach(function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.wert === wert));
    });
  }
  (function () {
    var gespeichert;
    try { gespeichert = localStorage.getItem("schrift"); } catch (e) { gespeichert = null; }
    schriftAnwenden(gespeichert === "gross" ? "gross" : "normal");
  })();
  elEiSchrift.querySelectorAll("button").forEach(function (b) {
    b.addEventListener("click", function () {
      try { localStorage.setItem("schrift", b.dataset.wert); } catch (e) {}
      schriftAnwenden(b.dataset.wert);
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
      geschaeft_email: document.getElementById("ei-geschaeft-email").value.trim(),
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

  // --- Beleg-Design (nur Papa/Mama sichtbar) ---
  // Vorlage "Klassisch" = Layout der bisherigen Excel-Rechnung, "Schlicht"
  // = einfaches Layout ohne Linien. "Muster ansehen" erzeugt eine echte
  // PDF mit den Werten aus dem Formular, noch bevor gespeichert wird.
  function wahlSetzen(id, wert) {
    document.querySelectorAll("#" + id + " button").forEach(function (b) {
      b.setAttribute("aria-pressed", String(b.dataset.wert === wert));
    });
  }
  function wahlLesen(id) {
    var b = document.querySelector("#" + id + ' button[aria-pressed="true"]');
    return b ? b.dataset.wert : "";
  }
  ["eiBelegVorlage", "eiBelegFormat"].forEach(function (id) {
    document.querySelectorAll("#" + id + " button").forEach(function (b) {
      b.addEventListener("click", function () { wahlSetzen(id, b.dataset.wert); });
    });
  });

  function belegFormularFuellen() {
    wahlSetzen("eiBelegVorlage", aktuelleEinstellungen.beleg_vorlage || "klassisch");
    wahlSetzen("eiBelegFormat", aktuelleEinstellungen.beleg_format || "A5");
    document.getElementById("ei-beleg-farbe").value = (aktuelleEinstellungen.beleg_farbe || "#92D050").toLowerCase();
    document.getElementById("ei-beleg-titel-zusatz").value = aktuelleEinstellungen.beleg_titel_zusatz || "";
    document.getElementById("ei-beleg-dank").value = aktuelleEinstellungen.beleg_dank || "";
    document.getElementById("ei-beleg-zahlungshinweis").value = aktuelleEinstellungen.beleg_zahlungshinweis || "";
    document.getElementById("ei-beleg-logo").checked = aktuelleEinstellungen.beleg_logo_zeigen !== false;
  }

  function belegFormularWerte() {
    return Object.assign({}, aktuelleEinstellungen, {
      beleg_vorlage: wahlLesen("eiBelegVorlage"),
      beleg_format: wahlLesen("eiBelegFormat"),
      beleg_farbe: document.getElementById("ei-beleg-farbe").value.toUpperCase(),
      beleg_titel_zusatz: document.getElementById("ei-beleg-titel-zusatz").value.trim(),
      beleg_dank: document.getElementById("ei-beleg-dank").value.trim(),
      beleg_zahlungshinweis: document.getElementById("ei-beleg-zahlungshinweis").value.trim(),
      beleg_logo_zeigen: document.getElementById("ei-beleg-logo").checked,
    });
  }

  document.getElementById("eiBelegKnopf").addEventListener("click", function () {
    var elFehler = document.getElementById("ei-beleg-fehler");
    var elErfolg = document.getElementById("ei-beleg-erfolg");
    elFehler.hidden = true;
    elErfolg.hidden = true;
    var neu = belegFormularWerte();
    var knopf = document.getElementById("eiBelegKnopf");
    knopfSperren(knopf, true);
    invoke("einstellungen_speichern", { eingabe: neu })
      .then(function () {
        aktuelleEinstellungen = neu;
        elErfolg.textContent = "Gespeichert – gilt ab dem nächsten Beleg.";
        elErfolg.hidden = false;
      })
      .catch(function (e) { elFehler.textContent = fehlerText(e); elFehler.hidden = false; })
      .finally(function () { knopfSperren(knopf, false); });
  });

  document.getElementById("eiBelegMusterKnopf").addEventListener("click", function () {
    var elFehler = document.getElementById("ei-beleg-fehler");
    elFehler.hidden = true;
    var knopf = document.getElementById("eiBelegMusterKnopf");
    knopfSperren(knopf, true);
    invoke("beleg_muster_oeffnen", { eingabe: belegFormularWerte() })
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
        if (maAnsicht === "lohn") maLohnLaden();
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

  // Eine Zeile pro Auftrag: Kundin, Arbeit und Telefon (zum Anrufen, wenn
  // etwas fertig oder ueberfaellig ist), Status, Betrag und direkt
  // "Abrechnen". Ein Klick auf die Zeile oeffnet das Kundenblatt.
  function kurzlisteHtml(zeilen, leerText) {
    if (!zeilen.length) return '<p style="margin:0;font-size:14px;color:var(--tinte-2)">' + leerText + "</p>";
    return '<table class="verlauf kurzliste"><tbody>' + zeilen.map(function (z) {
      return '<tr class="klickzeile ' + streifenKlasse(z) + '" data-kunde="' + z.kunde_id + '">' +
        "<td><strong>" + escapeHtml(z.kunde_name) + '</strong> <span style="color:var(--tinte-3);font-size:12px">Kd. ' + z.kunde_nummer +
        " · Nr. " + z.rechnungsnummer + "</span><br>" +
        '<span style="font-size:12.5px;color:var(--tinte-2)">' + escapeHtml(z.arbeit) +
        (z.kunde_telefon ? ' · <span class="telefon">' + escapeHtml(z.kunde_telefon) + "</span>" : "") + "</span></td>" +
        '<td class="re">' + (z.abholdatum ? '<span style="font-size:12px;color:var(--tinte-3)">' + datumKurz(z.abholdatum) + "</span><br>" : "") +
        statusChip(z) + "</td>" +
        '<td class="re">' + chf(z.summe) + "</td>" +
        '<td class="re"><button type="button" class="knopf knopf-voll knopf-klein" data-abr-kunde="' + z.kunde_id +
        '" data-abr-id="' + z.id + '">Abrechnen</button></td></tr>';
    }).join("") + "</tbody></table>";
  }

  // Zahl der ueberfaelligen Auftraege in der Seitenleiste bei "Auftraege".
  function zaehlerSetzen(ueberfaellig) {
    var el = document.getElementById("zaehlerAuftraege");
    el.textContent = ueberfaellig;
    el.hidden = !ueberfaellig;
    el.title = ueberfaellig + (ueberfaellig === 1 ? " überfälliger Auftrag" : " überfällige Aufträge");
  }

  function zaehlerAktualisieren() {
    invoke("uebersicht").then(function (u) { zaehlerSetzen(u.ueberfaellig.length); }).catch(function () {});
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
        zaehlerSetzen(u.ueberfaellig.length);
        var istInhaber = !aktuellerBenutzer || aktuellerBenutzer.rolle !== "mitarbeiterin";
        function kachel(wert, text, filter, klasse) {
          return '<button type="button" class="kachel kachel-knopf ' + (klasse || "") + '" data-filter="' + filter + '">' +
            "<b>" + wert + "</b><span>" + text + "</span></button>";
        }
        document.getElementById("startKacheln").innerHTML =
          kachel(u.ueberfaellig.length, "überfällig", "laufend", u.ueberfaellig.length ? "kachel-warn" : "") +
          kachel(u.abholbereit_anzahl, "abholbereit", "abholbereit", u.abholbereit_anzahl ? "kachel-gut" : "") +
          kachel(u.laufend_anzahl, "laufende Aufträge", "laufend") +
          kachel(chf(u.unbezahlt_summe), u.unbezahlt_anzahl + (u.unbezahlt_anzahl === 1 ? " offener Posten (CHF)" : " offene Posten (CHF)"),
            "unbezahlt", u.unbezahlt_anzahl ? "kachel-warn" : "") +
          (istInhaber ? kachel(chf(u.umsatz_monat), "Umsatz " + MONATSNAMEN_LANG[h.getMonth()] + " (CHF)", "alle") : "");
        document.getElementById("startHeuteTitel").textContent = "Heute abholen" + (u.heute.length ? " · " + u.heute.length : "");
        document.getElementById("startUeberTitel").textContent = "Überfällig" + (u.ueberfaellig.length ? " · " + u.ueberfaellig.length : "");
        document.getElementById("startHeute").innerHTML = kurzlisteHtml(u.heute, "Heute ist nichts zum Abholen eingetragen.");
        document.getElementById("startUeberfaellig").innerHTML = kurzlisteHtml(u.ueberfaellig, "Nichts überfällig.");

        document.querySelectorAll("#startKacheln [data-filter]").forEach(function (b) {
          b.addEventListener("click", function () { auFilter = b.dataset.filter; reiterOeffnen("r-auftraege"); });
        });
        document.querySelectorAll("#t-start .klickzeile").forEach(function (tr) {
          tr.addEventListener("click", function () { kundeOeffnen(Number(tr.dataset.kunde)); });
        });
        document.querySelectorAll("#t-start [data-abr-id]").forEach(function (b) {
          b.addEventListener("click", function (ev) {
            ev.stopPropagation(); // nicht zusaetzlich den Zeilen-Klick ausloesen
            kundeOeffnen(Number(b.dataset.abrKunde), Number(b.dataset.abrId));
          });
        });
      })
      .catch(function (e) {
        document.getElementById("startKacheln").innerHTML = '<p class="fehler">' + fehlerText(e) + "</p>";
      });
  }

  // Grosses Suchfeld: Name oder Kunden-Nr. tippen - Vorschlaege erscheinen
  // darunter, Enter oeffnet direkt das Kundenblatt (bei einer Nummer genau
  // diese Kundin, bei einem Namen den ersten Treffer).
  var elStartSuche = document.getElementById("startSuche");
  var elStartTreffer = document.getElementById("startTreffer");

  function startTrefferZeigen(kunden, gesamt) {
    elStartTreffer.innerHTML = kunden.map(function (k, i) {
      return '<button type="button" class="knopf knopf-klein" data-start-treffer="' + i + '">Nr. ' + k.nummer + " · " +
        escapeHtml(k.vorname + " " + k.name) + (k.ort ? " · " + escapeHtml(k.ort) : "") + "</button>";
    }).join("") + (gesamt > kunden.length ? '<span style="font-size:12px;color:var(--tinte-2)">… genauer suchen</span>' : "");
    elStartTreffer.querySelectorAll("[data-start-treffer]").forEach(function (b) {
      b.addEventListener("click", function () {
        elStartSuche.value = "";
        elStartTreffer.innerHTML = "";
        kundeOeffnen(kunden[Number(b.dataset.startTreffer)].id);
      });
    });
  }

  function startSuchen(direktOeffnen) {
    var text = elStartSuche.value.trim();
    elStartTreffer.innerHTML = "";
    if (!text) return;
    function oeffnen(k) { elStartSuche.value = ""; elStartTreffer.innerHTML = ""; kundeOeffnen(k.id); }
    if (/^\d+$/.test(text)) {
      invoke("kunde_nach_nummer", { nummer: Number(text) })
        .then(function (k) { if (direktOeffnen) oeffnen(k); else startTrefferZeigen([k], 1); })
        .catch(function (e) { elStartTreffer.innerHTML = '<span class="fehler">' + fehlerText(e) + "</span>"; });
      return;
    }
    invoke("kunden_suchen", { suchtext: text, archiv_zeigen: true }).then(function (kunden) {
      if (!kunden.length) { elStartTreffer.innerHTML = '<span class="fehler">Niemand gefunden.</span>'; return; }
      if (direktOeffnen) oeffnen(kunden[0]);
      else startTrefferZeigen(kunden.slice(0, 6), kunden.length);
    });
  }

  elStartSuche.addEventListener("input", debounce(function () { startSuchen(false); }, 200));
  elStartSuche.addEventListener("keydown", function (ev) {
    if (ev.key === "Enter") { ev.preventDefault(); startSuchen(true); }
    if (ev.key === "Escape") { elStartSuche.value = ""; elStartTreffer.innerHTML = ""; }
  });

  document.getElementById("startNeuerAuftrag").addEventListener("click", function () {
    reiterOeffnen("r-auftraege");
    document.getElementById("au-kunde").focus();
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

  // "Speichern & Auftrag drucken" = derselbe Ablauf, danach gleich der
  // Auftragsschein.
  var auNachSpeichernDrucken = false;
  document.getElementById("au-speichern-drucken").addEventListener("click", function () {
    auNachSpeichernDrucken = true;
    document.getElementById("au-speichern").click();
  });

  document.getElementById("au-speichern").addEventListener("click", function () {
    var drucken = auNachSpeichernDrucken;
    auNachSpeichernDrucken = false;
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
          " gespeichert (CHF " + chf(a.summe) + ")" + (a.abholdatum ? ", abholen am " + datumKurz(a.abholdatum) : "") + ". ";
        var nochmal = document.createElement("button");
        nochmal.type = "button";
        nochmal.className = "link-knopf";
        nochmal.textContent = "Auftragsschein drucken";
        nochmal.addEventListener("click", function () { auftragsscheinDrucken(a.id, kunde.id, nochmal); });
        elAuErfolg.appendChild(nochmal);
        elAuErfolg.hidden = false;
        if (drucken) invoke("quittung_als_pdf_oeffnen", { auftrag: a, kunde: kunde }).catch(function (e) { alert(fehlerText(e)); });
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
    zaehlerAktualisieren();
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
            ? '<span class="aktionen"><button type="button" class="knopf knopf-klein" data-schein-id="' + z.id + '" data-schein-kunde="' + z.kunde_id +
              '" title="Auftragsschein drucken">🖨</button>' +
              '<button type="button" class="knopf knopf-voll knopf-klein" data-abrechnen-kunde="' + z.kunde_id + '" data-abrechnen-id="' + z.id + '">Abrechnen</button></span>'
            : !z.bezahlt
              ? '<button type="button" class="knopf knopf-klein" data-bezahlt-id="' + z.id + '">bezahlt ✓</button>'
              : '<span class="zahlart">' + escapeHtml(z.zahlart) + "</span>";
          var alter = auFilter === "unbezahlt"
            ? ' <span style="font-size:12px;color:var(--tinte-3)">(' + z.alter_tage + (z.alter_tage === 1 ? " Tag" : " Tage") + ")</span>"
            : "";
          return '<tr class="' + streifenKlasse(z) + '">' +
            "<td>" + z.rechnungsnummer + "</td>" +
            '<td><button type="button" class="knopf knopf-klein" data-kunde-id="' + z.kunde_id + '" title="Kundenblatt öffnen">' +
            "Kd. " + z.kunde_nummer + " · " + escapeHtml(z.kunde_name) + "</button></td>" +
            "<td>" + escapeHtml(z.arbeit) + "</td>" +
            "<td>" + datumKurz(z.angenommen_am || z.datum) + alter + "</td>" +
            '<td class="' + (istUeberfaellig(z) ? "ueberfaellig" : "") + '">' + (z.abholdatum ? datumKurz(z.abholdatum) : "–") + "</td>" +
            "<td>" + (laufend ? statusWahlHtml(z) + (istUeberfaellig(z) ? " " + statusChip(z) : "") : statusChip(z)) + "</td>" +
            '<td class="re">' + chf(z.summe) + "</td>" +
            "<td>" + aktion + "</td></tr>";
        }).join("");

        elListe.querySelectorAll("[data-kunde-id]").forEach(function (b) {
          b.addEventListener("click", function () { kundeOeffnen(Number(b.dataset.kundeId)); });
        });
        elListe.querySelectorAll("[data-schein-id]").forEach(function (b) {
          b.addEventListener("click", function () { auftragsscheinDrucken(Number(b.dataset.scheinId), Number(b.dataset.scheinKunde), b); });
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
  // Nur die Bereiche der Seitenleiste - die Unterreiter im Kundenblatt
  // sind ebenfalls role="tab", schalten aber nur innerhalb des Blatts um.
  document.querySelectorAll('.seitenleiste [role="tab"]').forEach(function (t) {
    t.addEventListener("click", function () {
      document.querySelectorAll('.seitenleiste [role="tab"]').forEach(function (x) {
        var an = x === t;
        x.setAttribute("aria-selected", an);
        document.getElementById(x.getAttribute("aria-controls")).hidden = !an;
      });
      if (t.id === "r-start") uebersichtLaden();
      if (t.id === "r-auftraege") auListeLaden();
      if (t.id === "r-preisliste") plLaden();
      if (t.id === "r-monat") monatLaden();
      if (t.id === "r-stunden") maLaden();
      if (t.id === "r-treuhand") thLaden();
    });
  });

  // ================= KUNDENORDNER EINLESEN =================
  // Stefans bisherige Ablage: pro Kundin ein Ordner "Nummer Name" mit einer
  // Excel-Datei, jedes Blatt eine Rechnung (kundenordner.rs). Hier: Ordner
  // waehlen, Vorschau, einlesen. Die alten Rechnungen erscheinen danach im
  // Kundenblatt unter "Verlauf" - ohne im Umsatz mitzuzaehlen.
  var elKoDialog = document.getElementById("kundenordnerDialog");
  var elKoVorschau = document.getElementById("ko-vorschau");
  var elKoFehler = document.getElementById("ko-fehler");
  var elKoKnopf = document.getElementById("ko-importieren");
  var koPfad = null;

  document.getElementById("kundenordnerKnopf").addEventListener("click", function () {
    koPfad = null;
    elKoVorschau.innerHTML = "";
    elKoFehler.hidden = true;
    elKoKnopf.disabled = true;
    document.getElementById("ko-pfad").textContent = "";
    elKoDialog.showModal();
  });
  document.getElementById("ko-abbrechen").addEventListener("click", function () { elKoDialog.close(); });

  document.getElementById("ko-waehlen").addEventListener("click", function () {
    if (!window.__TAURI__.dialog || !window.__TAURI__.dialog.open) return;
    window.__TAURI__.dialog.open({ directory: true, multiple: false }).then(function (pfad) {
      if (!pfad) return;
      koPfad = pfad;
      document.getElementById("ko-pfad").textContent = pfad;
      elKoFehler.hidden = true;
      elKoKnopf.disabled = true;
      elKoVorschau.innerHTML = '<div class="import-zusammenfassung">Lese die Ordner … (bei vielen Kundinnen einige Sekunden)</div>';
      invoke("kundenordner_vorschau", { pfad: pfad })
        .then(koVorschauZeichnen)
        .catch(function (e) { elKoVorschau.innerHTML = ""; elKoFehler.textContent = fehlerText(e); elKoFehler.hidden = false; });
    });
  });

  function koVorschauZeichnen(liste) {
    var kundinnen = liste.filter(function (k) { return k.nummer !== null || k.rechnungen > 0; });
    var neu = kundinnen.filter(function (k) { return !k.vorhanden; }).length;
    var rechnungen = kundinnen.reduce(function (s2, k) { return s2 + k.rechnungen; }, 0);
    var dateien = kundinnen.reduce(function (s2, k) { return s2 + k.dateien; }, 0);
    var ohne = liste.length - kundinnen.length;
    var mitFehler = kundinnen.filter(function (k) { return k.fehler.length; });
    elKoKnopf.disabled = !kundinnen.length;
    if (!liste.length) {
      elKoVorschau.innerHTML = '<div class="import-zusammenfassung">In diesem Ordner sind keine Unterordner – bitte den Ordner wählen, in dem die Kundenordner liegen (z. B. „Kunden 2026“).</div>';
      return;
    }
    elKoVorschau.innerHTML =
      '<div class="import-zusammenfassung"><b>' + kundinnen.length + " Kundenordner</b> gefunden: " +
      neu + " neue Kundinnen, " + (kundinnen.length - neu) + " schon im Programm (werden ergänzt) · " +
      "<b>" + rechnungen + " alte Rechnungen</b> · " + dateien + " Dateien." +
      (ohne ? "<br>" + ohne + " Ordner ohne Nummer und ohne Rechnung werden übersprungen." : "") +
      (mitFehler.length ? "<br><b>" + mitFehler.length + " Ordner mit unlesbarer Datei</b> – siehe Spalte „Hinweis“." : "") +
      "<br>Die alten Rechnungen zählen im Umsatz und für Treuhand mit – ausser in Monaten, die schon aus der Treuhand-Excel kommen (sonst doppelt).</div>" +
      '<div class="tabellenrahmen" style="max-height:340px;overflow:auto"><table class="auflistung"><thead><tr>' +
      '<th>Nr.</th><th>Name</th><th>Telefon</th><th>Ort</th><th class="re">Rechnungen</th><th class="re">CHF</th><th>Letzte</th><th>Hinweis</th>' +
      "</tr></thead><tbody>" +
      kundinnen.map(function (k) {
        return "<tr><td>" + (k.nummer === null ? "neu" : k.nummer) + "</td><td>" + escapeHtml(k.name) + "</td><td>" + escapeHtml(k.telefon) +
          "</td><td>" + escapeHtml(k.ort) + '</td><td class="re">' + k.rechnungen + '</td><td class="re">' + chf(k.summe) +
          "</td><td>" + (k.letzte ? datumKurz(k.letzte) : "–") + "</td><td>" +
          (k.vorhanden ? '<span class="status st-bereit">wird ergänzt</span> ' : "") +
          escapeHtml(k.fehler.join("; ")) + "</td></tr>";
      }).join("") + "</tbody></table></div>";
  }

  elKoKnopf.addEventListener("click", function () {
    if (!koPfad) return;
    elKoFehler.hidden = true;
    knopfSperren(elKoKnopf, true);
    invoke("kundenordner_importieren", { pfad: koPfad })
      .then(function (r) {
        elKoDialog.close();
        elSuche.value = "";
        suchtextSuchen();
        alert("Eingelesen: " + r.neu + " neue Kundinnen, " + r.ergaenzt + " ergänzt, " + r.rechnungen + " alte Rechnungen, " + r.dateien + " Dateien.");
      })
      .catch(function (e) { elKoFehler.textContent = fehlerText(e); elKoFehler.hidden = false; })
      .finally(function () { knopfSperren(elKoKnopf, false); });
  });

  // Im Kundenblatt unter "Verlauf": die frueheren Rechnungen aus dem alten
  // Ordner und die Dateien daraus (oeffnen mit Excel & Co.).
  function altesArchivLaden(kundeId) {
    Promise.all([invoke("alte_rechnungen_von_kunde", { kunde_id: kundeId }), invoke("kunden_dateien_von_kunde", { kunde_id: kundeId })])
      .then(function (e) {
        var el = document.getElementById("altesArchiv");
        if (!el || !aktuellerKunde || aktuellerKunde.id !== kundeId) return;
        var rechnungen = e[0], dateien = e[1];
        if (!rechnungen.length && !dateien.length) { el.innerHTML = ""; return; }
        var total = rechnungen.reduce(function (s2, r) { return s2 + r.summe; }, 0);
        el.innerHTML =
          '<h3 class="altes-archiv-titel">Frühere Rechnungen <small>aus dem alten Kundenordner · ' + rechnungen.length +
          " Rechnungen · CHF " + chf(total) + "</small></h3>" +
          (dateien.length
            ? '<div class="knopfreihe" style="margin-top:0">' + dateien.map(function (d) {
                return '<button type="button" class="knopf knopf-klein" data-datei-id="' + d.id + '">📄 ' + escapeHtml(d.dateiname) + "</button>";
              }).join("") + "</div>"
            : "") +
          (rechnungen.length
            ? '<table class="verlauf"><thead><tr><th>Datum</th><th>Arbeit</th><th class="re">Zahlart</th><th class="re">CHF</th></tr></thead><tbody>' +
              rechnungen.map(function (r) {
                return "<tr><td>" + (r.datum ? datumKurz(r.datum) : "–") + '</td><td class="alte-posten">' +
                  escapeHtml(r.posten || "–").replace(/\n/g, "<br>") + '<br><small>' + escapeHtml(r.quelle) + "</small></td>" +
                  '<td class="re">' + escapeHtml(r.zahlart || "–") + '</td><td class="re">' + chf(r.summe) + "</td></tr>";
              }).join("") + "</tbody></table>"
            : "");
        el.querySelectorAll("[data-datei-id]").forEach(function (b) {
          b.addEventListener("click", function () {
            knopfSperren(b, true);
            invoke("kunden_datei_oeffnen", { id: Number(b.dataset.dateiId) })
              .catch(function (err) { alert(fehlerText(err)); })
              .finally(function () { knopfSperren(b, false); });
          });
        });
      })
      .catch(function () {});
  }

  // ================= DATEN-UEBERGABE (anderer PC) =================
  // Stefan richtet auf seinem PC alles ein und gibt die Daten als eine
  // Datei weiter (uebergabe.rs); beim Vater wird sie eingespielt - in den
  // Einstellungen oder gleich beim ersten Start.
  function paketEinspielen(knopf, echo) {
    if (!window.__TAURI__.dialog || !window.__TAURI__.dialog.open) return;
    window.__TAURI__.dialog
      .open({ multiple: false, filters: [{ name: "Atelierbuch-Daten", extensions: ["atelierbuch", "sqlite3"] }] })
      .then(function (pfad) {
        if (!pfad) return;
        return invoke("datenpaket_pruefen", { pfad: pfad }).then(function (info) {
          var frage = "Daten-Paket einspielen?\n\n" +
            info.kunden + " Kundinnen, " + info.auftraege + " Aufträge, " + info.ausgaben + " Ausgaben, " +
            info.einnahmen_excel + " Einnahmen aus Excel, " + info.stunden + " Stunden-Einträge.\n" +
            "Anmelden danach mit: " + (info.konten.join(", ") || "–") + ".\n\n" +
            "Die Daten auf diesem PC werden dadurch ersetzt (vorher wird automatisch gesichert).";
          if (!confirm(frage)) return;
          knopfSperren(knopf, true);
          return invoke("datenpaket_einspielen", { pfad: pfad }).then(function () {
            alert("Eingespielt. Das Programm startet neu – bitte mit einem der Konten anmelden: " + info.konten.join(", "));
            location.reload();
          });
        });
      })
      .catch(function (e) {
        knopfSperren(knopf, false);
        if (echo) { echo.style.color = "var(--faden)"; echo.textContent = fehlerText(e); echo.hidden = false; }
        else alert(fehlerText(e));
      });
  }

  document.getElementById("eiPaketErstellen").addEventListener("click", function () {
    var k = this, echo = document.getElementById("eiPaketEcho");
    knopfSperren(k, true);
    echo.hidden = true;
    invoke("datenpaket_erstellen")
      .then(function (pfad) {
        echo.style.color = "var(--gruen)";
        echo.textContent = "Erstellt: " + pfad + " – diese Datei auf den anderen PC bringen.";
        echo.hidden = false;
      })
      .catch(function (e) { echo.style.color = "var(--faden)"; echo.textContent = fehlerText(e); echo.hidden = false; })
      .finally(function () { knopfSperren(k, false); });
  });
  document.getElementById("eiPaketEinspielen").addEventListener("click", function () {
    paketEinspielen(this, document.getElementById("eiPaketEcho"));
  });
  document.getElementById("einrichtung-paket").addEventListener("click", function () {
    paketEinspielen(this, document.getElementById("einrichtung-fehler"));
  });

  // ================= AUTOMATISCHES UPDATE =================
  // Beim Start still pruefen (update.rs); gibt es eine neue Version,
  // erscheint unten in der Seitenleiste ein Hinweis. Ohne Internet oder bei
  // einem Fehler passiert beim stillen Pruefen einfach nichts.
  function updateAnzeigen(info) {
    var verfuegbar = info && info.verfuegbar;
    document.getElementById("updateHinweis").hidden = !verfuegbar;
    document.getElementById("eiUpdateInstallieren").hidden = !verfuegbar;
    if (verfuegbar) document.getElementById("updateHinweisText").textContent = "vom " + info.neu_datum;
    var text = !info ? "Version konnte nicht geprüft werden."
      : info.entwicklung ? "Entwicklungsversion (sucht nicht nach Updates)."
      : "Installiert: Version vom " + (info.installiert_datum || "?") + ". " +
        (verfuegbar ? "Neue Version vom " + info.neu_datum + " ist bereit." : "Das ist die neueste Version.");
    document.getElementById("eiVersionText").textContent = text;
  }

  function updatePruefen(still) {
    var echo = document.getElementById("eiUpdateEcho");
    echo.hidden = true;
    return invoke("update_pruefen")
      .then(updateAnzeigen)
      .catch(function (e) {
        if (still) return;
        echo.textContent = fehlerText(e);
        echo.hidden = false;
      });
  }

  function updateInstallieren(knopf) {
    if (!confirm("Jetzt aktualisieren? Das Programm lädt die neue Version herunter, schliesst sich und startet danach neu. Ihre Daten bleiben erhalten.")) return;
    knopfSperren(knopf, true);
    knopf.textContent = "Wird heruntergeladen …";
    invoke("update_installieren").catch(function (e) {
      knopfSperren(knopf, false);
      alert(fehlerText(e));
    });
  }

  document.getElementById("eiUpdateKnopf").addEventListener("click", function () {
    var k = this;
    knopfSperren(k, true);
    updatePruefen(false).finally(function () { knopfSperren(k, false); });
  });
  document.getElementById("eiUpdateInstallieren").addEventListener("click", function () { updateInstallieren(this); });
  document.getElementById("updateJetztKnopf").addEventListener("click", function () { updateInstallieren(this); });

  // ================= START =================
  function programmStarten() {
    suchtextSuchen();
    elBlatt.innerHTML = '<p class="leer" style="padding:40px">Links einen Kunden wählen oder „+ Neuer Kunde".</p>';
    auPostenZeichnen();
    preislisteVorschlaegeLaden();
    zaehlerAktualisieren();
    reiterOeffnen("r-start");
    updatePruefen(true);
  }
})();
