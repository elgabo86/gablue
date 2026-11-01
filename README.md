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
- **Plasma Bigscreen** (BETA) — couch/TV interface, switchable from the desktop session
- **Flatpak** and **brew** ready out of the box
- **Automatic weekly updates** — system and flatpaks staged in the background
- **gremote** — remote desktop between Gablue machines, two transports: waypipe + nested KWin (ephemeral session, no audio), or `--rdp` — the host's *existing* Plasma session over RDP, tunnelled in SSH (no port opened, sound carried by a PipeWire tunnel sink)
- Optional **DX mode** with Docker, Libvirt, QEMU, and ROCm for AMD

## Current Versions

<!-- VERSIONS_START -->

| Component | Version |
|---|---|
| Last build | 2026-11-02 |
| Fedora | 44 |
| Kernel (OGC) | 7.2.11 (rev 4.1) |
| Mesa (Terra) | 26.2.6 |
| KDE Plasma | 6.8.2 |
| NVIDIA (open) | 615.78.08 |
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

## Remote desktop (ephemeral)

`gremote` opens a Plasma session (or a single application) hosted on another Gablue machine and shows it in a window on your desktop — Waypipe over SSH, nested KWin on the remote side. The session dies with the connection: this is a stopgap until KRdp supports remote login.

```bash
gremote gab@sucina                    # distant Plasma desktop
gremote -a dolphin gab@sucina         # one application
gremote -s 1920x1080 gab@sucina       # forced size
gremote --quality high gab@sucina     # H.264 video, higher bitrate
gremote --rdp gab@sucina              # existing Plasma session over RDP, sound included
gremote --bpf 3e5 --codec vp9 gab@sucina
gremote --compress none gab@sucina    # no compression (fast LAN)
gremote --clean                       # kill leftover sessions on this machine
```

Quality and bandwidth are tunable: `--video[=hw|sw|none]` picks the encoder, `--codec h264|vp9|av1` the codec, and `--bpf N` the bitrate **in bits per frame**. Careful: waypipe assumes a nominal 100 fps and sets `bit_rate = bpf × 100` (both software and hardware paths), so 3e4 ≈ 3 Mbit/s, 1e5 ≈ 10 Mbit/s, 5e5 ≈ 50 Mbit/s. `--quality low|medium|high` are presets for 2e4/1e5/5e5. Defaults are tuned for WAN latency: H.264 video on at 3e4 bits per frame (~3 Mbit/s, deliberately low quality), encoded on the remote GPU when it supports Vulkan video — that is what waypipe uses, not VAAPI, and without it waypipe silently falls back to software x264. `--no-video` turns encoding off (raw stream, for a fast LAN or a loaded host). Note that Waypipe only encodes **opaque 8-bit** dmabufs (Xrgb8888/Xbgr8888): a compositor presents its surfaces with alpha or 10-bit, so `--video`/`--bpf` have no effect on a full desktop — they matter for `-a` with an app that presents opaque 8-bit buffers. On a desktop, the levers are the resolution and `--compress`. `waypipe bench` measures the best setting for a given link.

Sound is carried separately from Waypipe: `gremote` forwards this machine's PipeWire/Pulse sockets over SSH, so remote applications play on local speakers (on by default, `--no-audio` to disable). The remote session logs nothing unless `--debug` is passed. Default size follows the local screen down a notch: 1600x900 on 2K and above, 1280x720 on 1080p.

Requires the same `waypipe` version on both ends, a passwordless SSH key (`ssh-key-add`), and `AllowStreamLocalForwarding` on the remote sshd. Known limit: `plasmashell` may not paint its panel in a non-logind session — use `-a konsole` as a fallback.

## Contributing

This is a personal project. Issues and PRs welcome.
