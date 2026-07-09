# Gablue [![build](https://github.com/elgabo86/gablue/actions/workflows/gablue-builds.yml/badge.svg)](https://github.com/elgabo86/gablue/actions/workflows/gablue-builds.yml)

Gablue is a custom immutable desktop distribution based on **Fedora Kinoite** (KDE Plasma), built with the [Universal Blue](https://universal-blue.org/) toolkit. Optimized for gaming, multimedia, and everyday use.

Images and the system are continuously updated, following the latest Fedora releases.

> **Note** : tools and scripts created for Gablue are currently French-only.

## Features

- **KDE Plasma** on Wayland — modern, smooth desktop
- **OGC kernel** (Open Gaming Collective) — CachyOS-based, optimized for gaming (from ublue-os/akmods)
- **Terra Mesa** — up-to-date graphics stack with 32-bit multilib
- **NVIDIA support** — closed and open-source drivers via akmods
- **Gaming optimizations** — MangoHud, VRR-aware KWin rules
- **Gwine** — Wine prefix manager with Windows desktop integration (gwine-proton is a Wine variant using Proton sources)
- **Gamepad shortcuts** — native C daemon for system actions via controller
- **32-bit ready** — full 32-bit library stack preinstalled
- **Flatpak** and **brew** ready out of the box
- Optional **DX mode** with Docker, Libvirt, QEMU

## Current Versions

<!-- VERSIONS_START -->

| Composant | Version |
|---|---|
| Last build | N/A |
| Fedora | 44 |
| Kernel (OGC) | N/A |
| Mesa (Terra) | N/A |
| KDE Plasma | N/A |
| NVIDIA (closed) | N/A |
| NVIDIA (open) | N/A |

<!-- VERSIONS_END -->

## Variants

| Image | Description | NVIDIA |
|-------|-------------|--------|
| `gablue-main` | Standard image | No |
| `gablue-nvidia-open` | NVIDIA open-source drivers | Yes (open) |
| `gablue-nvidia` | NVIDIA closed drivers | Yes (lts) |
| `gablue-main-dx` | Development mode with virtualization | No |

### Which NVIDIA image to choose?

- **`gablue-nvidia-open`** — required for RTX 50xx (Blackwell) and newer. Also recommended for RTX 20xx/30xx/40xx (Turing/Ampere/Ada Lovelace).
- **`gablue-nvidia`** (closed drivers) — only for older GPUs: GTX 7xx/9xx (Maxwell), GTX 10xx (Pascal), Titan V (Volta). Still works on newer architectures but open is preferred.

## Installation

Download the latest ISO from [GitHub Releases](https://github.com/elgabo86/gablue/releases) for a fresh install. Checksums available.

## Verification

Images are signed with Cosign. Public key available in this repository.

```bash
cosign verify --key cosign.pub ghcr.io/elgabo86/gablue-main:latest
```

## Contributing

This is a personal project. Issues and PRs welcome.
