# Deeprotection - Deutsche Version

Deeprotection ist ein Sicherheitstool, das Linux-Befehle mit hohem Risiko und verdächtige Skripte in Echtzeit abfängt. Es schützt Ihr System, indem es nicht autorisierte Operationen blockiert, riskantes Verhalten protokolliert und Warnungen vor potenziellen Sicherheitslücken ausgibt.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Hallo, danke für die Nutzung!</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>Dokumentation erkunden »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">Demo anzeigen</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Bug melden</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Feature anfordern</a>
  </p>

## Inhaltsverzeichnis

- [I. Dateistruktur](#dateistruktur)
- [II. Bedienungsanleitung](#bedienungsanleitung)
  - [1. Konfigurationsdatei](#1-konfigurationsdatei)
  - [2. Pfad der Konfigurationsdatei](#2-pfad-der-konfigurationsdatei)
  - [3. Skriptfunktionen](#3-skriptfunktionen)
- [III. Bereitstellung](#bereitstellung)
  - [Pfade](#pfade)
- [IV. Technische Details](#technische-details)
- [V. Mitwirkende](#mitwirkende)
- [VI. Lizenz](#lizenz)
- [VII. Danksagung](#danksagung)

### Dateistruktur
```
filetree 
├── LICENSE
├── README
│   ├── Bokmål.md
│   ├── Dansk.md
│   ├── Deutsch.md
│   ├── Français.md
│   ├── Italiano.md
│   ├── 한국어.md
│   ├── Svenska.md
│   ├── Русский.md
│   ├── 日本語.md
│   ├── 简体中文.md
│   └── 繁體中文.md
├── ARCHITECTURE.md
├── CONTRIBUTING.md
├── README.md
├── deeprotection.conf
├── launcher
├── loader
└── mariana─core
```

### Bedienungsanleitung

#### 1\. Konfigurationsdatei

`deeprotection.conf`

```
disable=false        # Aktivieren
expire_hours=5       # Standard-Dauer für das Deaktivieren
timestamp=           # Zeitstempel
update=enable        # Automatische Aktualisierung aktivieren
...
...                  # Abfangregeln
...
```

#### 2\. Pfad der Konfigurationsdatei

```
/etc/deeprotection/deeprotection.conf        # Der Standardpfad kann geändert werden
```

#### 3\. Skriptfunktionen

```
launcher            # Initialisierungsprogramm

loader              # Aktualisierungen überprüfen und Konfigurationsdatei überprüfen

mariana─core        # Haupt-Schutzprogramm
```

### Bereitstellung

#### Pfade

```
/
├── etc
│   └── deeprotection
│       └── deeprotection.conf        # Konfigurationsdatei und Regeln
├── usr
│   └── bin 
│       ├── launcher                  # Startprogramm
│       ├── loader                    # Initialisierungsprogramm
│       └── mariana─core              # Schutzprogramm
└── var
    └── log
        └── deeprotection.log
```

### Technische Details

Informationen zur Projektarchitektur finden Sie in der [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md).

### Mitwirkende

Eine Liste der an diesem Projekt beteiligten Entwickler finden Sie in der [CONTRIBUTING.md](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING.md).

### Lizenz

![CC-BY-NC-SA](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by-nc-sa.svg)

Dieses Projekt steht unter der [CC-BY-NC-SA-Lizenz](https://creativecommons.org/licenses/by-nc-sa/4.0/). Sie dürfen dieses Projekt unter nicht kommerziellen Zwecken frei nutzen, teilen, bearbeiten und präsentieren, müssen sich dabei jedoch an folgende Bedingungen halten:

1. **Namensnennung**: Sie müssen die Namensnennung des ursprünglichen Autors beibehalten.
2. **Nicht kommerziell**: Sie dürfen dieses Projekt nicht für kommerzielle Zwecke nutzen oder wirtschaftliche Vorteile daraus ziehen.
3. **Weitergabe unter gleichen Bedingungen**: Wenn Sie dieses Projekt bearbeiten oder Ableitungsarbeiten erstellen, müssen die neuen Werke ebenfalls unter der gleichen CC-BY-NC-SA-Lizenz veröffentlicht werden.

Bitte beachten Sie, dass die CC-BY-NC-SA-Lizenz Sie nicht von anderen gesetzlichen Verpflichtungen oder Haftungen befreit, die sich aus der Nutzung dieses Projekts ergeben können. Sie übernehmen das Risiko und die möglichen Folgen der Nutzung dieses Projekts selbst.

Den vollständigen Text der CC-BY-NC-SA-Lizenz finden Sie in der [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE)-Datei des Projekts. Wenn Sie Fragen zur Lizenz haben oder weitere Erklärungen benötigen, zögern Sie bitte nicht, uns zu kontaktieren.

Wir danken Ihnen herzlich für Ihre Unterstützung und Ihre Beiträge und freuen uns auf Ihre Mitwirkung bei der weiteren Entwicklung des Projekts. Bitte beachten Sie unbedingt die Lizenzbestimmungen, um die nachhaltige Entwicklung des Projekts und den Schutz der Rechte der ursprünglichen Autoren zu gewährleisten.

Vielen Dank nochmals für Ihre Unterstützung und Ihr Engagement!

### Danksagung

- [GitHub Pages](https://pages.github.com)
