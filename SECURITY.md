# Security policy

## Supported versions

This policy covers NetSpeed Widget 2.x. Only the latest 2.x release receives security fixes. Use the latest available 2.x build when reporting an issue.

| Version        | Supported |
| -------------- | --------- |
| Latest 2.x     | Yes       |

## How to report a vulnerability

Use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/jn-s3s/netspeed-widget/security) of this repository.
2. Click **Report a vulnerability**.
3. Describe the issue, the steps to reproduce it and the impact you believe it has.

If private reporting is unavailable for any reason, open a minimal public issue asking for a private contact channel without revealing details.

## What to include

- The NetSpeed Widget version affected (check the release ZIP filename or executable's Details properties)
- Steps or a proof of concept showing how the issue manifests
- Which component is involved if you know it (HTTPS speedtest, optional fast-cli provider, config or log file handling, global hotkey registration, taskbar integration, tray icon)

## What to expect

We will acknowledge your report as soon as we triage it. Where possible, we develop fixes privately and include them in a patched release. We will credit you in the release notes unless you prefer to stay anonymous.

Please do not disclose the issue publicly until a fix has been released.

## Scope notes

NetSpeed Widget runs with your user privileges and stores no secrets: its only local state is a small config file and a log file under `%APPDATA%\NetSpeedWidget`. Reports are in scope when they show behavior beyond what the app legitimately does, such as:

- Command injection or argument smuggling through an optional fast-cli provider (a manually installed Node/fast-cli bundle, or `fast.exe` / `fast-cli.exe` on PATH)
- Unsafe path handling when resolving bundled resources or the config and log files
- Unexpected code execution when resolving or launching optional speedtest providers; standard release ZIPs contain no Node runtime or fast-cli bundle
- Unexpected outbound connections beyond the app's latency probes and speedtest traffic

The widget opens no network listeners. It makes outbound HTTPS speedtest requests and TCP/ICMP latency probes, and integrates with Explorer through Windows APIs. Reports may involve local input, optional provider execution or remote measurement responses.
