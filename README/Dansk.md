# Deeprotection - Dansk Udgave v1.0.0

Deeprotection er et sikkerhedsværktøj, der kan blokere farlige Linux-kommandoer og mistænkelige script i realtid. Det beskytter systemet ved at blokere ikke-autoriserede operationer, logge risikabelt adfærd og give advarsler om potentielle sikkerhedsårhuller.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Hej, tak fordi du bruger vores produkt!</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>Udforsk projektdokumentationen »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">Se Demo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Rapportér Fejl</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Forslag til nye funktioner</a>
  </p>
</p>

## Indhold

- [1. Filsystem](#filesystem)
- [2. Brugerhåndbog](#brugerhåndbog)
  - [1. Konfigurationsfil](#1-konfigurationsfil)
  - [2. Konfigurationsfilens placering](#2-konfigurationsfilens-placering)
  - [3. Script funktioner](#3-script-funktioner)
- [3. Distribution](#distribution)
  - [Stier](#stier)
- [4. Tekniske detaljer](#tekniske-detaljer)
- [5. Bidragsyderliste](#bidragsyderliste)
- [6. Licensaftale](#licensaftale)
- [7. Tak](#tak)

### Filsystem
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

### Brugerhåndbog

#### 1. Konfigurationsfil

`deeprotection.conf`

```
disable=false         # Aktiver
expire_hours=5        # Standardvarighed
timestamp=            # Tidsstempel
update=enable         # Aktiver automatisk opdatering
...
...                   # Blokering regler
...
```

#### 2. Konfigurationsfilens placering

```
/etc/deeprotection/deeprotection.conf		# Standardplaceringen kan ændres
```

#### 3. Script funktioner

```
launcher        # Start
loader          # Kontroller opdateringer og valider konfigurationsfil
mariana─core    # Hovedbeskyttelsesprogram
```

### Distribution

#### Stier

```
/
├── etc
│   └── deeprotection
│       └── deeprotection.conf    # Konfigurationsfil og regler
├── usr
│   └── bin 
│   ├── launcher                  # Startprogram
│   └── mariana─core              # Beskyttelsesprogram
└── var
    └── log
        └── deeprotection.log
```

### Tekniske detaljer

Læs [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md) for at få mere at vide om projektets arkitektur.

### Bidragsyderliste

Læs [CONTRIBUTING.md](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING.md) for at se listen over udviklere, der har bidraget til projektet.

### Licensaftale

![CC-BY-NC-SA](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by-nc-sa.svg)

Dette projekt er licensieret under [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/). Du kan frit bruge, dele, ændre og vise projektet til ikke-kommercielle formål under følgende vilkår:

1. **Tilskrivelse**: Du skal bevare den oprindelige forfatters navn.
2. **Ikke-kommerciel brug**: Du må ikke bruge projektet til kommercielle formål eller opnå økonomisk fordel gennem det.
3. **Del på samme vilkår**: Hvis du ændrer projektet eller laver afledte værker, skal de nye værker også licensieres under CC-BY-NC-SA.

Bemærk, at CC-BY-NC-SA-licensen ikke friger dig fra andre mulige juridiske forpligtelser eller ansvar vedrørende brugen af projektet. Du skal selv bære risikoen for eventuelle problemer, der kan opstå ved brugen af projektet.

Den komplette CC-BY-NC-SA-licenstekst kan findes i projektets [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE)-fil. Hvis du har spørgsmål om licensen eller ønsker yderligere forklaringer, kontakt venligst os.

Vi er truly thankful for din støtte og bidrag og ser frem til at du bidrager til projektets udvikling. Samtidig opfordrer vi dig til at følge licensens regler for at sikre projektets bæredygtige udvikling og beskytte den oprindelige forfatters rettigheder.

Tak for din støtte og deltagelse!

### Tak

- [GitHub Pages](https://pages.github.com)
