# Security policy

## Supported versions

Only the latest NetSpeed Widget 2.x release receives security fixes. Use that
version when reporting an issue.

| Version    | Supported |
| ---------- | --------- |
| Latest 2.x | Yes       |

## How to report a vulnerability

Use GitHub's private vulnerability reporting:

1. Go to the [Security tab](https://github.com/jn-s3s/netspeed-widget/security) of this repository.
2. Click **Report a vulnerability**.
3. Describe the issue, the steps to reproduce it and the impact you believe it has.

If private reporting is unavailable for any reason, open a minimal public issue asking for a private contact channel without revealing details.

## What to include

- The NetSpeed Widget version affected (check the release asset filename or executable's Details properties)
- Steps or a proof of concept showing how the issue manifests
- Which component is involved if you know it (HTTPS speedtest, config or log file handling, global hotkey registration, taskbar integration, tray icon)

## What to expect

We will acknowledge your report as soon as we triage it. Where possible, we develop fixes privately and include them in a patched release. We will credit you in the release notes unless you prefer to stay anonymous.

Please do not disclose the issue publicly until a fix has been released.

## Scope notes

NetSpeed Widget runs with your user privileges. Its settings and saved speedtest
result are in `config.json` under `%APPDATA%\NetSpeedWidget`, alongside `log.txt`
and up to two log backups. Enabling Start with Windows also writes a per-user
Run entry and can clear its Windows Startup Apps disabled state. The app does
not store credentials.

Reports are in scope when they show behavior beyond the app's intended actions,
such as:

- Unsafe path handling when resolving bundled resources or the config and log files
- Unexpected outbound connections beyond the app's latency probes and speedtest traffic

The widget opens no network listeners. It makes outbound HTTPS speedtest requests and TCP/ICMP latency probes, and integrates with Explorer through Windows APIs. Reports may involve local input or remote measurement responses.
