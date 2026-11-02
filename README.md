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
- **gremote** — remote desktop between Gablue machines: the host's *existing* Plasma session over RDP, tunnelled in SSH — no port opened, sound carried by a PipeWire tunnel sink, ephemeral RDP password. `--waypipe` keeps the old nested-KWin mode (single app, no session needed)
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

## Remote desktop

`gremote` shows another Gablue machine's desktop in a window. It attaches to the
host's Plasma session over RDP, tunnelled inside SSH: nothing is exposed on the
network, the RDP password is drawn per session, and the sound comes back through
a PipeWire tunnel. **If nobody is logged in, it creates the session first**
(`gremote-session@<user>.service`): you log in with your own SSH account and get
your own desktop, which survives the disconnect. The session is **virtual** —
KWin renders off-screen, with no DRM device, no VT and no seat — so several
accounts can be connected at the same time (gab and bdc), and the physical screen
keeps showing the greeter. `--no-start` keeps the explicit-failure behaviour.

```bash
gremote gab@sucina                    # existing Plasma session, RDP + sound
gremote --rdp-port 13390 gab@sucina   # use another local tunnel port
gremote --no-launch gab@sucina        # print the xfreerdp line instead of running it
gremote --no-audio gab@sucina         # no sound tunnel
gremote --no-start gab@sucina         # never create a session, fail if none
```

`--waypipe` keeps the previous mode: a nested KWin displayed in a window, no
session needed on the host, input through the Wayland protocol, lossless and
therefore heavy. It is the only mode that does `-a APP` (a single application).

```bash
gremote --waypipe -a dolphin gab@sucina      # one application
gremote --waypipe -s 1920x1080 gab@sucina    # forced size
gremote --waypipe --quality high gab@sucina  # H.264, higher bitrate
gremote --clean                              # kill leftover waypipe sessions here
```

In `--waypipe` mode, quality and bandwidth are tunable: `--video[=hw|sw|none]`
picks the encoder, `--codec h264|vp9|av1` the codec, and `--bpf N` the bitrate
**in bits per frame**. Careful: waypipe assumes a nominal 100 fps and sets
`bit_rate = bpf × 100`, so 3e4 ≈ 3 Mbit/s. `--quality low|medium|high` are
presets for 2e4/1e5/5e5. Encoding goes through Vulkan video on the remote GPU
(not VAAPI) and silently falls back to software x264 without it. Note that
Waypipe only encodes **opaque 8-bit** dmabufs (Xrgb8888/Xbgr8888): a compositor
presents its surfaces with alpha or 10-bit, so `--video`/`--bpf` have no effect
on a full desktop — they matter for `-a`. The remote session logs nothing unless
`--debug` is passed.

Both modes need a passwordless SSH key (`ssh-key-add`) and
`AllowStreamLocalForwarding` on the remote sshd; `--waypipe` additionally needs
the same `waypipe` version on both ends.

## Contributing

This is a personal project. Issues and PRs welcome.
