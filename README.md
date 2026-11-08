# Gablue

Gablue is a custom immutable desktop distribution based on **Fedora Kinoite** (KDE Plasma), built with the [Universal Blue](https://universal-blue.org/) toolkit. Optimized for gaming, multimedia, and everyday use.

Images and the system are continuously updated, following the latest Fedora releases.

> **Note** : tools and scripts created for Gablue are currently French-only.

## Features

- **KDE Plasma** on Wayland — modern, smooth desktop
- **OGC kernel** (Open Gaming Collective) — optimized for gaming (from ublue-os/akmods)
- **Terra Mesa** — up-to-date graphics stack with 32-bit multilib
- **NVIDIA support** — closed and open-source drivers via akmods
- **Gwine** — seamless Wine runner: launch .exe and .wgp files directly with no prefix management, thanks to a single shared prefix built on Proton sources. Bundles ds2xbox to emulate an Xbox controller for Wine games (like Steam)
- **Game packages** — .wgp (Windows Game Pack) and .lgp (Linux Game Pack) self-contained squashfs game bundles: package a full compressed PC game into a single file you can create yourself, then run it directly with no installation required
- **Gamepad shortcuts** — native C daemon for system actions via controller
- **Flatpak** and **brew** ready out of the box
- **Automatic weekly updates** — system and flatpaks staged in the background
- **gremote** — remote desktop between Gablue machines: the host's *existing* Plasma session over RDP, tunnelled in SSH — no port opened, sound carried by a PipeWire tunnel sink, ephemeral RDP password. when no session exists one is created **virtually**, so several accounts can work at once without taking the physical screen
- **gablue-manager** — « Gablue Manager »: one window for this machine and the remote ones — SSH host list with latency and online indicator, one-click remote desktop (`gremote`), file browsing (Dolphin over sftp), SSH terminal, remote shutdown/reboot/update, local-network discovery, host addition with automatic key copy, local SSH server and local-announcement toggles, plus local updates (Gablue image, Flatpaks, gwine) and advanced options (CPU mitigations, CPUID fix, Bluetooth ERTM, HDMI 2.1 on AMD GPUs, CPUID fault emulation, services and image variant — DisplayLink, Docker on DX variants, Vulkan low-latency, Broadcom wl driver, atuin shell history, rebase to another Gablue variant — gwine DXVK/display/gamepad settings, foreground booster, Tailscale with a user-supplied login server), and app management — install, update or remove ES-DE, Azahar, Eden, Xenia, Lichii and OpenCode 2, on this machine or over SSH on a remote host (AppImages via GearLever; config, saves and sessions are kept on removal)
- Optional **DX mode** with Docker, Libvirt, QEMU, and ROCm for AMD

## Current Versions

<!-- VERSIONS_START -->

| Component | Version |
|---|---|
| Last build | 2026-11-07 |
| Fedora | 45 |
| Kernel (OGC) | 7.3.3 (rev 1.1) |
| Mesa (Terra) | 26.2.6 |
| KDE Plasma | 6.8.2 |
| NVIDIA (open) | 620.70.06 |
| NVIDIA (closed) | 580.196.07 |

<!-- VERSIONS_END -->

## Variants

| Image | Description | Recommended for |
|---|---|---|
| `gablue-main` | Standard image | AMD / Intel GPUs |
| `gablue-main-dx` | DX mode with ROCm | AMD / Intel GPUs |
| `gablue-nvidia-open` | NVIDIA open-source drivers | NVIDIA (RTX 20xx and newer) |
| `gablue-nvidia-open-dx` | DX mode with NVIDIA open drivers | NVIDIA (RTX 20xx and newer) |
| `gablue-nvidia` | NVIDIA closed drivers | NVIDIA (GTX 9xx/10xx) |

## Installation

Download the latest live ISO from [GitHub Releases](https://github.com/elgabo86/gablue/releases) to try and install Gablue. Checksums available.

## Verification

Images are signed with Cosign. Public key available in this repository.

```bash
cosign verify --key cosign.pub ghcr.io/elgabo86/gablue-main:latest
```

## Remote desktop

`gremote` shows another Gablue machine's desktop in a window. It attaches to the
host's Plasma session over RDP, tunnelled inside SSH: nothing is exposed on the
network, the RDP password is drawn per session, and the sound comes back through
a PipeWire tunnel. **If nobody is logged in, it creates the session first**
(`gremote-session@<user>.service`): you log in with your own SSH account and get
your own desktop, which survives the disconnect. The session is **virtual** —
KWin renders off-screen, with no DRM device, no VT and no seat — so several
accounts can be connected at the same time (gab and bdc), and the physical screen
keeps showing the greeter. The rule is: **an existing graphical session for that
account is reused** (a physical one is shared — the server's screen then shows
what the client does), and a **virtual** one is created only when there is none.
`--no-start` keeps the explicit-failure behaviour.

```bash
gremote gab@sucina                    # existing Plasma session, RDP + sound
gremote gab@sucina:2323               # SSH on a non-standard port
gremote -p 2323 gab@sucina            # same, explicit option
gremote --rdp-port 13390 gab@sucina   # use another local tunnel port
gremote --no-launch gab@sucina        # print the client line instead of running it
gremote --no-audio gab@sucina         # no sound tunnel
gremote --no-start gab@sucina         # never create a session, fail if none
```

The old waypipe mode (nested KWin, `-a`, `-s`, `--video`, `--codec`, `--quality`,
`--bpf`, `--compress`, `--no-gpu`, `--clean`) was removed on 02/11/2026 — RDP is
the only transport now, and the removed code stays in the git history.

The local client is `sdl-freerdp` when present (SDL3, native Wayland — no XWayland
in the image/input path), with `xfreerdp` as fallback. `wlfreerdp` is not used:
FreeRDP deprecated it in favour of the SDL3 client. The RDP password never appears
on a command line (readable by any local user through `ps`): it is piped to the
client's `/from-stdin:force`.

One passwordless SSH key (`ssh-key-add`) and `AllowStreamLocalForwarding` on the
remote sshd are all it needs.

`gablue-manager` (menu → Gablue Manager) does the same graphically: it lists the hosts
with latency, shows whether the key is accepted (green online, orange key
refused, purple unknown host fingerprint, grey offline), launches the remote
desktop, Dolphin (`sftp://`) or an SSH terminal in one click, opens a new
desktop window on every click (the second client takes over the first, and its
exit code is not reported as a failure), can close that session from the card
menu, reboots / shuts down / updates a host, discovers machines on the local
network, and copies the
key to a new host — the password is asked once and never stored. It can also
turn the local SSH server on or off, switch the local network announcement of that
server (`/etc/avahi/services/ssh.service`) on or off, and manage this machine:
Gablue image, Flatpak and gwine updates, plus the advanced options that used to
live in the old `gablue-config` dialog (CPU mitigations, CPUID fix, Bluetooth
ERTM, CPUID fault emulation), the gwine settings (async DXVK, X11 or experimental
Wayland, Sony pads as Xbox 360), the Plasma foreground booster and Tailscale —
the login server URL is asked on enable, never hardcoded. Every one of those
updates and options is also available **on the remote hosts**, from each host's
menu; kernel, CPUID and Tailscale changes go through NOPASSWD helpers, so no
password is asked on a Gablue image. Operations run in their own window:
several hosts can be updated or reconfigured at the same time.

**No authorisation prompts in the remote desktop.** polkit treats a seatless
session as *not local*, so it falls back on `implicit_any` instead of
`implicit_active`: plasmashell's Flatpak appstream refresh, fwupd, tuned and 177
other actions popped an authentication dialog that nobody could answer, while a
session at the seat asks nothing. `70-gremote-seatless-active.rules` grants those
sessions exactly what an *active local* session already gets without a password —
a list generated at image build time by `/usr/libexec/gablue-polkit-active-list`,
so it follows the installed packages. It cannot be computed at runtime: polkit 127
no longer exposes the action's implicit values to rules, and a rule that asks
polkitd for them (spawning `pkaction --verbose`) deadlocks and is killed after
15 s, which delays every check and then shows the dialog anyway. The grant is
restricted to `wheel` members: without that, any non-local subject would qualify,
including a plain user's process in a remote SSH session (measured: a `nobody`
process got `app-update` and `power-off` for free). Anything that needs a password
at the physical console still needs one here.

Installing or uninstalling a Flatpak from Discover is `auth_admin` by policy, even
for an active session; Fedora's own rule opens it to `wheel` users but only when
`subject.local` is true, i.e. at the seat. `71-gremote-wheel-flatpak.rules` mirrors
that rule minus the seat clause, so a remote desktop installs what the same wheel
user installs at the keyboard. One deliberate exception to the parity:
`NetworkManager.network-control` stays refused (`60-gremote-seatless.rules`), so
the network applet cannot cut the link the session runs on.

## Contributing

This is a personal project. Issues and PRs welcome.
