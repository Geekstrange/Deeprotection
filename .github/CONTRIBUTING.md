# Contributing to Deeprotection

First off, thank you for considering contributing to Deeprotection.  
By participating in this project, you agree to abide by our [Code of Conduct](https://github.com/Geekstrange/Deeprotection/blob/main/CODE_OF_CONDUCT.md).

## How Can I Contribute?

### Reporting Bugs or Suggesting Features

- Use the [issue tracker](https://github.com/Geekstrange/Deeprotection/issues) to report bugs or propose new features.
- Check for existing issues before creating a new one.
- Provide a clear description, steps to reproduce (for bugs), and any relevant logs or screenshots.

### Improving Documentation

- Documentation lives in the `README/` folder and the main `README.md`.
- Feel free to open a pull request with improvements or translations.

### Writing Code

We follow a few simple rules to keep the project healthy and maintainable.

## Development Setup

1. Fork the repository and clone it locally.
3. Make your changes in a dedicated branch.

## Commit Message Guidelines

**Deeprotection strictly follows [Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/).**  
All commit messages **must** be formatted as:

```
<type>(<scope>): <subject>

<body>

<footer>
```

### Allowed `<type>` values:

| Type       | Description                                             |
| ---------- | ------------------------------------------------------- |
| `feat`     | New feature                                             |
| `fix`      | Bug fix                                                 |
| `docs`     | Documentation only changes                              |
| `style`    | Code style changes (whitespace, formatting)             |
| `refactor` | Code change that neither fixes a bug nor adds a feature |
| `perf`     | Performance improvement                                 |
| `test`     | Adding or correcting tests                              |
| `chore`    | Changes to the build process or auxiliary tools         |

### `<scope>` (optional)

Use a scope to indicate the affected module (e.g., `dpshell`, `config`, `installer`, `logger`).

### `<subject>`

- Use the imperative, present tense: “add” not “added” nor “adds”
- Do not capitalise the first letter
- No dot at the end

### `<body>` (optional)

Explain **what** and **why** (not how). May include multiple paragraphs.

### `<footer>` (optional)

- Reference issues: `Closes #123, #456`
- **Must contain the DCO `Signed-off-by` trailer** (see below)

### Example

```
feat(dpshell): add recursive directory selection with cd ??

Implement interactive numbered menu for deeper navigation.
Allows users to traverse directories step by step.

Closes #42
Signed-off-by: Jane Doe <jane@example.com>
```

## Developer Certificate of Origin (DCO)

As stated in our [Code of Conduct](https://github.com/Geekstrange/Deeprotection/blob/main/CODE_OF_CONDUCT.md), **every commit must include a `Signed-off-by` line** in the commit message footer. By adding this line, you certify that you have the right to submit the contribution under the project’s open-source license (MPL 2.0).

Format:

```
Signed-off-by: Real Name <email@example.com>
```

You can automatically append this line by committing with `git commit -s`.

## Branching and Pull Requests

- **Base branch**: Always target `main` (or `develop` if specified).
- **Branch naming**: Use descriptive names like `feat/improve-cd`, `fix/rm-interception`.
- Keep your branch up to date with the base branch before opening a PR.
- **One logical change per PR** – avoid mixing unrelated changes.

## Testing

- For changes that affect `dpshell` or command interception, manually test in both **Permissive** and **Enhanced** modes.
- Verify that logs are written correctly to `/var/log/deeprotection.log`.

## Submitting a Pull Request

1. Push your branch to your fork.
2. Open a Pull Request against the main repository.
3. Fill in the **Pull Request template** (it will be pre‑loaded).
4. Ensure the PR title follows Conventional Commits (same rules as commits).
5. Request a review from the maintainers.

## License

By contributing, you agree that your contributions will be licensed under the [Mozilla Public License 2.0](./LICENSE).

---

Thank you for helping make Deeprotection more secure and reliable!