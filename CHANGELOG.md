# Changelog

## Unreleased

- Auto-reconnect on activity: after a failed read (usually the mouse asleep), the tray checks for mouse/keyboard input every 3 seconds and re-reads as soon as you're active, instead of waiting up to 30 seconds. No more clicking the icon to wake it.

## v2.3.0 — 2026-10-16

Readability + resilience pass.

- Reworked the tray battery display into a circular gauge around the Viper `V`, so charge level is readable as a ring at tray size.
- Replaced the left-click text-only popup with a compact battery meter: large percentage, connection state, and a mini battery bar.
- Reduced false offline/disconnected states. Failed HID reads now keep the last known percentage, show `retrying`, retry quickly, and only switch to offline after repeated misses.
- Made the HID battery query more tolerant while the mouse or dongle is waking up by retrying raw zero/not-ready responses and transient feature-report failures.

## v2.2.0 — 2026-05-06

New icon design + popup polish.

- **New tray icon: green "V" outline (Viper brand) with battery-level fill inside.** Replaces the original horizontal-battery shape that read too much like a generic laptop battery indicator. The V outline stays green; the interior fill drops with the battery level and shifts colour (green ≥60% → orange 20–60% → red <20%).
- **Popup auto-dismisses after 5 seconds** if you don't click it. Click-anywhere dismissal still works for instant close.
- `install.ps1` switched from `HKCU\Run` to a Startup folder shortcut for more reliable autostart on Windows 11 — Run-key entries for unsigned binaries were getting silently skipped on some boots.
- `uninstall.ps1` cleans up both the new shortcut and any legacy `HKCU\Run` entries from prior versions.

## v2.1.0 — 2026-05-05

Renamed to **Viper Tray**. GitHub repo also renamed to `viper-tray` (old URLs redirect).

- Binary renamed: `razer-viper-tray.exe` → `viper-tray.exe`
- Cargo package renamed: `razer-viper-tray` → `viper-tray`
- App title, window class, and named mutex updated accordingly
- HKCU autostart entry renamed: `RazerBatteryTray` → `ViperTray`
- `install.ps1` automatically migrates the legacy entry; existing users just re-run it
- README footprint comparison updated to **Razer Synapse 4** (current Razer client) instead of Synapse 3
- Comparison table reordered so Viper Tray gets the lead column

## v2.0.1 — 2026-05-05

Bug fixes from the v2.0.0 review.

- **Popup click-away dismissal works.** v2.0.0 used `WM_KILLFOCUS` which never fires when the window is shown with `SW_SHOWNOACTIVATE` — clicks outside the popup did nothing. Now uses `SetCapture` + `WM_CAPTURECHANGED` so any click anywhere dismisses the popup. README claim now matches behaviour.
- **Popup position clamped to monitor work area.** v2.0.0 could spill the popup off the right edge of the screen or onto an adjacent monitor when the cursor was near the boundary. Now uses `MonitorFromPoint` + `GetMonitorInfoW` and flips the popup below the cursor if it would overflow the top of the work area.
- **HID `busy` (status 0x01) responses are now properly retried** with full Set/Get cycles and escalating sleep (80 → 150 → 250 ms) instead of being accepted with stale data.
- **Replaced `static mut POPUP_TEXT` with `GWLP_USERDATA`** — proper per-window storage, no `unsafe` data race surface, future-proof against Rust 2024 lints.
- **`SetWaitableTimer` failure no longer silent** — the worker thread depends on it; failure now panics loudly rather than leaving a permanently stale tray icon.
- README: added footprint comparison vs Razer Synapse 3.

## v2.0.0 — 2026-05-05

Full Rust rewrite. Same protocol, much smaller footprint.

- Native Rust implementation: 213 KB single-file `.exe`, ~9 MB RAM (down from ~125 MB on PowerShell)
- Direct Win32 message pump — no `winit`, no `image`, no `tokio`, no GUI framework
- Waitable timer for the 5-minute refresh loop (system-coalesced, laptop-battery friendly)
- `CreateMutexW` single-instance guard
- Tray icon via `tray-icon` crate (Tauri's), proper `DestroyIcon` lifecycle
- Custom Win32 popup window for the click flyout (raw GDI rendering)
- HID query in raw `windows-rs` calls — `CreateFileW` with `dwDesiredAccess = 0` to bypass `mouclass`
- PowerShell version retained as a transparent reference implementation

### Migration from v1.0.0

```powershell
.\uninstall.ps1
# download release zip, extract over the same folder
.\install.ps1
```
The installer auto-detects the `.exe` and prefers it over the PowerShell fallback.

## v1.0.0 — 2026-05-05

Initial PowerShell release.

- HID feature-report battery query for Razer Viper V3 Pro (PID `0x00C0` / `0x00C1`, transaction ID `0x1F`)
- System tray icon: horizontal battery shape with colour-coded fill (green ≥60%, orange 20–60%, red <20%, grey when offline)
- Left-click toggles a small popup near the cursor with the percentage
- Right-click menu: Refresh now / Exit
- 5-minute auto-refresh
- `install.ps1` / `uninstall.ps1` — HKCU autostart, no admin required
