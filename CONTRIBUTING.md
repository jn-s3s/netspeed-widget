# Contributing

Thanks for your interest in improving NetSpeed Widget. This document covers setup and the conventions this repository follows.

## Setup

```powershell
python -m venv .venv
.venv\Scripts\pip install -r requirements.txt
.venv\Scripts\pip install -r requirements-dev.txt
.venv\Scripts\python app.py
```

## Conventions

- Lint and formatting use Ruff, configured in `ruff.toml` (88 character lines, double quotes).
- All UI work happens on the Tk main thread. Background threads must marshal UI calls through `ui_call`.
- Catch specific exception types. An intentional swallow needs a short comment explaining why.
- Public functions and classes carry a docstring covering purpose, arguments, return value and failure modes.

## Before opening a pull request

- Run `.\lint.ps1` (or `ruff check .` and `ruff format --check .`). Both must exit clean; CI enforces this on every push and pull request.
- Run the test suite (`python -m pytest tests -q`). CI enforces this on every push and pull request; the suite covers hotkey parsing, speedtest JSON parsing, sampler rate math and display formatting without needing a display.
- Follow [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <summary>` with a lowercase imperative summary under 69 characters.
- Keep each pull request focused on one concern.
- By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Reporting bugs and security issues

Bug reports go through GitHub issues. Security vulnerabilities must not be opened as public issues; see [SECURITY.md](SECURITY.md) instead.

## Licensing

By contributing you agree that your contributions will be licensed under the MIT License that covers this project.
