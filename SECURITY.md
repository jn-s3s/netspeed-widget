# Security Policy

Thank you for helping keep NetSpeed Widget and its users safe.

## Supported versions

Only the latest release receives security fixes. If you are running an older build, please update before reporting an issue.

| Version        | Supported |
| -------------- | --------- |
| Latest release | Yes       |
| Older releases | No        |

## How to report a vulnerability

Please use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/jn-s3s/netspeed-widget/security) of this repository.
2. Click **Report a vulnerability**.
3. Describe the issue, the steps to reproduce it and the impact you believe it has.

If private reporting is unavailable for any reason, open a minimal public issue asking for a private contact channel without revealing details.

## What to include

- The NetSpeed Widget version affected (the release exe filename includes it)
- Steps or a proof of concept showing how the issue manifests
- Which component is involved if you know it (speedtest subprocess chain, bundled Node runtime, config or log file handling, global hotkey registration, tray icon)

## What to expect

You will receive an acknowledgment as soon as the report is triaged. Fixes are developed privately where possible and released together with a patched version. You will be credited in the release notes unless you prefer to stay anonymous.

Please do not disclose the issue publicly until a fix has been released.

## Scope notes

NetSpeed Widget runs with your user privileges and stores no secrets: its only local state is a small config file and a log file under `%APPDATA%\NetSpeedWidget`. Reports are in scope when they show behavior beyond what the app legitimately does, such as:

- Command injection or argument smuggling through the speedtest chain (bundled node.exe and fast-cli, fast on PATH or speedtest-cli)
- Unsafe path handling when resolving bundled resources or the config and log files
- Tampering with the bundled Node runtime or fast-cli bundle so the widget executes unexpected code
- Unexpected outbound connections beyond the latency probes and speedtest traffic the app exists to make

The widget opens no listeners and exposes no IPC or network surface, so reports generally need a local vector.
