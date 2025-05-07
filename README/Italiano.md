# Deeprotection - Versione Italiana

Deeprotection è uno strumento di sicurezza progettato per intercettare comandi Linux ad alto rischio e script sospetti in tempo reale. Protegge il tuo sistema bloccando operazioni non autorizzate, registrando comportamenti a rischio e fornendo allertesui potenziali vulnerabilità di sicurezza.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Ciao, grazie per l'utilizzo!</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>Esplora la documentazione »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">Vedi la demo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Segnala un bug</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Richiedi una nuova funzionalità</a>
  </p>

## Indice

- [I. Struttura dei file](#struttura-dei-file)
- [II. Guida alle operazioni](#guida-alle-operazioni)
  - [1. File di configurazione](#1-file-di-configurazione)
  - [2. Percorso del file di configurazione](#2-percorso-del-file-di-configurazione)
  - [3. Funzioni degli script](#3-funzioni-degli-script)
- [III. Distribuzione](#distribuzione)
  - [Percorsi](#percorsi)
- [IV. Dettagli tecnici](#dettagli-tecnici)
- [V. Elenco dei contributori](#elenco-dei-contributori)
- [VI. Licenza](#licenza)
- [VII. Ringraziamenti](#ringraziamenti)

### Struttura dei file
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

### Guida alle operazioni

#### 1\. File di configurazione

`deeprotection.conf`

```
disable=false        # Attivare
expire_hours=5       # Durata di disabilitazione predefinita
timestamp=           # Timestamp
update=enable        # Abilita aggiornamento automatico
...
...                  # Regole di intercettazione
...
```

#### 2\. Percorso del file di configurazione

```
/etc/deeprotection/deeprotection.conf        # Il percorso predefinito può essere modificato
```

#### 3\. Funzioni degli script

```
launcher            # Avviatore

loader              # Controlla gli aggiornamenti e verifica il file di configurazione

mariana─core        # Programma di protezione principale
```

### Distribuzione

#### Percorsi

```
/
├── etc
│   └── deeprotection
│       └── deeprotection.conf        # File di configurazione e regole
├── usr
│   └── bin 
│       ├── launcher                  # Programma di avvio
│       ├── loader                    # Programma di avvio
│       └── mariana─core              # Programma di protezione
└── var
    └── log
        └── deeprotection.log
```

### Dettagli tecnici

Per informazioni sull'architettura del progetto, fare riferimento a [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md).

### Elenco dei contributori

Per un elenco dei sviluppatori che hanno contribuito a questo progetto, fare riferimento a [CONTRIBUTING.md](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING.md).

### Licenza

![CC-BY-NC-SA](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by-nc-sa.svg)

Questo progetto è pubblicato sotto la licenza [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/). È possibile utilizzare, condividere, modificare e presentare questo progetto liberamente a scopo non commerciale, rispettando le seguenti condizioni:

1. **Credito**: È necessario mantenere le informazioni di accredito dell'autore originale.
2. **Uso non commerciale**: Non è possibile utilizzare questo progetto a fini commerciali o per trarre vantaggio economico.
3. **Condividi allo stesso modo**: Se si modifica questo progetto o se ne creano opere derivate, le nuove opere devono essere pubblicate sotto la stessa licenza CC-BY-NC-SA.

Si prega di notare che la licenza CC-BY-NC-SA non esonera dall'adempimento di altri obblighi legali o responsabilità che potrebbero sorgere dall'utilizzo di questo progetto. Si assumono tutti i rischi e le conseguenze derivanti dall'utilizzo di questo progetto.

Il testo completo della licenza CC-BY-NC-SA può essere trovato nel file [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE) del progetto. Se si hanno domande sulla licenza o si necessita di ulteriori chiarimenti, si prega di contattarci.

Grazie mille per il vostro supporto e contributo e speriamo che possiate partecipare allo sviluppo del progetto. Si prega di rispettare i termini della licenza per garantire lo sviluppo sostenibile del progetto e proteggere i diritti degli autori originali.

Grazie ancora per il vostro supporto e partecipazione!

### Ringraziamenti

- [GitHub Pages](https://pages.github.com)
