# Deeprotection

Deeprotection is a high-performance security tool written in Rust. It acts as a secure shell wrapper (`dpshell`) that provides command interception, rule-based matching, plugin extensibility, path protection, audit logging, and password authentication. It offers three operation strategies: Enforcing, Permissive, and Disable modes.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="logo" width="100" height="100">
  </a>
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

------

## 📜Table of Contents

- [🔍User Guide](#user-guide)
- [🕹Basic Usage](#basic-usage)
- [🛡️Protection Modes](#protection-modes)
- [🛠Configuration File Introduction](#configuration-file-introduction)
- [📌Log Introduction](#log-introduction)
- [🧩Plugin Architecture](#plugin-architecture)
- [📂Installation Directory](#installation-directory)
- [🔬Technical Details](#technical-details)
- [📃Contributors List](#contributors-list)
- [⚖License](#license)
- [⭐Acknowledgements](#acknowledgements)

## 🔍User Guide

### 🕹Basic Usage

**First Launch & Context**

Deeprotection runs as a custom shell environment called `dpshell`. It handles commands natively, logging activity and applying your security rules before executing them via the system's standard shell.

**Enhanced `cd` Command**

In `dpshell`, the `cd` command comes with interactive built-ins to make terminal navigation cleaner and faster.

**Interactive Single-Level Navigation:** Entering `cd ?` allows you to view numbered subdirectories and input a number to enter the corresponding directory.

```bash
dpshell(1)# cd ?
1) amd64
2) arm64
3) debug
4) test_space
Select directory (enter q to quit):
```

**Recursive Navigation:** Entering `cd ??` enables a recursive directory browser, allowing you to traverse up and down the file tree interactively.

```bash
dpshell(1)# cd ??
1) debug
2) test_space
3) arm64
4) amd64
l] Back to parent directory
q] Exit recursive mode
Current directory: /root/dpshell >
```

**Nested Level Prompt**

The environment variable `DPSHELL_LEVEL` tracks nesting depth. The prompt displays as `dpshell(level)$ ` (or `#` for root), helping you identify the current shell nesting level.

### 🛡️ Protection Modes

Deeprotection operates in one of three modes, defined in your configuration file.

**Disable Mode**

Commands pass through without modification or blocking. Activity is strictly logged to the audit file. No rules, plugins, or path protection are applied.

**Permissive Mode**

Commands are evaluated against your defined `[[rules]]` and any active plugins. Path protection is **ignored**. This is excellent for testing rule logic.

**Enforcing Mode**

Strict security. Commands are evaluated against rules, plugins, and finally, the path protection engine. Operations involving commands in the `allowlist` targeting protected directories will require password authentication. Commands not in the `allowlist` will be blocked immediately.

```bash
dpshell(1)# ls test/
[!] Protected path operation requires authorization.
Admin password:
```

**Password Authentication**

In `enforcing` mode, when operating on protected paths with allowlisted commands, you will be prompted for password authentication (up to 3 attempts). The password is verified using SHA-256 against the hash stored in the configuration file.

### 🛠Configuration File Introduction

Deeprotection uses a clean, minimalist TOML configuration file located at `/etc/deeprotection/config.toml`.

```toml
[core]
# Operating mode: "disable", "permissive", or "enforcing"
mode = "enforcing"

[auth]
# SHA-256 hex digest of admin password (generate with: echo -n "pass" | sha256sum)
password_hash = "31fc7f00f4a0f72653d3ba5f445b8c21d922ae786da3f0a3a780f573942d00aa"

[paths]
# Directories that are strictly protected against modification commands
protect = ["/root/test", "/root/.ssh"]

# Commands allowed to operate on protected paths (requires authentication)
allowlist = ["rm", "rmdir", "mv", "cp", "chmod", "chown", "touch", "cat", "ls"]

# ---------------------User Rules---------------------

[[rules]]
name = "block_rm_rf"
pattern = "rm -rf"
action = { block = true }
enabled = true

[[rules]]
name = "replace_echo"
pattern = "re:^echo 111$"
action = { replace = "echo 222" }
enabled = true
```

**Rule Pattern Types:**

- **Plain string**: Automatically converted to an anchored regex that allows flexible whitespace (e.g., `"rm -rf"` becomes `^\s*rm\s+-rf\s*$`).
- **Explicit regex**: Prefixed with `re:` (e.g., `"re:^echo 111$"`).

**Rule Actions:**

- `block = true`: Block the command and log the action.
- `replace = "new command"`: Replace the command with the specified string.

### 📌Log Introduction

Logs use the JSON Lines (JSONL) format for seamless integration with modern log aggregators and dashboard tools. Logs are safely appended to `/var/log/audit.log`.

**Log Field Definitions:**

| Field         | Type   | Description                                 |
| ------------- | ------ | ------------------------------------------- |
| `timestamp`   | string | ISO 8601 UTC (second precision)             |
| `level`       | string | INFO / WARN                                 |
| `user`        | string | Username who executed the command           |
| `mode`        | string | disable / permissive / enforcing            |
| `command`     | string | Original user input command                 |
| `working_dir` | string | Current working directory at execution time |
| `pid`         | u32    | Process ID                                  |
| `exit_code`   | i32    | Exit code (reserved, currently 0)           |
| `message`     | string | Additional info (e.g., "blocked by rule")   |

**Example Log Entry:**

```json
{"timestamp":"2025-04-13T10:30:22Z","level":"WARN","user":"alice","mode":"enforcing","command":"rm /etc/passwd","working_dir":"/home/alice","pid":1234,"exit_code":0,"message":"blocked: command not in allowlist (final: rm /etc/passwd)"}
{"timestamp":"2025-04-13T10:32:05Z","level":"INFO","user":"alice","mode":"permissive","command":"echo 111","working_dir":"/home/alice","pid":1235,"exit_code":0,"message":"replaced to: echo 222"}
```

### 🧩Plugin Architecture

Deeprotection supports external extensibility via a robust plugin system. Drop your plugins into `/etc/deeprotection/plugins/<plugin-name>/`.

**Plugin Directory Structure:**

```
/etc/deeprotection/plugins/
  example-plugin/
    plugin.json
    entrypoint_script
```

**`plugin.json` Format:**

```json
{
  "id": "example-plugin",
  "name": "Example Plugin",
  "version": "1.0.0",
  "author": "Jane Doe",
  "description": "Description of what the plugin does.",
  "enabled": true,
  "entrypoint": "entrypoint_script"
}
```

**Plugin Invocation Model:**

- The command string is passed to the plugin via **stdin** and the environment variable `DPSHELL_COMMAND`.
- The plugin must exit with a specific code:
  - `0` → Allow the command (stdout ignored).
  - `1` → Block the command.
  - `2` → Replace the command; stdout must contain the new command string.
- Any other exit code, timeout (>5 seconds), or spawn failure results in **fail-open** (allow original command, warn to stderr).

**Execution Order:** Plugins are run **synchronously** in the order they were discovered (directory scan order). The command may be transformed by each plugin in sequence.

## 📂Installation Directory

```
├── etc
│   └── deeprotection
│       ├── config.toml
│       └── plugins
│           └── example-plugin
│               ├── plugin.json
│               └── main
├── usr
│   └── bin
│       └── dp
└── var
    └── log
        └── audit.log
```

## 🔬Technical Details

You can refer to the architecture design of this project in the [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/blob/main/ARCHITECTURE.md) file.

**Key Architecture Highlights:**

- **Modular Design**: Clear separation of concerns with dedicated modules for rules, plugins, path protection, and logging.
- **Configuration-Driven**: All security policies are defined in TOML configuration files.
- **Fail-Open Philosophy**: Plugin failures default to allowing the original command with a warning.
- **Thread-Safe Logging**: Mutex-protected file writes ensure safe concurrent access.

**Core Dependencies:**

| Crate        | Purpose                                      |
| ------------ | -------------------------------------------- |
| chrono       | Timestamp generation                         |
| rustyline    | Command-line interface and history           |
| regex        | Pattern matching for rules                   |
| anyhow       | Error handling                               |
| serde/toml   | Configuration parsing                        |
| sha2         | Password hash verification                   |
| rpassword    | Secure password input                        |
| walkdir      | Directory traversal for plugin discovery     |

## 📃Contributors List

Thank you to all developers who have contributed to this project. You can view all contributors to this project in the [CONTRIBUTORS](https://github.com/Geekstrange/Deeprotection/tree/main/CONTRIBUTORS) directory.

## ⚖License

<div style="display: inline-flex; align-items: center; gap: 0px; vertical-align: middle;">

<a href="https://www.mozilla.org/en-US/MPL/2.0/" target="_blank">

<img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/Godzilla.gif" alt="MPL 2.0" style="width: 300px; height: auto; display: block;"/>

</a>

</div>

This project is licensed under the Mozilla Public License Version 2.0 (MPL 2.0). You may freely use, copy, distribute, and modify this project, as well as create derivative works based on it, provided you comply with the core terms outlined in the LICENSE file.

## ⭐Acknowledgements

**Below are the indispensable dependencies of this project**

**Listed in alphabetical order, no ranking implied**

**Rust Toolchain**: For providing the memory-safe and highly performant foundation of the refactored engine.

**Rustyline**: For powering the robust command-line interface, history management, and autocompletion features.

**Regex Crate**: For enabling efficient pattern matching in the rule engine.

**SHA2 & rpassword**: For secure password authentication in enforcing mode.