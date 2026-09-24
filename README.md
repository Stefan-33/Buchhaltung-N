# Atelierbuch Straub

Kunden- und Auftragsverwaltung für Nähservice Straub, Pfäffikon SZ.

Ein richtiges Windows-Programm (kein Browser, keine Webseite), das komplett
lokal auf einem PC läuft. Die Daten liegen in einer einzigen Datei auf dem
Computer, es braucht kein Internet und keine laufenden Kosten.

## Was es kann

- Kunden anlegen und per Sofortsuche wiederfinden (Name, Ort, Telefon)
- Automatisch vergebene Kundennummer — kein Nachzählen mehr im Register
- Aufträge erfassen, Beleg drucken
- Kartensatz (1,5 % / 2,5 %) pro Kunde hinterlegt, rechnet sich bei jeder
  Kartenzahlung automatisch mit ein
- Monatsübersicht: Bar, Twint, Karte, Rechnung, Anzahl Kunden
- Automatische Sicherung auf den PC (Datenbank-Kopie + Excel-lesbare Liste)
- Zwei eigene Logins auf demselben Computer

## Installation

Unter **Actions** → jüngster Lauf von *Windows-Installationsdatei bauen* →
**Artifacts** liegt eine fertige `.msi`-Datei zum Herunterladen und
Installieren, wie jedes andere Windows-Programm.

Beim allerersten Start wird automatisch ein Konto mit dem Benutzernamen
`papa` angelegt; das zufällig erzeugte Start-Passwort erscheint einmalig im
Programm-Protokoll. Danach unter „Konto" sofort ein eigenes Passwort
setzen und ein zweites Konto für den Notfallzugang anlegen.

## Entwicklung

Gebaut mit [Tauri](https://tauri.app) (Rust-Backend, SQLite-Datenbank,
schlichtes HTML/CSS/JS-Frontend ohne Build-Schritt). Der Windows-Installer
wird automatisch über GitHub Actions erzeugt, da hier lokal keine
Windows-Umgebung zur Verfügung steht.

```
src-tauri/   Rust-Backend: Datenbank, Auftragslogik, Login, Sicherung
src/         Programmoberfläche (index.html, styles.css, app.js)
```
