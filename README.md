# pactrack

Pactrack is an Arch Linux package update tracker with a tray icon and menu.

## Screenshot
<img width="2556" height="1406" alt="Image" src="https://github.com/user-attachments/assets/f050b12b-dbdf-4d5d-a809-2bdb7a94fe92" />

## Features

- Tray icon with status states: `checking`, `up_to_date`, `updates_available`, `error`
- Menu items:
  - Status
  - Official update count
  - AUR update count
  - Last check timestamp
  - Refresh now
  - Open details
  - Upgrade
  - Upgrade official
  - Upgrade AUR
  - Quit
- Official updates via built-in Rust implementation (`fakeroot pacman -Sy` + `pacman -Qu`)
- AUR updates with auto-detected `paru` (preferred) or `yay`
- 30-minute polling by default
- Desktop notification when total pending update count changes
- XDG config file support at `~/.config/pactrack/config.toml`
- One-shot CLI mode for diagnostics

## Runtime Requirements (Arch)

- `gtk3`
- `libayatana-appindicator` (or `libappindicator-gtk3` compatible library)
- DBus session
- `pacman`, `pacman-conf`, and `fakeroot`
- Optional: `paru` or `yay`

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run --release
```

One-shot check mode:

```bash
cargo run --release -- --once
```

## Systemd User Service

Use the provided unit file at `systemd/pactrack.service`.

Install and enable:

```bash
mkdir -p ~/.config/systemd/user
cp systemd/pactrack.service ~/.config/systemd/user/pactrack.service
systemctl --user daemon-reload
systemctl --user enable --now pactrack.service
```

Check status/logs:

```bash
systemctl --user status pactrack.service
journalctl --user -u pactrack.service -f
```

Notes:

- This is a `--user` service (not a system-wide root service) because Pactrack is a tray GUI app.
- Default `ExecStart` points to `%h/.cargo/bin/pactrack`; change it if your binary is elsewhere.

## CLI Flags

- `--config <path>`: use a custom config file
- `--poll-minutes <n>`: override polling interval
- `--no-aur`: disable AUR checks
- `--once`: run one check and exit

## Config

Default config path: `~/.config/pactrack/config.toml`

```toml
poll_minutes = 30
notify_on_change = true
enable_aur = true
terminal = "auto"
official_check_cmd = "auto"
aur_helper = "auto" # auto | paru | yay | none
upgrade_cmd = "auto"
```

## Notes

- `Upgrade` opens a terminal and runs:
  - `paru -Syu` or `yay -Syu` when helper is available
  - `sudo pacman -Syu` otherwise
- `Open details` opens a terminal and prints official/AUR pending updates.
