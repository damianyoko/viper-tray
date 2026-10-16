# Viper Tray

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Platform: Windows](https://img.shields.io/badge/platform-Windows%2010%2B-blue)](https://www.microsoft.com/windows)
[![Language: Rust](https://img.shields.io/badge/language-Rust-orange)](https://www.rust-lang.org)
[![Release](https://img.shields.io/github/v/release/damianyoko/viper-tray)](https://github.com/damianyoko/viper-tray/releases)

A tiny system tray battery indicator for the **Razer Viper V3 Pro** wireless mouse. No Razer Synapse, no kernel drivers, no admin required. ~9 MB resident, single-file `.exe`.

<p align="center">
  <img src="docs/demo.gif" alt="Viper Tray demo" />
  <br/>
  <sub><em>First interaction: hover → native tooltip. Second: left-click → popup with the percentage.</em></sub>
</p>

## Why

Synapse is bloatware. It runs multiple services, installs kernel-mode drivers, and phones home, just to show you a battery percentage. This tool does the same job in a 213 KB binary that uses ~9 MB of RAM and talks directly to the mouse over standard HID feature reports.

### Footprint comparison

Razer's currently-recommended client is **Synapse 4** (modular installer, Mouse module only).

| | **Viper Tray** (this) | Razer Synapse 4 |
|---|---|---|
| Disk install | **213 KB** | ~250 MB – 1 GB (varies with modules; +Chroma adds more) |
| RAM at idle | **~9 MB** | ~150–350 MB across `Razer Synapse Service`, `RazerAppEngine`, helpers |
| Background services | None | Multiple user-mode services, autostart |
| Kernel drivers | None | `RzCommon.sys`, `RzDev_*.sys` (per-device) |
| Admin required | No | Yes (installer) |
| Network calls | None | Yes (telemetry, updates, account) |
| Tells you the battery % | Yes | Yes |

If the only thing you use Synapse for is checking your mouse battery, this does the same job at roughly 1/30th the RAM and a fraction of a percent of the disk footprint.

## What you get

- A compact gauge icon in the system tray: the outer ring shows battery level around the Viper `V`, colour-coded:
  - **Green** ≥ 60%
  - **Orange** 20–60%
  - **Red** < 20%
- **Left-click** → small popup near cursor with a large percentage, connection state, and battery bar. Click again to dismiss.
- **Right-click** → menu: *Refresh now* / *Exit*.
- Auto-refresh every 5 minutes (system-coalesced waitable timer; battery-friendly on laptops).
- Transient read failures show as *retrying* first; the tray keeps the last known percentage and retries quickly before declaring the mouse offline.
- Single-instance guard via named mutex — clicking the icon twice doesn't stack trays.
- Survives reboots once installed.

## Install

### Easy: pre-built binary (recommended)

1. Download `viper-tray-vX.Y.Z.zip` from the [latest release](https://github.com/damianyoko/viper-tray/releases/latest).
2. Extract anywhere stable (e.g. `C:\Users\<you>\Tools\RazerBattery\`).
3. From an unprivileged PowerShell prompt in that folder:
   ```powershell
   .\install.ps1
   ```
   Adds an `HKCU\…\Run` entry and launches the tray immediately.

### From source (requires Rust toolchain)

```powershell
git clone https://github.com/damianyoko/viper-tray
cd viper-tray\rust
cargo build --release
copy target\release\viper-tray.exe ..\
cd ..
.\install.ps1
```

### PowerShell-only fallback

If you don't want to download a binary or build Rust, the original PowerShell implementation is still in this repo (`razer-battery.ps1` + `start-hidden.vbs`). The installer will use it if no `.exe` is present. ~125 MB RAM though.

## Uninstall

```powershell
.\uninstall.ps1
```
Removes the autostart entry and stops any running instance.

## How it works

The Viper V3 Pro exposes its mouse HID interface (UsagePage `0x01`, Usage `0x02`) on the dongle (VID `0x1532`, PID `0x00C0` / `0x00C1`). Razer's command protocol piggybacks on that interface as a 90-byte feature report.

The binary:
1. Enumerates HID device interfaces via SetupAPI for VID `0x1532`
2. Opens each candidate with `dwDesiredAccess = 0` to bypass `mouclass`'s exclusive lock (mice can't be opened with read/write while in use, but feature-report-only access works)
3. Filters for the mouse collection (UP=1, Usage=2, FeatureReportByteLength ≥ 91)
4. Sends a Razer feature report: `command_class = 0x07`, `command_id = 0x80` (get battery level), transaction ID `0x1F`
5. Reads the response; battery byte is `arguments[1]` (offset 10), scaled `0–255 → 0–100%`

Protocol details from the [OpenRazer](https://github.com/openrazer/openrazer) project.

## Why it only works for Viper V3 Pro

The PID filter and transaction ID are hard-coded for this mouse. Other Razer mice use different PIDs and some older ones use TID `0x3F`. If you have a different Razer mouse, [xzeldon/razer-battery-report](https://github.com/xzeldon/razer-battery-report) supports more devices.

## Troubleshooting

**The icon doesn't appear** — Windows hides new tray icons by default. Click the `^` arrow in your taskbar and drag the battery icon out into the always-visible area.

**It shows `retrying` or `offline`** — brief `retrying` states usually mean the mouse was asleep or the dongle returned a transient HID busy/not-ready response. The tray keeps the last known percentage when available, retries every 15 seconds for the first two failed reads, then marks the mouse offline and retries every 30 seconds. If it stays offline, move the mouse, click *Refresh now*, or unplug and replug the dongle.

**Two icons appear on boot** — duplicate Run entries. Run `.\uninstall.ps1` then `.\install.ps1` to reset cleanly.

**Antivirus complains** — the binary is unsigned. Submit a false-positive report or whitelist it; the source is right here for verification.

## License

MIT. See [LICENSE](LICENSE).
