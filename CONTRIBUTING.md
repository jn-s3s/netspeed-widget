# Contributing

## Setup and checks

Use Windows with the stable Rust toolchain pinned in `rust-toolchain.toml` (MSVC),
Visual Studio C++ build tools and the Windows SDK. Rustup installs the compiler,
rustfmt and Clippy versions used in CI. Start the widget with `cargo run --locked`
and format changes with `cargo fmt --all`. Before opening a pull request, run:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
$env:RUSTDOCFLAGS = '-D warnings'
cargo doc --locked --workspace --no-deps
cargo build --release --locked
```

Quit any running widget before tests: the lifecycle regression launches a real
copy with isolated `APPDATA`. Tests must not depend on public Internet access or
write to the user's real settings. Keep `Cargo.lock` committed for reproducible
application builds.

`Cargo.toml` declares the MSRV, tested separately in CI. To check it locally,
install that version with `rustup toolchain install <rust-version> --profile minimal`
and run `cargo +<rust-version> test --locked --workspace`.

Verifying the window layer requires a developer desktop with Explorer running.
Without `Shell_TrayWnd` (as on headless CI runners), the lifecycle suite skips all
window-layer assertions and emits a CI warning. A green CI check alone does not
verify taskbar parenting, input, popup stacking, recovery or shutdown. Run
`cargo test --locked --test windows_lifecycle -- --show-output` on a desktop
before accepting changes to these behaviors.

## Rendering checks

Check the widget at 100%, 125%, 150% and 200% display scaling, including moving it
between monitors. Compare native-resolution captures in each layout with Default,
Dark and Light themes, graph fill on/off, and healthy/slow/failed status. Check
small units, arrows, one-pixel side borders, graph joins and missing-data gaps.
Also check text in the details panel and shortcut dialog. The transparent widget
uses grayscale masks; opaque native controls follow system font smoothing.

Run the manual paint benchmark in an optimized build:

```powershell
cargo test --release --locked --bin netspeed-widget rendering_benchmark -- --ignored --nocapture
```

It reports median paint time for warmed font caches, with empty and active graph
history at four DPIs. It excludes window presentation, sampling and worker CPU;
it is not an end-to-end CPU or memory benchmark. Compare on the same machine
without other builds running. Keep refresh frequency unchanged when comparing.

## Visual Studio Code

Open the repository folder and use **Extensions: Show Recommended Extensions**
to install the suggestions from `.vscode/extensions.json`:

- **rust-analyzer** (`rust-lang.rust-analyzer`): Rust navigation, completion,
  rustfmt formatting and Clippy diagnostics.
- **Even Better TOML** (`tamasfe.even-better-toml`): Cargo and toolchain TOML support.
- **GitHub Actions** (`GitHub.vscode-github-actions`): workflow editing and validation.
- **CodeLLDB** (`vadimcn.vscode-lldb`): optional Rust debugging with F5.

Workspace settings use four-space Rust indentation, a 100-column ruler and
format-on-save through rustfmt. Rust Analyzer runs Clippy with warnings denied;
neither overrides the repository's formatter or toolchain configuration.
Run **Tasks: Run Task → Rust: Check conventions** for the same formatting,
lint, test and documentation checks as CI. Ctrl+Shift+B builds the debug binary.

The F5 configuration builds through Cargo and stores debug settings and logs in
`target/vscode-appdata`, separate from normal user settings. Quit the installed
widget before debugging or running lifecycle tests: the single-instance mutex
still applies. Windows C++ build tools and the SDK are required regardless of
which extensions are installed.

## Conventions

- Follow the Rust Style Guide and Rust API Guidelines, preserving the existing
  edition and MSRV. Use four spaces and rustfmt's 100-character default width;
  `rustfmt.toml` records these requirements. Tool configuration follows these
  conventions, not the reverse.
- Use idiomatic names, borrowed views where ownership is unnecessary, and private
  implementation details. Prefer `name()` to `get_name()` for getters.
- Propagate recoverable errors or handle them at a reporting boundary. Explain
  intentional result discards; reserve panics for proven invariants and tests.
- Document public APIs and unsafe preconditions. Use explicit unsafe blocks even
  inside unsafe functions, with a `SAFETY` comment explaining each invariant.
- Write docs and doc comments in plain language. Explain behavior, units and
  failure cases; check claims against the code. Preserve attributed standards
  and historical release notes when updating current documentation.
- Start with default Clippy lints. Keep any extra lints deliberate and any
  suppressions narrow, reasoned and preferably expressed with `#[expect]`.
- Keep application state and widget window calls on the UI thread. Measurement
  workers publish snapshots or post completion messages. The occupancy worker
  queries taskbar windows and accessibility providers off-thread so an
  unresponsive provider cannot block the UI.
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
and opens a draft release with changelog notes, a standalone `.exe`, and a ZIP
containing the executable, README and license. A maintainer publishes it manually.
No registry publication is configured.

Report bugs in GitHub issues. Report vulnerabilities privately as described in
[SECURITY.md](SECURITY.md). Contributions use the project's MIT license.
