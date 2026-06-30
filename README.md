# Gablue [![build](https://github.com/elgabo86/gablue/actions/workflows/gablue-builds.yml/badge.svg)](https://github.com/elgabo86/gablue/actions/workflows/gablue-builds.yml)

Gablue is a custom immutable desktop distribution based on **Fedora Kinoite 44** (KDE Plasma), built with the [Universal Blue](https://universal-blue.org/) toolkit. Optimized for gaming, multimedia, and everyday use.

## Features

- **KDE Plasma 6** on Wayland — modern, smooth desktop
- **OGC kernel** optimized for gaming (from ublue-os/akmods)
- **Terra Mesa** — up-to-date graphics stack with 32-bit multilib
- **NVIDIA support** — closed and open-source drivers via akmods
- **Gaming optimizations** — Gamescope, MangoHud, VRR-aware KWin rules
- **Gwine** — Wine/Proton prefix manager with Windows desktop integration
- **Gamepad shortcuts** — native C daemon for system actions via controller
- **Wine/Proton ready** — full 32-bit library stack preinstalled
- **Flatpak** and **brew** ready out of the box
- Optional **DX mode** with Docker, Libvirt, QEMU

## Variants

| Image | Description | NVIDIA |
|-------|-------------|--------|
| `gablue-main` | Standard image | No |
| `gablue-nvidia` | NVIDIA closed drivers | Yes (lts) |
| `gablue-nvidia-open` | NVIDIA open-source drivers | Yes (open) |
| `gablue-main-dx` | Development mode with virtualization | No |

## Installation

Download the latest ISO from [GitHub Releases](https://github.com/elgabo86/gablue/releases) for a fresh install. Checksums available.

## Verification

Images are signed with Cosign. Public key available in this repository.

```bash
cosign verify --key cosign.pub ghcr.io/elgabo86/gablue-main:latest
```

## Contributing

This is a personal project. Issues and PRs welcome.
