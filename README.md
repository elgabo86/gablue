# Gablue

Gablue is a custom immutable desktop distribution based on **Fedora Kinoite** (KDE Plasma), built with the [Universal Blue](https://universal-blue.org/) toolkit. Optimized for gaming, multimedia, and everyday use.

Images and the system are continuously updated, following the latest Fedora releases.

> **Note** : tools and scripts created for Gablue are currently French-only.

## Features

- **KDE Plasma** on Wayland — modern, smooth desktop
- **OGC kernel** (Open Gaming Collective) — optimized for gaming (from ublue-os/akmods)
- **Terra Mesa** — up-to-date graphics stack with 32-bit multilib
- **NVIDIA support** — closed and open-source drivers via akmods
- **Gaming optimizations** — MangoHud, VRR-aware KWin rules
- **Gwine** — seamless Wine runner: launch .exe and .wgp files directly with no prefix management, thanks to a single shared prefix (gwine-proton is a Wine variant using Proton sources)
- **Game packages** — .wgp (Windows Game Pack) and .lgp (Linux Game Pack) self-contained squashfs game bundles: package a full compressed PC game into a single file you can create yourself, then run it directly with no installation required
- **Gamepad shortcuts** — native C daemon for system actions via controller
- **Plasma Bigscreen** (BETA) — couch/TV interface, switchable from the desktop session
- **32-bit ready** — full 32-bit library stack preinstalled
- **Flatpak** and **brew** ready out of the box
- Optional **DX mode** with Docker, Libvirt, QEMU, and ROCm for AMD

## Current Versions

<!-- VERSIONS_START -->

| Component | Version |
|---|---|
| Last build | 2026-07-10 |
| Fedora | 44 |
| Kernel (OGC) | 7.1.3-ogc3.4.fc44 |
| Mesa (Terra) | 26.1.426.1.4 |
| KDE Plasma | 6.7.2 |
| NVIDIA (open) | 610.43.03 |
| NVIDIA (closed) | 580.173.02 |

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

Download the latest ISO from [GitHub Releases](https://github.com/elgabo86/gablue/releases) for a fresh install. Checksums available.

## Verification

Images are signed with Cosign. Public key available in this repository.

```bash
cosign verify --key cosign.pub ghcr.io/elgabo86/gablue-main:latest
```

## Contributing

This is a personal project. Issues and PRs welcome.
