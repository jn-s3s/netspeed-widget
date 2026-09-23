# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow writes generated notes for the merged pull requests of every tag into `RELEASE.md` and attaches them to that GitHub release. This file is the curated history instead, so an entry here should say what changed for someone running or building the app, not repeat a PR list.

## [Unreleased]

### Changed

- The build is now an onedir package: `dist/NetSpeedWidget[-<version>]/` holds the executable next to the `_internal` directory that carries its resources, the bundled `node.exe` and the fast-cli tree. Nothing is unpacked to a temp folder at startup, which is also where self-extracting builds tend to draw antivirus interference.
- A release publishes that folder as `NetSpeedWidget-<version>-windows-x64.zip`, with the versioned directory as the top-level entry. Extract the whole archive and run the executable in place, since it only finds `_internal` beside it.
- The release build cache key now also covers `third_party/fast-bundle/package.json` and its lockfile, so a vendored fast-cli change can no longer hit a stale `dist`.

### Added

- This changelog.

## [1.0.0] - 2025-09-03

Initial release. See [v1.0.0](https://github.com/jn-s3s/netspeed-widget/releases/tag/v1.0.0) for what it shipped; it predates this file, so nothing is listed here beyond the pointer.

[Unreleased]: https://github.com/jn-s3s/netspeed-widget/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jn-s3s/netspeed-widget/releases/tag/v1.0.0
