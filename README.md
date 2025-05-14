# 🛡Deeprotection

Deepotection is a security protection tool developed in Bash. It has three mechanisms: path protection, command interception, and deletion confirmation. These prevent accidental operations in key system directories. It offers two modes: Enhanced Mode and Tolerant Mode.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Hello, thank you for using!</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>Explore the project documentation »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection">View Demo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Report Bug</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">Request Feature</a>
  </p>

## 🌐Find your language!

### 🌏Asia

[🇨🇳简体中文](https://github.com/Geekstrange/Deeprotection/blob/main/README/简体中文.md)

[🇨🇳繁體中文](https://github.com/Geekstrange/Deeprotection/blob/main/README/繁體中文.md)

[🇯🇵日本語](https://github.com/Geekstrange/Deeprotection/blob/main/README/日本語.md)

[🇰🇷한국어](https://github.com/Geekstrange/Deeprotection/blob/main/README/한국어.md)

### 🌍Europe

[🇫🇷Français](https://github.com/Geekstrange/Deeprotection/blob/main/README/Français.md)

[🇩🇪Deutsch](https://github.com/Geekstrange/Deeprotection/blob/main/README/Deutsch.sh)

[🇮🇹Italiano](https://github.com/Geekstrange/Deeprotection/blob/main/README/Italiano.md)

[🇷🇺Русский](https://github.com/Geekstrange/Deeprotection/blob/main/README/Русский.md)

[🇸🇪Svenska](https://github.com/Geekstrange/Deeprotection/blob/main/README/Svenska.md)

[🇳🇴Bokmål](https://github.com/Geekstrange/Deeprotection/blob/main/README/Bokmål.md)

[🇩🇰Dansk](https://github.com/Geekstrange/Deeprotection/blob/main/README/Dansk.md)

---

## 📜Table of Contents

- [Getting Started](#getting-started)
  - [Installation](#installation)
- [Usage Tutorial](#usage-tutorial)
  - [Basic Usage](#basic-usage)
- [Installation Directory](#installation-directory)
- [Technical Details](#technical-details)
- [List of Contributors](#list-of-contributors)
- [License Agreement](#license-agreement)
- [Contributor Agreement](#contributor-agreement)
- [Acknowledgements](#acknowledgements)

## ⚡Getting Started

### 📦Installation

You can get the latest version of Deeprotection from the [GitHub repository](https://github.com/Geekstrange/Deeprotection/) and install it.

```bash
git clone https://github.com/Geekstrange/Deeprotection.git
dpkg -i deeprotection.deb
```



## 🔍Usage Tutorial

### 🕹Basic Usage

You can check the `/var/log/deeprotection.log` file for detailed log information.

You can customize Deeprotection's behavior via the `/etc/deeprotection/deeprotection.conf` file. For example, you can add custom high-risk commands and path protection rules.

## 📂Installation Directory

```
/
├── etc
│   ├── deeprotection
│   │   └── deeprotection.conf
│   └── systemd
│       └── system
│           └── deeprotection.srevice
├── usr
│   ├── sbin
│   │   ├── launcher
│   │   ├── loader
│   │   └── mariana-core
│   └── share
│       ├── doc
│       │   └── deeprotection
│       │       ├── changelog.gz
│       │       ├── OVERVIEW.gz
│       │       └── README.gz
│       ├── icons
│       │   └── deeprotection.svg
│       └── locale
│           └── deeprotection
│               ├── da_DK
│               ├── de_DE
│               ├── en_US
│               ├── fr_FR
│               ├── it_IT
│               ├── ja_JP
│               ├── ko_KR
│               ├── nb_NO
│               ├── ru_RU
│               ├── sv_SE
│               ├── TEST
│               ├── zh_CN
│               └── zh-Hant
└── var
    └── log
        └── deeprotection.log
```

## 🔬Technical Details

You can refer to the [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/ARCHITECTURE.md) file for the project's architecture design.

## 📃List of Contributors

Thanks to all developers who have contributed to this project. You can view the list of contributors in the [CONTRIBUTING](https://github.com/Geekstrange/Deeprotection/CONTRIBUTING/) directory.

## ⚖License Agreement

![CC-BY-NC-SA](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by-nc-sa.svg)

This project is licensed under [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/). You can freely use, share, modify, and display this project for non-commercial purposes, but you must follow these terms:

1. ** Attribution**: You must retain the original author's attribution information.
2. **Non-Commercial Use**: You cannot use this project for any commercial purposes or derive economic benefits from it.
3. **Derivative Works**: If you modify this project or create derivative works, the new works must also be licensed under the same CC-BY-NC-SA license.

Please note that the CC-BY-NC-SA license does not exempt you from other legal obligations or liabilities that may arise from using this project. You assume all risks and consequences of using this project.

The full text of the CC-BY-NC-SA license can be found in the project's [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE) file. If you have any questions about the license or need further clarification, please feel free to contact me.

We appreciate your support and contributions and look forward to your participation in the project's development. At the same time, please comply with the license agreement to ensure the project's sustainable development and protect the original author's rights.

Thank you for your support and participation!

## 📑Contributor Agreement

![DCO](https://img.shields.io/badge/Developer%20Certificate%20of%20Origin-v1.1-blue.svg)

This project adopts the [Developer Certificate of Origin (DCO)](https://developercertificate.org/) v1.1. This ensures that contributors clearly state they have the right to submit code and agree to follow the project's license. Here's the full content of the DCO:

By contributing code, documentation, or other materials to this project, you declare and agree to the following:

1. **Permission to Contribute**: You have the right to submit code, documentation, or other materials to this project without violating any laws, regulations, or third-party agreements.
2. **Compliance with License**: Your contributions to this project will comply with the project's license, which is the [CC-BY-NC-SA](https://creativecommons.org/licenses/by-nc-sa/4.0/) license.
3. **Attribution and Declaration**: You retain the right to be attributed for your contributions and declare that you legally own the intellectual property rights to the submitted code, documentation, or other content, or have obtained legal authorization.
4. **Limitation of Liability**: You understand and agree that your contributions to this project are provided on an "AS IS" basis, without any form of warranty or liability.

When submitting contributions, you need to add the following statement in the code comments or contribution documentation for each submission:

```
Signed-off-by: Name <Email Address>
```

This statement indicates that you have read and agreed to the above DCO content.

If you contribute to this project, it means you agree to comply with the DCO regulations.

The full text of the DCO can be found on the [Developer Certificate of Origin](https://developercertificate.org/) website. If you have any questions about the DCO, please feel free to contact the project maintainer.

We sincerely appreciate your contributions and support. By following the DCO, you can help us ensure the project's legality and sustainability, contributing to its healthy development.

## ⭐Acknowledgements

We would like to thank [ShellCheck](https://www.shellcheck.net/) for providing shell script analysis tools. Their tool has helped us improve the project's code quality.
