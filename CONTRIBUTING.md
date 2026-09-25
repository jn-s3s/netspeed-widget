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

- Lint and formatting use Ruff, configured in `ruff.toml` (88 character lines, double quotes). `E501` is selected explicitly because the formatter cannot shorten a long string or comment.
- BLE001 stays enabled everywhere. A handler that genuinely must catch a broad `Exception` carries its own `# noqa: BLE001` with the reason; do not add a per-file ignore.
- Logging goes through `utils.logger.get("<subsystem>")`. Never hand-write a `[TAG]` prefix in a message string, and use `error()` for a failure the caller could not recover from rather than folding it into `warning()`.
- All UI work happens on the Tk main thread. Background threads marshal UI calls through `ui_call`, which is what drops a late call after shutdown instead of raising.
- The tray reaches the widget only through the `WidgetActions` protocol, never through `app.root`.
- Anything reading `config.json` must tolerate a wrong-typed value and return a default. The file is user-editable and is a trust boundary.
- A new dependency is added to `requirements.txt` with its exact version and wheel hashes, and `third_party/fast-bundle/package-lock.json` is regenerated in the same commit so a reviewer can see every transitive move.
- Public functions and classes carry a docstring covering purpose, arguments, return value and failure modes.
- Catch specific exception types. An intentional swallow needs a short comment explaining why.

## Before opening a pull request

- Run `.\lint.ps1` (or `ruff check .` and `ruff format --check .`). Both must exit clean; CI enforces this on every push and pull request.
- Run the test suite (`python -m pytest tests -q`). CI enforces this on every push and pull request; the suite covers hotkeys, the speedtest provider order and its fallback limits, sampler math and thread lifecycle, monitor geometry, speedtest scheduling, graph scaling, config load and save, logging, paths, tray routing and the widget's own interactions. Everything runs headless, and an autouse fixture keeps every test out of your real `%APPDATA%`.
- Follow [Conventional Commits](https://www.conventionalcommits.org/): `<type>(<scope>): <summary>` with a lowercase imperative summary under 69 characters.
- Keep each pull request focused on one concern.
- By participating, you agree to follow the [Code of Conduct](CODE_OF_CONDUCT.md).

## Releasing

The workflows in `.github/workflows` cut releases; no tag is pushed by hand.

- Every pull request into `main` must add a dated `## [X.Y.Z] - YYYY-MM-DD` section to `CHANGELOG.md` and leave `## [Unreleased]` empty. The new section should say what changed for someone running or building the app.
- `changelog-gate` in `ci.yml` enforces that contract: the unreleased section must be empty, the version must be newer than the latest tag and `APP_VERSION` in `utils/version.py` must match the newest dated changelog version. Bump the constant in the same pull request that adds the section. The job runs only on pull requests whose base is `main`, never on a push and never on a pull request into `dev`, so a broken changelog can still reach `dev`.
- `changelog-gate` must be configured as a required status check on `main`, otherwise a pull request that fails it can still be merged.
- Merging to `main` runs `tag-release.yml`, which reads the newest dated section, creates the `vX.Y.Z` tag and pushes it. That tag starts `release.yml`, which builds the app, packages the Windows x64 zip and opens a draft GitHub release whose notes are generated verbatim from the changelog section. A maintainer publishes the draft release by hand.
- The tag push authenticates with the `RELEASE_PAT` repository secret, a personal access token whose owner is a bypass actor on the tag ruleset. The default `GITHUB_TOKEN` cannot push `v*` tags and its pushes would not start `release.yml`, so a missing secret fails `tag-release.yml` before any tag is created.

## Reporting bugs and security issues

Bug reports go through GitHub issues. Security vulnerabilities must not be opened as public issues; see [SECURITY.md](SECURITY.md) instead.

## Licensing

By contributing you agree that your contributions will be licensed under the MIT License that covers this project.
