# Deeprotection

Deeprotection is a security tool developed in Bash. It filters user commands through path protection, command interception, and deletion confirmation. It offers two operation strategies: Enhanced and Permissive modes.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="Logo" width="80" height="80">
  </a>
  <h3 align="center">Deeprotection</h3>
  <h5 align="center">: ) Hello, thank you for using! ⭐</h5>
  <p align="center">
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection"><strong>📖Explore the project documentation »</strong></a>
    <br />
    <br />
    <a href="https://github.com/Geekstrange/Deeprotection/blob/main/images/demo_en_US.mp4">🎬View Demo</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">🧪Report Bug</a>
    ·
    <a href="https://github.com/Geekstrange/Deeprotection/issues">🔭Propose New Feature</a>
  </p>
</p>

## 🌐Find your language!

### 🌏Asia

[🇨🇳简体中文](https://github.com/Geekstrange/Deeprotection/blob/main/README/简体中文.md) 更新至v0.0.5

[🇨🇳繁體中文](https://github.com/Geekstrange/Deeprotection/blob/main/README/繁體中文.md) 更新至v0.0.5

[🇯🇵日本語](https://github.com/Geekstrange/Deeprotection/blob/main/README/日本語.md) 更新至v0.0.5

[🇰🇷한국어](https://github.com/Geekstrange/Deeprotection/blob/main/README/한국어.md) 으로 업데이트v0.0.5

### 🌍Europe

[🇫🇷Français](https://github.com/Geekstrange/Deeprotection/blob/main/README/Français.md) v0.0.1

[🇩🇪Deutsch](https://github.com/Geekstrange/Deeprotection/blob/main/README/Deutsch.sh) v0.0.1

[🇮🇹Italiano](https://github.com/Geekstrange/Deeprotection/blob/main/README/Italiano.md) v0.0.1

[🇷🇺Русский](https://github.com/Geekstrange/Deeprotection/blob/main/README/Русский.md) v0.0.1

[🇸🇪Svenska](https://github.com/Geekstrange/Deeprotection/blob/main/README/Svenska.md) v0.0.1

[🇳🇴Bokmål](https://github.com/Geekstrange/Deeprotection/blob/main/README/Bokmål.md) v0.0.1

[🇩🇰Dansk](https://github.com/Geekstrange/Deeprotection/blob/main/README/Dansk.md) v0.0.1

---

## 📜Table of Contents

- [⚡Quick Start](#⚡quick-start)
  - [📦Installation](#📦installation)
- [🔍User Guide](#🔍user-guide)
  - [🕹Basic Usage](#🕹basic-usage)
  - [🛠Configuration File Introduction](#🛠configuration-file-introduction)
  - [📌Log Introduction](#📌log-introduction)
- [📂Installation Directory](#📂installation-directory)
- [🔬Technical Details](#🔬technical-details)
- [📃Contributors List](#📃contributors-list)
- [⚖License](#⚖license)
- [⭐Acknowledgements](#⭐acknowledgements)

## ⚡Quick Start

### 📦Installation

**Automatic Deployment**

Running the `install.sh` script automatically resolves system dependencies and installs the latest Release.

**Manual Installation**

If you enjoy the fun of manual installation, first run the `check_env.sh` script to automatically deploy the dependent environment.

Then you can obtain the latest version of Deeprotection from the [GitHub repository](https://github.com/Geekstrange/Deeprotection/) and install it.

```bash
git clone https://github.com/Geekstrange/Deeprotection.git

dpkg -i deeprotection.deb
```

**RAW**

The purest manual installation!

If you are a Linux beginner, it is recommended to use this method. The process of manually troubleshooting errors will help improve your Linux skills. Good luck!

## 🔍User Guide

### 🕹Basic Usage

**First Launch**

The first launch via the dplauncher module automatically obtains the current system language and confirms with the user. You can still manually change it in the configuration file or create your personalized language file.

*Naming Rules*

```
name_                     # Language Name
greet_                    # Greeting
war_                      # Interception Warning Category
err_                      # Error Warning Category
log_                      # Log Recording Category
ask_                      # Message Inquiry Category
msg_                      # Status Warning Category
```

Each time you open a new session, it will ask whether to load the protection function. You can turn it off in the configuration file, but you can still directly call the protection kernel by entering the `dp` command in the terminal.

> [!WARNING]
> **DPSHELL IS JUST AN AUXILIARY TOOL AND CANNOT BE USED AS YOUR DEFAULT SHELL.**

All commands executed in dpshell will not be recorded.

For Linux beginners, you may need to first understand the classification of subshells before starting the tutorial.

>- **sub-shell**：Created via `fork`, it can inherit variables, functions, aliases, etc., from the parent `shell`, but modifications to these data will not affect the parent `shell`. Generation methods for `sub-shell` include process replacement, command replacement, `(LIST)`, `|`, or `&`.
>- **child-shell**：Created via `fork-exec` mode, it can only inherit environment variables exported by the parent `shell` through `export`.

**Enhanced `cd` Command**

In dpshell, entering `cd` changes the directory and echoes the current working path.

```bash
(Enter exit or Ctrl+D to quit)
dpshell(1)# cd DEBUG_BAK/
/root/DEBUG_BAK
dpshell(1)#
```

In dpshell, entering `cd ?` allows you to input a number to enter the corresponding directory.

```bash
(Enter exit or Ctrl+D to quit)
dpshell(1)# cd ?
1) DEBIAN/
2) etc/
3) usr/
4) var/
Select a directory (Enter q to quit):
```

In dpshell, entering `cd ??` enables you to consecutively select directories and hidden directories.

```bash
(Enter exit or Ctrl+D to quit)
dpshell(1)# cd ??
1) DEBIAN
2) etc
3) usr
4) var
l] Go back to the parent directory
q] Exit recursive mode
Current directory: /root/develop/deeprotection >
```

**Permissive Mode**

This mode only has command interception, command replacement, and interception of rm \* commands (unlimited for \*.txt commands)

Usage 1: Run with dp followed by a command

```bash
root@hyperv:~/develop/# dp echo Hello!
Hello!
```

Usage 2: Directly run the dp command

```bash
root@hyperv:~/develop/# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# echo Hello!
Hello!
```

* Interception Function

Configuration file example

```
 42 #command_intercept_rules
 43 echo
```

Add commands to be intercepted under the `#command_intercept_rules` line

Running effect (demonstrated in child shell mode)

```
root@hyperv:~/develop# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# echo
[!] Intercepted echo
```

* Command Replacement Function

Configuration file example

```
 42 #command_intercept_rules
 44 echo 111 > echo 222
```

Running effect (demonstrated in child shell mode)

```
root@hyperv:~/develop# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# echo 111
[!] Original command: echo 111 -> Replaced with: echo 222
222
```

* rm \* Command Interception

Enabled by default, no configuration file setup required

Running effect (demonstrated in child shell mode)

```
1. Intercept rm \* commands

root@hyperv:~/develop# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# rm -rf *
[!] Intercepted: Detected 'rm \*' operation, blocked

2. Allow rm *.txt commands

root@hyperv:~/developl# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# ls
1.txt  2.txt  3.txt  test
dpshell(1)# rm -f *.txt
dpshell(1)# ls
test
```

**Enhanced Mode**

This mode strictly distinguishes between upper and lower case in the configuration file. Please ensure proper spelling of `Enhanced`.

Enhanced mode interception process: Directory protection ---> Command interception ---> rm command reinforcement

The command interception function has been demonstrated. This section only showcases the directory protection and rm command reinforcement functions.

* Directory Protection

Configuration file example

```
 37 #protected_paths_list
 38 /root/develop
```

> [!CAUTION]
> **BECAUSE IT PROVIDES RECURSIVE PROTECTION, DO NOT ADD `/` AS A RULE.**

Running effect (demonstrated in shell mode)

*Prohibits executing all commands under the protected directory*

```
root@hyperv:~/develop# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# echo
[!] Warning: Operations on protected path /root/develop are prohibited
```

* rm Command Reinforcement

No configuration file setup required

Running effect (demonstrated in shell mode)

```
root@hyperv:~/develop# dp
dpshell>
(Enter exit or press Ctrl+D to exit)
dpshell(1)# rm -rf 111
[!] About to execute: /bin/rm -i -v -r 111
/bin/rm: remove regular empty file '111'? y
removed '111'
```

### 🛠Configuration File Introduction

> [!TIP]
> **Only the English version is actually installed**

You can customize Deeprotection's behavior through the `/etc/deeprotection/deeprotection.conf` file, such as adding custom high-risk commands and path protection rules.

```
# This is the Deeprotection configuration file.
# Here is an explanation for each configuration item.

# Language settings: Automatically obtained on first run.
# To manually set the language, use standard language codes.
# Language file path: /usr/share/locale/deeprotection
language=


# Startup settings: Default is false,
# which means it is enabled; set to true to disable.
disable=false


# Default disable duration settings: Select the
# disable duration in hours when choosing n.
expire_hours=2


# Temporary disable timestamp: Records the time of temporary disablement.
timestamp=


# Set auto-update: Default is disabled.
# To enable, change to enable.
update=disable


# Protection mode: Default is permissive mode.
# To enable enhanced mode, manually change to Enhanced
# Note that enhanced mode is case-sensitive.
mode=permissive

#---------------------User Rules---------------------

# Protected paths settings: Enabled in enhanced mode.
#protected_paths_list
/your/protect/path/here
# Command interception rules.
# If there is no > after the command,
# the command will be directly intercepted.
#command_intercept_rules
^:\s*()\s*{\s*:\s*|\s*:\s*&\s*}\s*;\s*: > echo "Detected Fork Bomb attack!"
^\s*function\s+\w+\s*$\s*$\s*{.*\|\s*&.*} > echo "Detected Pipeline background execution attack pattern"
```

### 📌Log Introduction

```
2025-05-12 22:10:20 | user: root | command: -f rm+pt | path: /root/develop | current_pid: 1561 | exit_code: 0
   Command execution time     |   Executing user  |                Command executed                 |        Command PID     | Command exit code
```

## 📂Installation Directory

```
├── etc
│   └── deeprotection
│       └── deeprotection.conf
├── usr
│   ├── local
│   │   └── bin
│   │       ├── dplauncher
│   │       ├── dploader
│   │       └── dp
│   └── share
│       ├── doc
│       │   └── deeprotection
│       │       ├── changelog.gz
│       │       ├── copyright
│       │       ├── OVERVIEW.gz
│       │       └── README.gz
│       ├── icons
│       │   └── deeprotection.svg
│       └── locale
│           └── deeprotection
│               ├── da_DK
│               ├── de_DE
│               ├── en_US
│               ├── fr_FR
│               ├── it_IT
│               ├── ja_JP
│               ├── ko_KR
│               ├── nb_NO
│               ├── ru_RU
│               ├── sv_SE
│               ├── zh_CN
│               └── zh-Hant
└── var
    └── log
        └── deeprotection.log
```

## 🔬Technical Details

You can refer to the architecture design of this project in the [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/blob/main/ARCHITECTURE.md) file.

## 📃Contributors List

Thank you to all developers who have contributed to this project. You can view all contributors to this project in the [https://github.com/Geekstrange/Deeprotection/tree/main/CONTRIBUTORS) directory.

## ⚖License

![CC-BY-NC-SA](https://mirrors.creativecommons.org/presskit/buttons/88x31/svg/by-nc-sa.svg)

This project is licensed under the [CC-BY-NC-SA License](https://creativecommons.org/licenses/by-nc-sa/4.0/). You may freely use, share, modify, and display this project for non-commercial purposes, provided you comply with the following terms:

1. **Attribution**：You must retain the original author's attribution information.
2. **Non-Commercial Use**：You may not use this project for any commercial purposes or derive economic benefits from it.
3. **Derivative Works**：If you modify this project or create derivative works based on it, the new works must also adopt the CC-BY-NC-SA License.

Please note that the CC-BY-NC-SA License does not exempt you from other legal obligations or liabilities that may arise from using this project. You assume all risks and consequences arising from the use of this project.

The full text of the CC-BY-NC-SA License can be found in the project's [LICENSE](https://github.com/Geekstrange/Deeprotection/LICENSE) file. If you have any questions about the license or require further clarification, please feel free to contact me.

We sincerely appreciate your support and contributions and look forward to your participation in advancing the project. At the same time, please ensure compliance with the license to safeguard the project's sustainable development and protect the rights of the original authors.

Thank you again for your support and involvement!

## ⭐Acknowledgements

**Below are the indispensable dependencies of this project**

**Listed in alphabetical order, no ranking implied**

[ShellCheck](https://www.shellcheck.net/) provides us with a shell script analysis tool that helps enhance the project's code quality.
