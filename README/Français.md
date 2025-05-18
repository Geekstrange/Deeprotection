# Deeprotection - Version Française v1.0.0

Deeprotection est un outil de sécurité conçu pour intercepter les commandes Linux à risque élevé et les scripts suspects en temps réel. Il protège votre système en bloquant les opérations non autorisées, en enregistrant les comportements à risque et en fournissant des alertes sur les vulnérabilités de sécurité potentielles.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Bonjour, merci d'utiliser !</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>Explorer la documentation »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">Voir la démo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Signaler un bug</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Proposer une fonctionnalité</a>
  </p>

## Table des matières

- [I. Structure des fichiers](#structure-des-fichiers)
- [II. Guide d'opération](#guide-dopération)
  - [1. Fichier de configuration](#1-fichier-de-configuration)
  - [2. Chemin du fichier de configuration](#2-chemin-du-fichier-de-configuration)
  - [3. Fonctions des scripts](#3-fonctions-des-scripts)
- [III. Déploiement](#déploiement)
  - [Chemins](#chemins)
- [IV. Détails techniques](#détails-techniques)
- [V. Liste des contributeurs](#liste-des-contributeurs)
- [VI. Licence](#licence)
- [VII. Remerciements](#remerciements)

### Structure des fichiers
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

### Guide d'opération

#### 1. Fichier de configuration

`deeprotection.conf`

```
disable=false        # Activer
expire_hours=5       # Durée d'expiration par défaut
timestamp=           # Timestamp
update=enable        # Activer la mise à jour automatique
...
...                  # Règles d'interception
...
```

#### 2. Chemin du fichier de configuration

```
/etc/deeprotection/deeprotection.conf        # L'emplacement par défaut peut être modifié
```

#### 3. Fonctions des scripts

```
launcher            # Programme de démarrage

loader              # Vérifier les mises à jour et valider le fichier de configuration

mariana─core        # Programme de protection principal
```

### Déploiement

#### Chemins

```
/
├── etc
│   └── deeprotection
│       └── deeprotection.conf        # Fichier de configuration et règles
├── usr
│   └── bin 
│       ├── launcher                  # Programme de démarrage
│       └── mariana─core              # Programme de protection
└── var
    └── log
        └── deeprotection.log
```

### Détails techniques

Veuillez vous référer à [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md) pour consulter l'architecture de ce projet.

### Liste des contributeurs

Veuillez vous référer à [CONTRIBUTING.md](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING.md) pour connaître la liste des développeurs ayant contribué à ce projet.

### Licence

![CC-BY-NC-SA](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by-nc-sa.svg)

Ce projet est licencié sous [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/). Vous pouvez utiliser, partager, modifier et exposer ce projet librement à des fins non commerciales, sous réserve des conditions suivantes :

1. **Attribution** : Vous devez conserver les informations d'attribution de l'auteur original.
2. **Usage non commercial** : Vous ne pouvez pas utiliser ce projet à des fins commerciales ou en tirer un bénéfice économique.
3. **Partage dans les mêmes conditions** : Si vous modifiez ce projet ou créez des œuvres dérivées, les nouvelles œuvres doivent également être licenciées sous la même licence CC-BY-NC-SA.

Veuillez noter que la licence CC-BY-NC-SA ne vous exonère pas des autres obligations légales ou responsabilités qui pourraient résulter de l'utilisation de ce projet. Vous assumez tous les risques et conséquences liés à l'utilisation de ce projet.

Le texte complet de la licence CC-BY-NC-SA peut être trouvé dans le fichier [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE) du projet. Si vous avez des questions sur la licence ou si vous avez besoin de plus d'explications, n'hésitez pas à nous contacter.

Nous vous remercions sincèrement pour votre soutien et vos contributions et espérons que vous participerez au développement du projet. Tout en vous engageant à respecter les termes de la licence pour garantir la durabilité du projet et la protection des droits des auteurs originaux.

Merci encore pour votre soutien et votre participation !

### Remerciements

- [GitHub Pages](https://pages.github.com)
