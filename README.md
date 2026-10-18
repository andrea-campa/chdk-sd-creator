# CHDK SD Creator

A Tauri desktop app that prepares an SD card with [CHDK](https://chdk.fandom.com/wiki/CHDK) for Canon PowerShot / IXUS cameras.
It's a four-step wizard: **Camera → SD card → Firmware → Install**.

Supported so far: **Canon IXUS 90 IS** (PowerShot SD790 IS / IXY Digital 95 IS), firmware 1.00C, 1.00D and 1.00E.

## What it does

- Lists FAT-formatted removable cards (it never shows internal or non-FAT volumes).
- Gets the latest stable or development build for your camera and firmware from the CHDK autobuild server (`mighty-hoernsche.de`) and caches it.
- Unpacks the build onto the card.
- Gives you two ways to start CHDK:
  - **Firmware Update** (recommended): start the camera in Playback mode, then go to Menu → Firmware Update.
  - **Autoboot**: writes the `BOOTDISK` signature into the card's boot sector, at offset 0x40 on FAT16 or 0x1E0 on FAT32. The app asks for your password through the standard macOS prompt. Then you lock the card. The IXUS 90 IS can only autoboot from FAT16.
- Supports two-partition cards. On cameras with `multipart = true` (like the IXUS 90 IS), CHDK boots from the first (FAT16) partition and then switches the camera to the first FAT32 partition. The app copies CHDK to both partitions; otherwise CHDK shows "missing modules".
- Lists locked (read-only) cards but won't write to them until you unlock them. The card list refreshes by itself.
- Asks for Full Disk Access at startup. Autoboot needs it to write the boot sector.
- Writes `ver.req`, so the camera shows its firmware version.
- Formats the card as FAT32, or as a FAT16 partition for autoboot (up to 2 GB; on bigger cards the rest becomes a second partition). It always asks you to confirm first.

## Adding a camera

Add a `[[camera]]` block to [`src-tauri/cameras.toml`](src-tauri/cameras.toml). The `id` must match the CHDK platform name used in the autobuild zip names (for example `ixus90_sd790-100c-1.6.1-6355-full.zip`). The `camnotes.txt` file in each build says whether the camera supports FI2 and how it boots.

## Development

```bash
cd src-tauri
cargo tauri dev        # run
cargo test             # unit tests
cargo tauri build      # .app + .dmg in target/release/bundle
```

End-to-end test against a disk image (no real card needed):

```bash
hdiutil create -size 64m -fs "MS-DOS FAT16" -volname CHDKTEST -layout MBRSPUD /tmp/t.dmg
hdiutil attach /tmp/t.dmg          # note the diskNs1 device
CHDK_TEST_DEVICE=diskNs1 cargo test --test disk_image -- --ignored --nocapture
```

Card handling is implemented for macOS only (it uses `diskutil`). Windows and Linux report "not implemented yet".
