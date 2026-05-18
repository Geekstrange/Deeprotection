# Deeprotection

Deeprotection is a high-performance, fully-featured shell environment (`dpshell`) written in Rust. It provides a hand-written recursive-descent parser, a direct `fork/execve` executor (no `/bin/sh` wrapper), POSIX-compatible control structures, 50+ built-in commands, job control, and rich interactive features. On top of this shell core, it layers rule-based command matching, plugin extensibility, path protection with symlink-aware auditing, JSONL audit logging, and SHA-256 password authentication. It offers three operation strategies: Enforcing, Permissive, and Disable modes.

<p align="center">
  <a href="https://github.com/Geekstrange/Deeprotection">
    <img src="https://github.com/Geekstrange/Deeprotection/blob/main/images/logo.svg" alt="logo" width="120" height="120">
  </a>
  <h5 align="center">: ) Hello, thank you for using!⭐</h5>
  <p align="center">
    <br />
    <strong>道阻且长,行则将至 行而不辍,未来可期</strong>
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

> [!WARNING]
> dpshell is under active development with unstable features. It is **NOT** recommended to use it as the default login shell in production environments.

> [!CAUTION]
> Security restrictions such as fork bomb prevention cannot provide full protection. Do not execute untrusted scripts or unknown commands in risky environments.

> [!IMPORTANT]
> Certain configuration options like `bash_compat` will load user profile scripts (`~/.bashrc`) automatically. Please ensure all sourced scripts come from trusted sources.

> [!NOTE]
> dpshell **does not source `/etc/profile` or `~/.bash_profile` during login**. If your workflow depends on login profile scripts, **use bash or brush instead**.

------

## 📜Table of Contents

