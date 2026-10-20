# Contributing

## Setup and checks

Use Windows with stable Rust (MSVC), Visual Studio C++ build tools and the Windows
SDK. Run `cargo run --locked` to start the widget. Before opening a pull request:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
cargo build --release --locked
```

Quit any running widget before tests: the lifecycle regression launches a real
copy with isolated `APPDATA`. Tests must not depend on public Internet access or
write to the user's real settings. Keep `Cargo.lock` committed for reproducible
application builds.

Verifying the window layer requires a developer desktop with Explorer running.
Without `Shell_TrayWnd` (as on headless CI runners), the lifecycle suite skips all
window-layer assertions and emits a CI warning. A green CI check alone does not
verify taskbar parenting, input, popup stacking, recovery or shutdown. Run
`cargo test --locked --test windows_lifecycle -- --show-output` on a desktop
before accepting changes to these behaviors.

## Conventions

- Keep all window calls and application state on the UI thread. Workers publish
  results through snapshots or posted messages, never by calling UI functions.
- Only embed the widget as a child of a taskbar window. Never attach its input
  queue to Explorer's input queue.
- Win32 calls can synchronously re-enter window procedures. Do not hold state
  locks across calls that invoke handlers which acquire the same lock.
- Release GDI objects only after they are deselected or their DC is destroyed.
- Use `crate::logger` for subsystem-tagged logging and document unsafe invariants.
- Keep tests deterministic, add regression coverage, and tolerate invalid settings.
- Follow Conventional Commits: `<type>(<scope>): <summary>`.
- Keep pull requests focused; participation follows [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## Branches and releases

`dev` is the integration branch; `main` is production. Both run the
Lint, format and test status check, backed by shared Rust checks in
`.github/actions/rust-checks/action.yml`.

Pull requests into `main` also require Changelog gate: leave
`[Unreleased]` empty and prepare a new dated version matching `package.version`
in `Cargo.toml`. Do not fabricate a release entry just to pass the gate.
Prepare the current development version's dated release changelog before merging
to `main`. Documentation and release notes should describe the
current version's features, controls and requirements.

`Cargo.toml` owns the executable/resource version and product strings. The
`app.manifest` assembly version is a fixed Windows assembly identity, not the
release version; no application update or compatibility decision uses it.

Merging to `main` runs the tag workflow. The `RELEASE_PAT` secret is
the dedicated identity for pushing protected `v*` tags. A tag triggers the release
workflow, which validates the version, runs Rust checks, builds an x64 executable,
and opens a draft release with changelog notes. A maintainer publishes it manually.
No registry publication is configured.

Report bugs in GitHub issues. Report vulnerabilities privately as described in
[SECURITY.md](SECURITY.md). Contributions use the project's MIT license.
