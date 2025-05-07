# Deeprotection - Bokmål Versjon

Deeprotection er et verktøy for sikkerhet som kan blokkere Linux-kommandoer og mistenkelige skript i sanntid. Det beskytter systemet ved å blokkere uautoriserte handlinger, logge risikovolle handlinger og varsle om potensielle sikkerhetshull.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Hei, takk for at du bruker oss!</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>Utforsk prosjektets dokumentasjon »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">Se Demo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Rapporter feil</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Foreslå nye funksjoner</a>
  </p>
</p>

## Innhold

- [1. Filsystem](#filesystem)
- [2. Brukerhåndbok](#brukerhåndbok)
  - [1. Konfigurasjonsfil](#1-konfigurasjonsfil)
  - [2. Sti til konfigurasjonsfil](#2-sti-til-konfigurasjonsfil)
  - [3. Skriptfunksjoner](#3-skriptfunksjoner)
- [3. Distribusjon](#distribusjon)
  - [Stier](#stier)
- [4. Tekniske detaljer](#tekniske-detaljer)
- [5. Bidragsytere](#bidragsytere)
- [6. Lisensavtale](#lisensavtale)
- [7. Takk](#takk)

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

### Brukerhåndbok

#### 1. Konfigurasjonsfil

`deeprotection.conf`

```
disable=false		# Aktiver
expire_hours=5		# Standardvarighet
timestamp=			# Tidsstempel
update=enable		# Aktiver automatisk oppdatering
...
...					# Blokkering regler
...
```

#### 2. Sti til konfigurasjonsfil

```
/etc/deeprotection/deeprotection.conf		# Standardstien kan endres
```

#### 3. Skriptfunksjoner

```
launcher			# Start
loader				# Kontroller oppdateringer og valider konfigurasjonsfil
mariana─core		# Hovedbeskyttelsesprogram
```

### Distribusjon

#### Stier

```
/
├── etc
│ 	└── deeprotection
│ 		└── deeprotection.conf		# Konfigurasjonsfil og regler
├── usr
│ 	└── bin 
│		├── launcher				# Startprogram
│		├── loader					# Oppstartprogram
│		└── mariana─core			# Beskyttelsesprogram
└── var
    └── log
    	└── deeprotection.log
```

### Tekniske detaljer

Les [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md) for å få mer informasjon om prosjektets arkitektur.

### Bidragsytere

Les [CONTRIBUTING.md](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING.md) for å se listen over utviklere som har bidratt til prosjektet.

### Lisensavtale

[![CC─BY─NC─SA Badge](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by─nc─sa.svg)](https://creativecommons.org/licenses/by-nc-sa/4.0/)

Dette prosjektet er lisensiert under [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/). Du kan fritt bruke, dele, endre og vise prosjektet for ikke-kommersielle formål under følgende vilkår:

1. **Tilskrivelse**: Du må bevare opplysningene om den opprinnelige forfatteren.
2. **Ikke-kommersiell bruk**: Du kan ikke bruke prosjektet for noen kommersielle formål eller oppnå økonomisk fordel gjennom det.
3. **Del på samme vilkår**: Hvis du endrer prosjektet eller lager avledede verker, må de nye verken lisensieres under samme CC-BY-NC-SA.

Merk at CC-BY-NC-SA-lisensen ikke frirer deg fra andre juridiske plikter eller ansvar som kan oppstå når du bruker prosjektet. Du er selv ansvarlig for eventuelle risikoer og følger som kan oppstå ved bruk av prosjektet.

Den fullstendige teksten til CC-BY-NC-SA-lisensen kan findes i prosjektets [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE)-fil. Hvis du har spørsmål om lisensen eller trenger ytterligere forklaringer, vennligst kontakt oss.

Vi er truly thankful for din støtte og bidrag og ser frem til at du bidrar til prosjektets utvikling. Samtidig oppmuntrer vi deg til å følge lisensens regler for å sikre prosjektets bærekraftige utvikling og verne opprinnelig forfatters rettigheter.

Takk for din støtte og deltakelse!

### Takk

- [GitHub Pages](https://pages.github.com)