- [🔍User Guide](#user-guide)
- [🕹Basic Usage](#basic-usage)
- [🚀Quick Start](#quick-start)
- [🛡️Protection Modes](#protection-modes)
- [🛠Configuration File Introduction](#configuration-file-introduction)
- [📌Log Introduction](#log-introduction)
- [🧩Plugin Architecture](#plugin-architecture)
- [📂Installation Directory](#installation-directory)
- [🔧Complete Feature List](#complete-feature-list)
- [📋Built-in Commands Inventory](#built-in-commands-inventory)
- [📝Script Execution](#script-execution)
- [🔬Technical Details](#technical-details)
- [⚠️Known Limitations](#known-limitations)
- [🤝Contributing](#contributing)
- [📃Contributors List](#contributors-list)
- [⚖License](#license)
- [⭐Acknowledgements](#acknowledgements)
- [🦀Rust Shell Ecosystem](#rust-shell-ecosystem)

## 🔍User Guide

### 🕹Basic Usage

**A Modern Shell Experience**

`dpshell` offers a native shell with real-time syntax highlighting, grey‑text autosuggestions from history, and smart tab completion (fuzzy matching or traditional bash-style). These fish‑style features are enabled by default and can be toggled in the configuration file (see `[features]`).

Inside the shell you can run pipelines, chain commands with `;`, `&&`, `||`, send jobs to the background with `&`, define shell functions, use control structures (`if/for/while/case`), and write POSIX-compatible scripts. All parsing and execution is done directly by `dpshell` via a hand-written recursive-descent parser, so security rules apply uniformly to every command.

**Interactive History & Completion**

- **Syntax highlighting** – commands, builtins, strings, flags, operators, and comments are colour‑coded as you type. Valid executables are distinguished from unknown commands.
- **Autosuggestions** – a grey ghost text from history appears; press `Right` or `Ctrl+F` to accept.
- **Tab completion** – two modes available:
  - **Enhanced** (default) – fuzzy matching powered by `nucleo` with columnar menu display.
  - **Bash-style** – traditional prefix-based completion. Toggle via `enhance_completion` in config.

**Enhanced `cd` Command**

In `dpshell`, the `cd` command comes with interactive built‑ins to make terminal navigation cleaner and faster.

**Interactive Single-Level Navigation:** Entering `cd ?` allows you to view numbered subdirectories and input a number to enter the corresponding directory.

```shell
dpshell(1)# cd ?
1) amd64
2) arm64
3) debug
4) test_space
Select directory (enter q to quit):
```

**Recursive Navigation:** Entering `cd ??` enables a recursive directory browser, allowing you to traverse up and down the file tree interactively.

```shell
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

**Bash Compatibility Mode**

Set `bash_compat = true` in the `[core]` section to enable:
- Reading `~/.bashrc` at startup (simple lines such as exports, aliases, and variable assignments).
- Persistent command history in `~/.bash_history` (shared with bash).
- Startup animation is skipped for a cleaner experience.

**Login Shell Support**

`dpshell` can be used as a login shell. When invoked with `-l`, `--login`, or with a leading `-` in `argv[0]` (as `login(1)` does), the `is_login` flag is set but **no profile files are sourced** (`/etc/profile`, `~/.bash_profile`, and `~/.profile` are intentionally not loaded). This avoids startup errors caused by non-POSIX constructs in system profiles when dpshell is used as a login shell on appliance or embedded systems.

### 🚀Quick Start

**Build from Source**

```shell
# Clone the repository
git clone https://github.com/Geekstrange/Deeprotection.git
cd Deeprotection

# Build release binary
cargo build --release

# The binary is at target/release/dpshell
```

**Install**

```shell
# Copy binary to system path
sudo cp target/release/dpshell /usr/bin/dp

# Create configuration directory and default config
sudo mkdir -p /etc/deeprotection/plugins
sudo cp config.toml /etc/deeprotection/config.toml

# Create log directory
sudo mkdir -p /var/log

# (Optional) Register as a valid login shell
echo "/usr/bin/dp" | sudo tee -a /etc/shells
```

**Run**

```shell
# Start interactively
dp

# Run a script
dp script.sh

# Run an inline command
dp -c 'echo hello && echo world'

# Start as login shell
dp -l
```

### 🛡️ Protection Modes

Deeprotection operates in one of three modes, defined in your configuration file.

**Disable Mode**

Commands pass through without modification or blocking. No rules, plugins, or path protection are applied. Activity is logged to the audit file only for security-relevant events (e.g., blocked commands in other modes).

**Permissive Mode**

Commands are evaluated against your defined `[[rules]]` and any active plugins. Path protection is **ignored**. This is excellent for testing rule logic.

**Enforcing Mode**

Strict security. Commands are evaluated against rules, plugins, and finally, the path protection engine (including a post‑glob expansion audit). Operations involving commands in the `allowlist` targeting protected directories will require password authentication. Commands not in the `allowlist` will be blocked immediately.

```shell
dpshell(1)# ls test/
[!] Protected path operation requires authorization.
Admin password:
```

**Password Authentication**

In `enforcing` mode, when operating on protected paths with allowlisted commands, you will be prompted for password authentication (up to 3 attempts). The password is verified using SHA‑256 against the hash stored in the configuration file.

### 🛠Configuration File Introduction

Deeprotection uses a clean, minimalist TOML configuration file located at `/etc/deeprotection/config.toml`.

```toml
[core]
# Operating mode: "disable", "permissive", or "enforcing"
mode = "enforcing"

# Enable Bash compatibility: sources ~/.bashrc, uses ~/.bash_history
bash_compat = false

# Enable live config reloading (checks file content before each prompt)
dynamic_config = true

[auth]
# SHA-256 hex digest of admin password (generate with: echo -n "pass" | sha256sum)
password_hash = "31fc7f00f4a0f72653d3ba5f445b8c21d922ae786da3f0a3a780f573942d00aa"

[paths]
# Directories that are strictly protected against modification commands
protect = ["/root/test", "/root/.ssh"]

# Commands allowed to operate on protected paths (requires authentication)
allowlist = ["rm", "rmdir", "mv", "cp", "chmod", "chown", "touch", "cat", "ls"]

[features]
# Enable/disable fish-style interactive helpers (all default to true)
syntax_highlighting = true    # Colour-code commands, builtins, strings, operators
auto_suggest        = true    # Grey ghost-text suggestions from history
enhance_completion  = true    # Fuzzy (nucleo) completion; false = bash-style prefix

# ---------------------User Rules---------------------

[[rules]]
name = "block_rm_rf"
pattern = "rm -rf"
action = { block = true }
enabled = true

[[rules]]
name = "block_fork_bomb"
pattern = "re:^\\s*:\\(\\)\\s*\\{.*\\|.*&.*\\}.*;"
action = { block = true }
enabled = true

[[rules]]
name = "replace_echo"
pattern = "re:^echo 111$"
action = { replace = "echo 222" }
enabled = true
```

**Configuration Options Reference:**

| Section | Key | Type | Default | Description |
|---|---|---|---|---|
| `[core]` | `mode` | string | `"permissive"` | `disable`, `permissive`, or `enforcing` |
| `[core]` | `bash_compat` | bool | `false` | Source `~/.bashrc`, use `~/.bash_history` |
| `[core]` | `dynamic_config` | bool | `true` | Reload config before each prompt (no background thread) |
| `[auth]` | `password_hash` | string | — | SHA-256 hex digest for enforcing mode authentication |
| `[paths]` | `protect` | string[] | `[]` | Absolute directory prefixes to protect |
| `[paths]` | `allowlist` | string[] | `[]` | Command names permitted to touch protected paths (with auth) |
| `[features]` | `syntax_highlighting` | bool | `true` | Enable real-time syntax colouring |
| `[features]` | `auto_suggest` | bool | `true` | Enable history-based ghost-text suggestions |
| `[features]` | `enhance_completion` | bool | `true` | `true` = fuzzy/nucleo completion; `false` = bash-style prefix |

**Rule Pattern Types:**

- **Plain string**: Automatically converted to an anchored regex that allows flexible whitespace (e.g., `"rm -rf"` becomes `^\s*rm\s+-rf\s*$`).
- **Explicit regex**: Prefixed with `re:` (e.g., `"re:^echo 111$"`).
- **Command name match**: `cmd:rm` matches if the command name is `rm`.
- **Argument regex**: `arg:\.\.` matches if any argument matches the regex.

**Rule Actions:**

- `block = true`: Block the command and log the action.
- `replace = "new command"`: Replace the command with the specified string.

**Dynamic Config Reload:**

When `dynamic_config = true`, dpshell reads `/etc/deeprotection/config.toml` before processing each command. If the file content has changed, it reloads all settings: mode, rules, paths, allowlist, password hash, and feature flags. The editor is rebuilt if feature flags change. A `dpshell: config reloaded` message is printed to stderr on successful reload.

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

## 🔧Complete Feature List

### Shell Core

- **Hand-written recursive-descent parser** producing a full AST with 11 node types
- **Direct `fork/execve` execution** — no `sh -c` wrapper; eliminates shell injection vectors
- **Pipelines** — N-stage pipe(2) with process group management
- **Logical operators** — `&&`, `||`, `;` with exit-code-driven evaluation
- **Background execution** — `&` with process group isolation
- **I/O redirection** — 9 types: `>`, `>>`, `<`, `<&`, `>&`, `<>`, `>|`, `<<` (heredoc), `<<-` (tab-stripping heredoc)
- **Command substitution** — `$(cmd)` with fork/pipe/waitpid capture
- **Brace expansion** — `{a,b,c}`, `{1..5}` with checked arithmetic and overflow protection
- **Glob expansion** — `*.log`, `file?.txt`, `[abc]*` with 65,536 argument cap
- **Variable expansion** — `$VAR`, `${VAR}`, `${VAR:-default}`, `${VAR:+word}`, `${VAR:=word}`, `${VAR%pat}`, `${VAR%%pat}`, `${VAR#pat}`, `${VAR##pat}`
- **Arithmetic expansion** — `$((expr))` with `+`, `-`, `*`, `/`, `%`, comparisons, and variable lookup
- **Positional parameters** — `$1`–`$9`, `$#`, `$@`, `$*` in functions
- **Alias expansion** — user-defined command aliases
- **Shell functions** — `name() { body }` with pre-parsed AST, positional parameter threading
- **Heredocs** — `<<DELIM` and `<<-DELIM` with temp-file substitution, multi-heredoc support
- **Multi-line input** — automatic continuation for unclosed `if/for/while/case`, trailing `|`/`&&`/`||`

### Control Structures (POSIX)

- `if/elif/else/fi` — with full nesting support
- `for var in words; do..done` — word list expansion at execution time
- `while condition; do..done` / `until condition; do..done` — with stdin redirection
- `case word in pattern) ..;; esac` — fnmatch-style glob patterns (`*`, `?`, `[...]`)
- Compound commands `{ ...; }` — supported as pipeline stages

### Interactive Features

- **Syntax highlighting** — token-level colouring: commands (PATH-resolved), builtins, arguments, flags, strings, operators, comments
- **Autosuggestions** — fish-style grey ghost-text from command history
- **Dual-mode tab completion** — fuzzy (nucleo) or bash-style prefix matching
- **Line editing** — full cursor navigation, word/character deletion, clipboard, undo (via reedline)
- **History search** — `Ctrl+R` reverse incremental search
- **Job control** — `fg`, `bg`, `jobs`, `Ctrl+Z` suspend, background job completion notices
- **Interactive `cd`** — `cd ?` (single-level browser), `cd ??` (recursive browser)
- **Signal handling** — `Ctrl+C` (interrupt), `Ctrl+D` (EOF/exit), `Ctrl+L` (clear screen)

### Security

- **Three-layer command auditing** — raw-input regex → AST-level rule matching → post-expansion path audit
- **Fork-bomb protection** — rate limiter (64 forks/s), child limit (256), call depth limit (128)
- **Path protection** — symlink-aware canonicalization, `--option=VALUE` and `key=value` inspection
- **Plugin system** — external scripts with 5-second timeout, proper `SIGKILL` + zombie reap
- **Environment sanitization** — strips `LD_PRELOAD`, `LD_LIBRARY_PATH`, `PYTHONPATH`, `IFS`, and 7 other dangerous variables
- **JSONL audit logging** — every command logged with timestamp, user, mode, cwd, PID
- **SHA-256 password authentication** — 3-attempt limit for enforcing mode operations

### Configuration

- **Centralized TOML config** — `/etc/deeprotection/config.toml`
- **Dynamic config reload** — race-free content comparison on each prompt (no background thread)
- **Independent feature toggles** — syntax highlighting, autosuggestions, completion mode
- **Bash compatibility mode** — `~/.bashrc` sourcing, `~/.bash_history` persistence

### Script Execution

- **Script file execution** — `dp script.sh [args...]`
- **Inline commands** — `dp -c 'command string'`
- **Shebang support** — `#!/usr/bin/dp`
- **Login shell mode** — `-l` / `--login` flag (no profile sourcing)
- **Multi-line block joining** — automatic joining of `if/for/while/case/function` bodies

## 📋Built-in Commands Inventory

dpshell provides 50+ built-in commands organized by category:

### Navigation & Directories

| Command | Description |
|---|---|
| `cd` | Change directory (supports `cd ?` and `cd ??` interactive modes) |
| `pwd` | Print working directory |
| `pushd` | Push directory onto stack |
| `popd` | Pop directory from stack |
| `dirs` | Display directory stack |

### Variables & Environment

| Command | Description |
|---|---|
| `export` | Set environment variables |
| `unset` | Remove variables |
| `readonly` | Mark variables as read-only |
| `local` | Declare function-local variables |
| `declare` / `typeset` | Declare variables with attributes |
| `let` | Evaluate arithmetic expressions |
| `set` | Set/unset shell options and positional parameters |

### I/O & Text

| Command | Description |
|---|---|
| `echo` | Print arguments to stdout |
| `printf` | Formatted output |
| `read` | Read input with multi-variable word splitting |
| `mapfile` / `readarray` | Read lines into an array variable |

### Flow Control

| Command | Description |
|---|---|
| `break` | Exit from a loop |
| `continue` | Skip to next loop iteration |
| `return` | Return from a function |
| `shift` | Shift positional parameters |
| `exit` / `logout` | Exit the shell (enforcing mode requires authentication) |
| `trap` | Set signal handlers |
| `wait` | Wait for background processes |

### Job Control

| Command | Description |
|---|---|
| `jobs` | List background/stopped jobs |
| `fg` | Bring job to foreground |
| `bg` | Resume job in background |
| `kill` | Send signals to processes |
| `suspend` | Suspend the shell |

### Shell Management

| Command | Description |
|---|---|
| `alias` / `unalias` | Define/remove command aliases |
| `history` | Display command history |
| `source` / `.` | Execute commands from a file in the current shell |
| `eval` | Evaluate a string as a command |
| `exec` | Replace shell with command |
| `command` | Run command bypassing functions (`command -v` for lookup) |
| `builtin` | Run a builtin bypassing functions |
| `type` | Show how a command name would be interpreted |
| `help` | Display help for builtins |
| `hash` | Manage the command hash table |
| `enable` | Enable/disable builtins |

### Test & Logic

| Command | Description |
|---|---|
| `test` / `[` | Evaluate conditional expressions |
| `true` | Return success (exit 0) |
| `false` | Return failure (exit 1) |

### Configuration & System

| Command | Description |
|---|---|
| `shopt` | Set/unset shell options |
| `ulimit` | Get/set resource limits |
| `umask` | Set file creation mask |
| `times` | Print accumulated user and system times |
| `caller` | Return the context of the current subroutine call |
| `fc` | Fix command — list/edit/re-execute history entries |
| `getopts` | Parse positional parameters |
| `complete` / `compgen` / `compopt` | Programmable completion control |
| `bind` | Display/modify key bindings |

### Special

| Command | Description |
|---|---|
| `:` | No-op (always returns 0) |

## 📝Script Execution

dpshell supports non-interactive script execution with POSIX-compatible syntax.

**Running a script file:**

```shell
dp script.sh arg1 arg2
```

**Running an inline command:**

```shell
dp -c 'for i in 1 2 3; do echo $i; done'
```

**Script features:**
- Shebang declarations (`#!/usr/bin/dp`)
- Positional argument passing (`$1`, `$2`, ..., `$#`, `$@`)
- Environment variable inheritance
- Heredoc preprocessing (`<<DELIM` and `<<-DELIM`)
- Multi-line block joining for `if/for/while/case/function` bodies
- Full security pipeline in permissive/enforcing modes
- Exit code propagation via `$?`

**POSIX compatibility:** dpshell passes a 14-test POSIX validation suite covering variable expansion, control structures, functions, pipelines, heredocs, case patterns, and external tool integration.

## 🔬Technical Details

You can refer to the architecture design of this project in the [ARCHITECTURE.md](https://github.com/Geekstrange/Deeprotection/blob/main/ARCHITECTURE.md) file.

**Key Architecture Highlights:**

- **No external shell**: `dpshell` parses and executes commands directly using a hand-written recursive-descent parser producing a full AST with 11 node types (Simple, Pipeline, Logical, Background, Compound, FunctionDef, If, For, While, Until, Case).
- **Direct fork/execve**: All external commands are executed via `fork/execve` — no `sh -c` wrapper. This eliminates an entire class of shell injection vulnerabilities.
- **Job Control**: Built‑in `fg`, `bg`, `jobs` with full POSIX process‑group management including `tcsetpgrp` terminal ownership transfer.
- **Brace & Glob Expansion**: `{1..3}`, `*.log`, etc. performed in the parent process before fork, with checked arithmetic, overflow protection, and a 65,536 argument cap.
- **Multi‑layer Security**: Raw input regex check → AST‑based rule matching → Plugin pipeline → Path protection (including post‑expansion symlink-aware audit).
- **Fail‑Closed in Enforcing Mode**: If the working directory cannot be determined, protected‑path checks block the command rather than allowing it.
- **Fork-Bomb Protection**: Built-in rate limiter (64 forks/s, 256 child limit, 128 call depth) in the executor.
- **Interactive Features**: Syntax highlighting, history autosuggestions, and dual-mode tab completion powered by `reedline` and `nucleo`.
- **Configuration‑Driven**: All security policies and feature toggles are defined in TOML configuration with optional live dynamic reloading.
- **Thread‑Safe Logging**: Mutex‑protected JSONL file writes ensure safe concurrent access.
- **Environment Sanitization**: Strips 11 dangerous variables (`LD_PRELOAD`, `LD_LIBRARY_PATH`, `PYTHONPATH`, `IFS`, etc.) from all child processes.

**Core Dependencies:**

| Crate | Purpose |
|---|---|
| `reedline` 0.35 | Modern line editor with highlighting, hints, and menus |
| `nucleo` 0.5 | High‑performance fuzzy matching for completions |
| `nu-ansi-term` 0.50 | ANSI terminal styling for the highlighter |
| `regex` 1.10 | Pattern matching for security rules |
| `anyhow` 1.0 | Error handling |
| `thiserror` 2.0 | Derive macro for custom error types |
| `serde` / `toml` | Configuration and log serialization |
| `serde_json` 1.0 | JSONL audit log serialization |
| `sha2` 0.10 | SHA-256 password hash verification |
| `rpassword` 7 | Secure password input (no terminal echo) |
| `nix` 0.29 | Unix system calls (fork, signal, wait, setpgid) |
| `libc` 0.2 | Low-level Unix API (dup2, tcsetpgrp, signal) |
| `glob` 0.3 | Filename globbing |
| `shlex` 1 | Shell lexing (tokenization) |
| `clap` 4.6 | Command-line argument parsing for builtins |
| `itertools` 0.14 | Iterator utilities |
| `chrono` 0.4 | Timestamp generation for audit logs |
| `terminal_size` 0.3 | Terminal width detection |
| `users` 0.11 | OS username lookup |
| `walkdir` 2.4 | Recursive directory traversal for `cd ??` |
| `ctrlc` 3 | Cross-platform Ctrl+C handler |

**Source Code Metrics:**

| Metric | Value |
|---|---|
| Total source files | 47 `.rs` files |
| Lines of code | ~10,883 |
| Direct dependencies | 25 crates |
| Built-in commands | 50+ |
| AST node types | 11 |
| Security check layers | 3 |
| I/O redirect types | 9 |

## ⚠️Known Limitations

The following features are not yet implemented or have known gaps:

- **Subshells `(...)`** — not supported as a control structure. Use `{ ...; }` for grouping.
- **Process substitution `<(cmd)`** — not implemented.
- **Extended test `[[ ]]`** — only `[ ]`/`test` is supported.
- **Arrays** — bash-style indexed and associative arrays are not available.
- **`set -e` (errexit)** — the flag is accepted but does not abort execution on errors.
- **`set -x` (xtrace)** — not implemented; no script debug tracing.
- **`${#VAR}` (string length)**, **`${VAR/pat/repl}`**, **`${VAR:offset:length}`** — not implemented.
- **Programmable completion** — `complete -F` function-based completion is not functional.
- **`PS1` prompt customization** — the prompt format is fixed (`dpshell(level)$`).
- **SIGHUP handling** — no cleanup on terminal hangup (temp files, history, background jobs).
- **Profile sourcing** — login shell mode does not source `/etc/profile` or `~/.bash_profile`; if needed, source them manually.
- **`.bashrc` sourcing** — skips lines containing `$(...)`, `[[`, `((`, multi-line blocks, and bash-only keywords (`shopt`, `complete`, `compopt`, `declare`, `typeset`, `local`, `let`, `select`, `function`).
- **History in default mode** — stored in `/tmp` and not persistent across reboots (use `bash_compat = true` for persistent history).
- **Log rotation** — audit log grows unboundedly; external `logrotate` integration required.
- **Per-user configuration** — config path is system-wide (`/etc/deeprotection/config.toml`); no per-user overrides.

## 🤝Contributing

**Build & Development:**

```shell
# Debug build
cargo build

# Release build
cargo build --release

# Type-check without building
cargo check

# Run clippy lints
cargo clippy

# Format code
cargo fmt
```

**Project Structure:**

```
src/
├── main.rs                    # REPL loop, script mode, login shell, config
├── utils.rs                   # Prompt generation, startup animation
├── parser/
│   ├── mod.rs                 # Shlex tokenization, PATH resolution, env sanitization
│   ├── syntax.rs              # Recursive-descent parser, AST definitions
│   └── expand_vars.rs         # Variable/arithmetic/alias expansion
├── executor/
│   ├── mod.rs                 # fork/execve, pipelines, function dispatch
│   └── expand.rs              # Brace and glob expansion
├── builtins/
│   ├── mod.rs                 # Module re-exports
│   ├── registry.rs            # Dispatch table (50+ commands)
│   └── *.rs                   # Per-command implementations (27 files)
├── interactive/
│   ├── mod.rs                 # Editor builder, feature flags
│   ├── highlighter.rs         # Token-level syntax highlighting
│   ├── hinter.rs              # Fish-style autosuggestions
│   ├── smart_completer.rs     # Fuzzy completion (nucleo)
│   └── bash_completer.rs      # Traditional prefix completion
├── security/
│   ├── mod.rs                 # Module re-exports
│   ├── rules.rs               # Rule engine (regex, command, argument matchers)
│   ├── plugins.rs             # External plugin system with timeout
│   └── protection.rs          # Path protection with symlink-aware auditing
├── shell/
│   └── mod.rs                 # DpShell state (history, aliases, vars, functions)
├── config/
│   └── mod.rs                 # TOML configuration deserialization
├── jobs/
│   └── mod.rs                 # POSIX job control (fg/bg/jobs)
└── logging/
    └── mod.rs                 # JSONL audit logger
```

**Guidelines:**

- No `sh -c` invocations — all execution must go through the AST and `fork/execve`.
- Security checks happen at two points: pre-fork (AST) and post-expansion (concrete paths).
- Built-ins are dispatched before the security pipeline since they run in-process.
- Match existing code style; the project uses no external formatter configuration beyond `cargo fmt`.
- There is no automated test suite currently; verify changes against `posix_test.sh` (14 tests).

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

**Below are the indispensable dependencies and inspirations of this project**

**Listed in alphabetical order, no ranking implied**

**[Rust Programming Language](https://www.rust-lang.org/)**: For providing the memory-safe, fearlessly concurrent, and highly performant foundation that makes a security-critical shell implementation viable.

**[brush-shell](https://github.com/reubeno/brush)**: For its excellent reference implementation of a POSIX-compatible shell in Rust. dpshell's parser logic, executor framework, and several built-in command implementations (echo, read, printf, pwd, and others) were informed by brush-shell's architecture. The brush-parser, brush-core, and brush-builtins crates served as invaluable reference material during development.

**[Reedline](https://github.com/nushell/reedline)**: For its excellent fish‑style line editor providing syntax highlighting, autosuggestions, completions, and menu frameworks. Reedline is the backbone of dpshell's interactive experience.

**[Nucleo](https://github.com/helix-editor/nucleo)**: For fast, fuzzy matching that powers the enhanced tab completion mode.

**[Nix Crate](https://github.com/nix-rust/nix)**: For safe, idiomatic Rust bindings to Unix process and signal APIs (`fork`, `execve`, `waitpid`, `setpgid`, `signal`).

**[Crossterm](https://github.com/crossterm-rs/crossterm)**: For cross-platform terminal manipulation, used as a transitive dependency through reedline.

**[Regex Crate](https://github.com/rust-lang/regex)**: For enabling efficient pattern matching in the security rule engine.

**[Clap](https://github.com/clap-rs/clap)**: For derive-based argument parsing used in several built-in command implementations.

**[SHA2 & rpassword](https://github.com/RustCrypto/hashes)**: For secure password authentication in enforcing mode — SHA-256 hashing and terminal-safe password input without echo.

**[Serde](https://github.com/serde-rs/serde) & [TOML](https://github.com/toml-rs/toml)**: For robust configuration deserialization and JSONL audit log serialization.

**[Glob](https://github.com/rust-lang/glob)**: For POSIX-compatible filename pattern matching in the expansion engine.

## 🦀 Rust Shell Ecosystem

If you are interested in shell implementations written in Rust, here are some outstanding projects worth exploring:

- **[Brush](https://github.com/reubeno/brush)** — A bash/POSIX‑compatible shell in Rust, combining script compatibility with modern interactive features.
- **[Nushell](https://github.com/nushell/nushell)** — A modern shell that treats data as structured tables, with a powerful pipeline language and rich built-in data processing capabilities.
- **[Ion](https://gitlab.redox-os.org/redox-os/ion)** — A fast, lightweight shell developed for the Redox OS project with a focus on simplicity and scripting performance.
- **[Murex](https://github.com/lmorg/murex)** — A typed, safety-conscious shell with inline spell-checking, smart autocompletion, and a rich set of built-in data manipulation tools.
- **[Rash](https://github.com/pka/rash)** — A minimal, embeddable shell scripting engine in Rust, useful for integrating shell-like scripting into Rust applications.
- **[Starship](https://github.com/starship/starship)** — Not a shell itself, but a blazing-fast cross-shell prompt written in Rust that works with any shell including dpshell.