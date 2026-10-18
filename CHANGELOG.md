# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

## [0.1.0] - 2026-10-18

First release.

### Added

- **Wizard:** five steps: Camera → Start method → SD card → Firmware → Install.
- **Cameras:**
  - All 146 Canon cameras with a CHDK build, searchable by IXUS, ELPH, SD, IXY or PowerShot name.
  - Filters by series.
- **Builds:** downloads stable or development builds from the CHDK autobuild server, with caching.
- **Start methods:**
  - **Firmware Update (FI2):** load CHDK from the camera menu.
  - **Autoboot:** writes the `BOOTDISK` boot-sector signature on FAT16 and FAT32 cards.
- **Cards:**
  - Two-partition cards: CHDK is copied to both partitions on cameras that switch to the FAT32 partition after booting.
  - Card detection on macOS, with locked-card handling and automatic refresh.
  - Formatting as FAT32, or FAT16 for autoboot.
- **Firmware helper:** writes `ver.req`, with step-by-step help to find the firmware version.
- **Permissions:** a Full Disk Access check with shortcuts to System Settings.
- **App:** light and dark themes that follow the system, with a manual switch.

[Unreleased]: https://github.com/andrea-campa/chdk-sd-creator/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/andrea-campa/chdk-sd-creator/releases/tag/v0.1.0
