# Deeprotection - Svenska Version

Deeprotection är ett säkerhet-verktyg som i realtid blockrar högriskkommandon och misstänkta skript i Linux. Det skyddar systemet genom att blockera oautorisade åtgärder, logga riskfyllda beteenden och ge varningar om potentiella säkerhets risker.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Hej, tack för att du använder oss!</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong> Utforska projektets dokumentation »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">Visa Demo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Rapportera fel</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Förslag på nya funktioner</a>
  </p>
</p>

## Innehåll

- [1. Filsystem](#filesystem)
- [2. Användarhandledning](#användarhandledning)
  - [1. Konfigurationsfil](#1-konfigurationsfil)
  - [2. Sökväg till konfigurationsfil](#2-sökväg-till-konfigurationsfil)
  - [3. Skriptfunktioner](#3-skriptfunktioner)
- [3. Distribution](#distribution)
  - [Sökvägar](#sökvägar)
- [4. Tekniska detaljer](#tekniska-detaljer)
- [5. Bidragsgivare](#bidragsgivare)
- [6. Licensavtal](#licensavtal)
- [7. Tacknemang](#tacknemang)

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

### Användarhandledning

#### 1. Konfigurationsfil

`deeprotection.conf`

```
disable=false		# Aktivera
expire_hours=5		# Standardvaraktighet
timestamp=			# Tidsstämpel
update=enable		# Aktivera automatisk uppdatering
...
...					# Blockeringsregler
...
```

#### 2. Sökväg till konfigurationsfil

```
/etc/deeprotection/deeprotection.conf		# Standardplatsen kan ändras
```

#### 3. Skriptfunktioner

```
launcher			# Starta
loader				# Kontrollera uppdateringar och validera konfigurationsfil
mariana─core		# Huvudskyddprogram
```

### Distribution

#### Sökvägar

```
/
├── etc
│ 	└── deeprotection
│ 		└── deeprotection.conf		# Konfigurationsfil och regler
├── usr
│ 	└── bin 
│		├── launcher				# Startprogram
│		├── loader					# Initieringsprogram
│		└── mariana─core			# Skyddprogram
└── var
    └── log
    	└── deeprotection.log
```

### Tekniska detaljer

Läs [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md) för att få mer information om projektets arkitektur.

### Bidragsgivare

Läs [CONTRIBUTING.md](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING.md) för att se listan över utvecklare som bidragit till projektet.

### Licensavtal

[![CC─BY─NC─SA Badge](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by─nc─sa.svg)](https://creativecommons.org/licenses/by-nc-sa/4.0/)

Detta projekt licensieras under [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/). Du kan fritt använda, dela, modifiera och visa projektet för icke-kommersiella ändamål under följande villkor:

1. **Till-attribuering**: Du måste behålla uppgifterna om den ursprunglige författaren.
2. **Icke-kommersiell användning**: Du får inte använda projektet för några kommersiella ändamål eller dra några ekonomiska fördelar av det.
3. **Delad i samma licens**: Om du modifierar projektet eller skapar derivata verk måste nya verk licensieras under samma CC-BY-NC-SA.

Observera att CC-BY-NC-SA-licensen inte befriar dig från andra juridiska skyldigheter eller ansvar som kan uppstå i samband med användningen av projektet. Du ansvarar själv för eventuella risker och konsekvenser som kan uppstå vid användningen av projektet.

Den fullständiga texten till CC-BY-NC-SA-licensen kan hittas i projektets [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE)-fil. Om du har några frågor om licensen eller behöver ytterligare förklaringar, vänligen kontakta oss.

Vi är innerligt tacksamma för din support och bidrag och ser fram emot att du lämnar in bidrag till projektets utveckling. Samtidigt uppmanar vi dig att följa licensens bestämmelser för att säkerställa projektets hållbara utveckling och skydda ursprunglig författares rättigheter.

Tack för din support och deltagande!

### Tacknemang

- [GitHub Emoji Cheat Sheet](https://www.webpagefx.com/tools/emoji─cheat─sheet)
- [GitHub Pages](https://pages.github.com)