# Security Policy

Deeprotection LTS is a security-hardened shell (`dp`, v2.1.0-lts) built for
stable, production, and enterprise environments. We take security seriously and
appreciate responsible disclosure from the community. Thank you for helping keep
Deeprotection and its users safe.

> **LTS notice:** this document belongs to the **lts** (Long-Term Support)
> branch, v2.1.0-lts. LTS is the recommended edition for production and
> enterprise use, and receives **backported security fixes**.

## Supported Versions

| Version | Branch   | Supported                      | Notes                              |
|---------|----------|--------------------------------|------------------------------------|
| 2.1.x   | `lts`    | ✅ Security fixes (backported) | **Recommended for production**     |
| 2.0.x   | `main`   | ✅ Security fixes              | Active development branch          |
| 2.0.x   | `canary` | ⚠️ Best-effort                 | Experimental; may lag              |

Only the versions listed above receive security fixes. Older snapshots and
unreleased local modifications are not supported.

## Reporting a Vulnerability

**Please do NOT report security vulnerabilities through public GitHub issues,
pull requests, or discussions.**

Use GitHub's **private vulnerability reporting**:

👉 https://github.com/Geekstrange/Deeprotection/security/advisories/new

If that channel is unavailable to you, open a *minimal* public issue **without
any vulnerability details** and ask a maintainer to arrange a private channel.

Please include as much of the following as possible:

- Affected LTS version and commit hash.
- Operating system and kernel version.
- The active protection `mode` (`enforcing` / `permissive` / `disable`).
- A minimal proof-of-concept: exact command(s), relevant `config.toml`, and the
  observed vs. expected result.
- Impact assessment (e.g. sandbox bypass, privilege escalation, DoS).
- Relevant audit-log lines, with secrets redacted.

### What to expect

- Because LTS is production-facing, we aim to acknowledge reports within
  **2 business days** (best effort).
- We will confirm the issue, prepare a backported fix, and coordinate a
  disclosure date with you — usually within **90 days**.
- Please give us reasonable time to ship a fix before any public disclosure.
- With your permission, we will credit you in the release notes and the
  published advisory. CVE IDs can be requested through the GitHub advisory.

## Security Model & Scope

Deeprotection is **defense-in-depth**, not a sandbox or a jail. It reduces risk
from command misuse but does **not** replace OS-level isolation (containers,
seccomp, SELinux/AppArmor, dedicated users). Two design constraints matter:

- Running the shell **as root/admin defeats the model** — there is no protection
  against a privileged user.
- **Plugins and the config file are trusted input.** A plugin runs with the
  shell's privileges; a malicious or misconfigured `config.toml` is out of scope.

### In scope

- Command-interception bypass: a command that rules/plugins should block ends up
  executing.
- Path-protection bypass: `../` traversal, symlink redirects, glob-through-symlink,
  `--option=VALUE`, or `key=value` forms evading protected-path checks.
- Environment-sanitization bypass (`LD_PRELOAD`, `LD_LIBRARY_PATH`, `PYTHONPATH`,
  `IFS`, etc.).
- Password-authentication bypass or flaws in the SHA-256 verification path.
- Plugin-system flaws: timeout/kill bypass, zombie handling, or command injection
  via crafted replacement strings (newlines / NUL bytes).
- Audit-log integrity: injection, tampering, or silent loss of security events.
- Parser/executor bugs leading to arbitrary code execution beyond documented
  shell semantics, or fork-bomb-limiter bypass leading to DoS.
- Fail-open behavior in `enforcing` mode where a check should fail closed.

### Out of scope

- Running as root, or the shell being attacked by an already-privileged process.
- Malicious plugins, a malicious `config.toml`, or a compromised `~/.bashrc`
  (loaded by `bash_compat`).
- Features intentionally **not** implemented (subshells `(...)`, arrays,
  `set -e` / `set -x`, `[[ ]]`, process substitution, etc.) — please file those
  as bugs, not vulnerabilities.
- Generic denial-of-service not caused by a bypass of the built-in limits.
- Vulnerabilities in third-party dependencies that are not reachable through
  Deeprotection (report those upstream; reachable ones are in scope).
- Windows-specific behavior — Deeprotection is Unix/Linux-only and does not
  support Windows.

## Disclosure Policy

We follow **coordinated disclosure**: we work privately with you, publish a
GitHub security advisory when a fix is available, and credit reporters who wish
to be acknowledged.

Thank you for helping make Deeprotection LTS more secure. 🙏

