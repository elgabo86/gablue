# Scripts de build (files/scripts/) — gablue

> Sous-document du AGENTS.md racine. Détail de chaque script appelé par les
> RUN du Containerfile-gablue (ordre et montages : voir le Containerfile +
> conventions racine).
> Même règle que la racine : à jour avant tout commit touchant files/scripts/
> ou le comportement d'une étape RUN.
> Fichiers système posés par post-install → files/system/AGENTS.md.

## Scripts de build détaillés

### copr — Configuration des dépôts

- **keepcache=1** activé ici (désactivé dans finalize) → cache DNF persistant entre builds
- **COPR** : ublue-os/bazzite, ublue-os/bazzite-multilib, ublue-os/staging, ublue-os/packages, che/nerd-fonts, hikariknight/looking-glass-kvmfr, lizardbyte/beta
  - Migration bazzite-org → ublue-os (08/2026) : `bazzite-org/bazzite` abandonné ; `ublue-os/bazzite` fournit bees 0.11 (requis par `--throttle-factor` de `configure-beesd`) ; `bazzite-org/rom-properties` retiré → rom-properties vient de Terra (comme Bazzite)
- **Tiers** : Tailscale, Negativo17
- **Terra (FyraLabs)** : terra-release, terra-release-extras, terra-release-mesa
  - Exclusion `terra-glfw*` : Conflicts cross-arch → casse l'install de `mangohud.i686` (glfw doit rester Fedora)
  - Exclusion `waydroid*` (09/2026) : `waydroid-nvidia` (terra-extras) Provides `libvirglrenderer.so.1` + Obsoletes `waydroid` — avec Terra priority 3, dnf5 l'utiliserait dans la transaction virtualisation DX (`qemu-device-display-*-gl`) : fork waydroid complet + lxc + `waydroid-container.service` activé, virglrenderer Fedora évincé (DX uniquement ; exclusion → retombe sur virglrenderer Fedora, validé conteneur)

Exclusions importantes :
- **Swap ostree** (Bazzite `b771fae6`) : `dnf5 swap --from-repo=copr:copr.fedorainfracloud.org:ublue-os:staging ostree ostree` — ostree patché ublue (bug flatpak)
- Mesa et kernel restent Fedora (fournis par Terra)
- `noopenh264` exclu de `*fedora*`/`updates*` (Bazzite `5161562`) : stub vide Fedora qui Obsoletes le vrai openh264 Cisco
- Bazzite : pipewire-*, bluez-*, xorg-x11-server-Xwayland, wireplumber-* (alignement i686/x86_64 fc44)
- Staging : scx-tools, scx-scheds, kf6-*, mesa*, mutter* — staging sans priorité : un build plus récent gagnerait au tri par version → dérive kf6 (hors versionlock qt6/plasma) ou scx (attendu du COPR cachyos) ; `ostree` reste installable (swap)
- Priorité Terra = 3 (haute)

### kernel — Installation du kernel OGC + akmods

- Récupération depuis `ghcr.io/ublue-os/akmods` et `akmods-extra` ; kernel installé depuis `/tmp/kernel-rpms/`
- Helper `/ctx/install-kmods` : vérifie l'existence de chaque RPM avant install (évite l'échec si un module disparaît de l'image akmods)
- Kmods communs : framework-laptop, kvmfr, openrazer, v4l2loopback, xone, wl
- Kmods extras : zenergy, gcadapter, evdi, kvmfr, new-lg4ff, hid-tmff2, t150-driver, hid-fanatecff, ryzen_smu, sc0710, nct6687d, system76, vhba
- Versionlock des versions ; scx-scheds depuis COPR bieszczaders/kernel-cachyos-addons

### mesa — Installation Mesa Terra (multilib fc44)

- Swap `mesa-filesystem` → terra-mesa ; installation x86_64 et i686 : dri-drivers, libEGL, libGL, libgbm, vulkan-drivers
- Terra fc44 : `LICENSE.dependencies` nommés par arch (`.i386`/`.x86_64`) → pas de conflit
- Versionlock des paquets Mesa

### nvidia — Pilotes via akmods (nvidia-install.sh ublue-os)

- Préalables : suppression `nvidia-gpu-firmware` (conflit proprio) ; activation terra-mesa (egl-wayland + Mesa i686) ; EGL Wayland 32/64 bits
- Appel : `AKMODNV_PATH="/tmp/rpms/nvidia"`, `MULTILIB=1`, `IMAGE_NAME="$SOURCE_IMAGE"` — gère driver, kmod, container-toolkit, supergfxctl (+ plasmoid Kinoite), SELinux, dracut, staging COPR
- Post-config : suppression ICD Nouveau, symlink libnvidia-ml, disable supergfxd
- Dracut : `omit_drivers` → `force_drivers` force `nvidia_peermem` (module datacenter NVLink/InfiniBand inutile desktop, échoue « Invalid argument ») → `dracut-pre-udev` +~26 s/boot → retiré de la conf (Bazzite, fix #4569)
- Services PM activés explicitement en boucle (preset RPM Fusion `70-nvidia.preset` pas appliqué de façon fiable en build container ; services absents du packaging nvidia-open → test d'existence, sans faire échouer le build) : nvidia-suspend, nvidia-resume, nvidia-hibernate, nvidia-suspend-then-hibernate, nvidia-powerd
- `nvidia-persistenced` désactivé (Bazzite `e93936b`) : maintient le pilote initialisé en permanence — conflit setups hybrides (supergfxctl, RTD3)
- PM (Bazzite, doc NVIDIA) : `nvidia-power.conf` /usr/lib/modprobe.d (`NVreg_EnableS0ixPowerManagement=1` + `NVreg_DynamicPowerManagement=0x02`) + `80-nvidia-pm.rules` udev (runtime PM auto au bind, suppression devices USB xHCI/UCSI qui empêchent la veille) — laptop surtout, inoffensif desktop
- `VK_hdr_layer` : pilotes closed uniquement, extraction manuelle du RPM
- `nvidia-modeset.conf` : copie /etc/modprobe.d/ → /usr/lib/modprobe.d/ (workaround Dracut, avec `[ -f ]`) — closed uniquement (open n'a pas ce fichier)
- Désactivation terra-mesa après installation

### rpm — Paquets RPM

- **Homebrew** : pas de RPM (`ublue-brew` déprécié, dernier build COPR 11/2025) — fourni par l'étape `FROM ghcr.io/ublue-os/brew:latest@sha256:<digest>` (digest épinglé, à bumper ponctuellement ; `brew-update.timer` maintient à jour côté client). Copie `/system_files/` : tarball `/usr/share/homebrew.tar.zst`, services brew-setup/update/upgrade, preset, intégration shell. Tarball SANS node/npm → `brew install node` à la demande (opencode2-install)
- **brew-setup.service activé explicitement** (09/2026) : les presets systemd ne sont évalués qu'à l'install de RPMs, JAMAIS au déploiement bootc → sans activation explicite, brew jamais extrait au premier boot (bug Bazzite #3788/#3817, même fix upstream). Oneshot idempotent (`ConditionPathExists=!/etc/.linuxbrew`)
- **profile.d gablue-brew.sh** : brew visible dans les shells NON-interactifs (le `/etc/profile.d/brew.sh` uBlue est gardé interactif) — session Plasma, `bash -lc`, OpenCode GUI. Exports `HOMEBREW_*`/`MANPATH`/`INFOPATH` + PATH en FIN (pas de shadowing système) ; garde `[ -d /home/linuxbrew/.linuxbrew ]` (1er boot), anti-doublon `HOMEBREW_PREFIX` + `case` PATH, POSIX-safe (sourcé par le DM). **Jamais patcher brew.sh** (réécrasé à chaque bump digest) → fichier dédié re-appliqué à chaque build
- **libxcrypt-compat** : `libcrypt.so.1` requis par Portable Ruby brew 4.0+ (fc44 ne fournit plus que `.so.2`) — sinon « Failed to upgrade Homebrew Portable Ruby! »
- **CLI** : fswatch, btop, fastfetch, git, atuin, tldr, amdsmi, jq, zoxide, bpftune-gaming (fork gaming bpftune, Terra 44, Bazzite `4333b30` — détection UDP burst des jeux ; service reste `bpftune.service`), etc.
- **Réseau** : tailscale, rar
- **Multimédia** : yt-dlp, openh264 x86_64+i686 (vrai codec Cisco, negativo17 `fedora-multimedia`, `--allowerasing` — voir exclusion `noopenh264`)
- **qemu-guest-agent (toutes variantes)** : ~300 Ko, inoffensif bare metal (lié au device virtio-serial), utile en VM (IP virt-manager, graceful shutdown, snapshots) — requis pour les ISOs live dans libvirt ; activé par preset Fedora
- **DX** : docker-ce, libvirt, virt-manager, virt-viewer, virt-install, swtpm, guestfs-tools, python3-libguestfs, qemu-kvm-core, qemu-system-ppc/m68k/arm/aarch64-core (émulation rétro), spice-server, modules QEMU externes (display qxl/virtio, audio spice/pipewire/pa/alsa, usb, ui, char)
  - Modules virtio/net/pci/vfio statiques dans qemu-system-x86-core (tiré par qemu-kvm-core) : `--setopt=install_weak_deps=False` empêche l'auto-install des modules externes en dépendances faibles
  - `python3-libguestfs` requis explicitement (Optional du groupe Fedora `Virtualization`) : import `guestfs` silencieux sinon (try/except) → inspection/resize VM désactivée
- **Gaming** : sunshine ; terra-mangohud + terra-gamescope + terra-gamescope-libs (.x86_64/.i686 — migrés du COPR bazzite vers Terra, Bazzite `a390d80` : même spec/packager KyleGospo, `Provides: mangohud`, chemins/binaires identiques) ; steam-devices (règles udev Valve depuis Fedora, remplace `8bitdo-udev-rules` Terra — règles 8BitDo upstream 10/2025, Bazzite `e5dbc91` ; Gablue n'installe pas le RPM Steam → install explicite)
  - **Fix conflit mangohud i686** : Recommends conditionnelle `(mangohud(x86-32) if glibc(x86-32))` de terra-mangohud → dnf5 tirait le COPR `mangohud.i686` (seul *nommé* mangohud i686 ; terra-mangohud.i686 ne Provides pas `mangohud(x86-32)`) → conflit fichiers (mangoapp, mangohudctl, libMangoHud*.so) → Transaction failed. Fix : `--setopt=install_weak_deps=False` sur cette commande (Bazzite le fait global dans dnf.conf ; Gablue préfère le setopt local)
  - **Piège archs explicites** : arguments arch-less satisfaits par le i686 tiré en dépendance (require `terra-gamescope-libs = EVR` arch-less) → les x86_64 (layer Vulkan WSI `libVkLayer_FROG_gamescope_wsi_x86_64.so`) jamais installés sans le voir. Toujours archs explicites sur les paquets multilib (comme Bazzite)
- **BTRFS** : snapper, btrfs-assistant (non activés par défaut)
- **KDE (Kinoite)** : okular, gwenview, kcalc, yakuake
- **Polices** : nerd-fonts · **Runtime** : patch, bzip2, sqlite, uv
- **Python** : python3-evdev, python3-pyside6 (python3-uinput retiré : cassé sous Python 3.14 — distutils supprimé, `mouse.py` remplacé par le binaire C `gamepadshortcuts-mouse`)
- **SELinux** : checkpolicy, selinux-policy-devel
- **Libs 32-bit Wine/Proton** : fontconfig, freetype, X11 (composite, cursor, damage, fix, i, inerama, randr, render, tst, v), Wayland (epoxy, decor, cursor, egl), core (gnutls, unwind, cups, openldap), audio (pulseaudio, pipewire + libs, FAudio, alsa, openal, ogg, vorbis, flac, sndfile), vulkan-loader (terra-mesa), vidéo (libva, libvdpau)
  - **Piège multilib** : upgrade `libva libvdpau` x86_64 AVANT l'install i686 (même pattern pipewire-libs) — fichiers %doc en conflit si arches divergentes, et `dnf5 install` ne upgrade pas un paquet déjà installé. L'alignement était accidentel (cascade ffmpeg i686 du libheif negativo, morte depuis son retrait de fedora-multimedia) → explicite obligatoire

**Upgrade initial restreint** (toutes variantes) : `dnf5 -y upgrade --refresh --repo=fedora --repo=updates` AVANT toute installation — image de base jusqu'à 48 h de retard ; dépôts officiels seuls (exclusions copr protègent mesa/kernel, NVIDIA vient des RPMs akmods) ; avant le versionlock qt6-*/plasma-* (Bazzite `c9ef733`, anti-dérive ABI Qt)

Supprimés : firefox, firefox-langpacks, htop, plasma-welcome-fedora, plasma-welcome, plasma-discover-rpm-ostree (Kinoite)

### pypi — Packages Python sans équivalent RPM

- `terminaltexteffects` via `uv` (appelé après rpm)
- **Conflit site-packages** : un RPM peut poser un fichier/symlink à n'importe quel niveau de `/usr/local/lib/python3.14/site-packages/` (transition majeure Python) → `uv pip install` échoue (`File exists, os error 17`) ; le script remonte la hiérarchie, supprime ce qui n'est pas un répertoire, `mkdir -p` avant install

### build-c / build-gwine — Compilation des sources

**build-c** (après pypi) :
- Compile `/src/gamepadshortcuts` (gamepadshortcuts + gamepadshortcuts-mouse + kbdnav, même Makefile), `/src/ds2xbox`, `/src/gablue-isomount`
- `make -C <dir> install DESTDIR=` ; sources nettoyées après (`rm -rf /src/<dir>`) ; `dbus-devel` désinstallé après (inutile en image finale)

**build-gwine** (après build-c) :
- Assemble le standalone depuis `/src/gwine-launcher/` : `build.sh` concatène les ~60 fichiers `lib/`, embarque les shims overlayfs (`composefs_statfs_shim.so` 32/64 bits, base64), installe `/usr/bin/gwine` + completions bash/zsh ; sources nettoyées
- **IMPORTANT** : toute modif de `lib/` nécessite un rebuild d'image pour être effective. Le gwine assemblé sert au pack cache de l'ISO → reconstruire l'image AVANT l'ISO
- Architecture interne → `src/gwine-launcher/AGENTS.md`

### post-install

- Permissions +x, setcap gamescope, modules SELinux (.te → .pp), binaires externes (retroplayer, zxtune), branding os-release, config système (tuned, bluetooth, pipewire, timers), désactivation dépôts, nettoyage .desktop, config DX (iptables, NetworkManager), MIME par défaut (Windows.desktop, LGP.desktop)
- **Maj auto** : `AutomaticUpdatePolicy=stage` dans `/etc/rpm-ostreed.conf` (copié depuis ublue-os-update-services) + timers flatpak/rpm-ostree samedi 04:00 (`RandomizedDelaySec=10m`)
- **Linuxbrew** : `/home/linuxbrew/.linuxbrew/bin` au secure_path sudo
- `toggle-updates` upstream (RPM ublue-os-just) intact (flatpak + rpm-ostree) — variante globale avec timers brew = `toggle-updates-all` (60-custom.just)
- **fstrim** (Bazzite `8a76282f`, silverblue#689) : drop-in remplace ExecStart par `fstrim --listed-in /proc/self/mountinfo` — composefs : `/etc/fstab` ne reflète pas les montages réels. **Divergence volontaire** : `ExecStart=` vide avant la nouvelle ligne (Type=oneshot — sans reset, le drop-in ajoute une 2e exécution au lieu de remplacer ; omission dans le drop-in Bazzite)

**Correction composefs** (toutes variantes) :
- LD_PRELOAD `gablue-composefs-fix.so` (~2,6 Ko, `src/composefs-fix/`) intercepte statfs/statfs64 : overlay composefs `/` rapporte 0 blocs libres → redirection `/`, `/home`, `/home/*` → `/var/home` (btrfs)
- Injection : sed `.desktop` Dolphin (post-install) + drop-in user `plasma-dolphin.service.d` (activation D-Bus `org.freedesktop.FileManager1` démarre `dolphin --daemon` hors .desktop) + réinjection dans `gablue-isomount` (execlp direct)
- Enfants (kioworker) héritent de l'env → un Dolphin lancé sans le hook propage le bug à toute la session (jusqu'au reboot). Non couvert (rare, accepté) : lancement manuel terminal

**plasmalogin settle udev** (TEMPORAIRE — à supprimer quand Fedora/KDE corrige) :
- `udevadm settle --timeout=10` du plasmalogin.service = écran noir 10 s sur certaines cartes mères (queue udev jamais vide) → drop-in `90-gablue-settle.conf` → `/usr/libexec/gablue-wait-devices`
- Script : attend un connecteur `/sys/class/drm/card*-*/status` = connected (le 1er suffit, écrans suivants hotpluggés) + `/dev/input/event*`, puis 1 s fixe (docks/HID lents), timeout 5 s (headless = sortie au timeout). Attendre la carte seule ne suffit pas : GPU récents (ex. RX 9060 XT/gfx12) → KWin greeter sans output → écran noir
- ~10,2 s → ~1,1 s (fixe upstream KDE `a8c752fe` trop conservateur)

**gablue-bigscreen-swap-session** (remplace `/usr/bin/plasma-bigscreen-swap-session`, appelé par QProcess du script C++ bigscreen) :
- **Env** : QProcess détaché = env minimal → fallbacks exportés avant tout (`XDG_RUNTIME_DIR`, `DBUS_SESSION_BUS_ADDRESS`, `WAYLAND_DISPLAY`, `QT_QPA_PLATFORM=wayland`, `LANG`) ; sans `QT_QPA_PLATFORM=wayland`, kscreen-doctor xcb → coredump. Au retour, saved-env sourcé EN PREMIER
- **Anti double-invocation** : signal bigscreen émis 2× (~2 s d'écart) → le 2e appel repartait en bigscreen et écrasait la restauration → flock (fd 9) + cooldown 6 s (`last-swap`) ; processus backgroundés ferment fd 9 (`plasmashell --replace` infini)
- **Aller** : sauvegarde env + settings KWin (`BorderlessMaximizedWindows`, `Placement`, `NoPlugin`) ; source `plasma-bigscreen-common-env` ; kwinrc bigscreen (max sans déco, pas de plugin) ; mirroring secondaires → principal (`kscreen-doctor output.X.mirror.Y`) ; inputhandler via `kioclient exec` (mécanisme natif permissions Wayland) ; `plasmashell --replace` ; +2 s : maximise tout + `noBorder = true` (script KWin 6)
- **Retour** : kill inputhandler (TERM puis KILL) ; restaure settings (supprime les clés absentes avant) ; relance `plasma-xwaylandvideobridge.service` (tué par bigscreen `HomeScreen.qml` — hack qui casse X11 au retour) ; restaure env (dont `XDG_CONFIG_DIRS`) ; `qdbus reconfigure` ; annule mirroring (`mirror.none`) ; `plasmashell --replace` ; +2 s : déco + `setMaximize(false, false)` (frameGeometry ignoré sur fenêtre maximisée) puis frameGeometry 80 % centré
- Détection mode : `pgrep -f plasma-bigscreen-inputhandler` (plus fiable que `PLASMA_BIGSCREEN_LAUNCH_REASON`, non héritée via KLauncher)

**gablue-bigscreen-session-init** (autostart KDE, toutes sessions) :
- Détection session native : `PLASMA_PLATFORM=mediacenter` (sourcé par `plasma-bigscreen-common-env` avant autostart)
- Session Plasma normale : `~/.cache/plasma-bigscreen/kscreen-mirrored.txt` résiduel → annule le mirroring listé + supprime le fichier (après init KWin)
- Session bigscreen native : symlink blacklist (`~/.config/applications-blacklistrc` → `/etc/xdg/`) ; attente KWin (`kscreen-doctor --json`, 10 s max) ; mirroring secondaires ; sauvegarde des outputs mirrorés dans `kscreen-mirrored.txt` (partagé avec swap-session)
- Actions swap (fenêtres, KWin, inputhandler) : uniquement dans swap-session

### systemd

- **Activés (toutes variantes)** : rpm-ostreed-automatic, flatpak-update, cec-poweroff-tv, cec-active-source, dmemcg-booster, bpftune (Bazzite `4333b30`), brew-setup (voir section rpm)
- **Désactivés** : scx_loader, tailscaled, displaylink
- **Masqués** : systemd-remount-fs, flatpak-add-fedora-repos (le service natif flatpak réajoute fedora/fedora-testing tant que `/var/lib/flatpak/.fedora-initialized` n'existe pas, annulant le kickstart — on garde Flathub seul via `/etc/flatpak/remotes.d/`)
- **Conditionnels (DX)** : ublue-os-libvirt-workarounds, gablue-dx-groups, incus-workaround

### initramfs

- Détection version kernel via dnf5 repoquery ; génération dracut avec options ostree et fido2 ; permissions 0600

### cleanup / finalize

**cleanup** (après chaque RUN) :
- Suppression `/tmp/*`, `/var/log/dnf5.log`, `/boot/*`
- PAS de `dnf5 clean all` (le cache mount `/var/cache` n'entre pas dans l'image, le nettoyer détruirait le cache persistant)
- PAS de `ostree container commit` (inutile avec bootc/rechunk)

**finalize** (une seule fois, à la fin) :
- `dnf5 config-manager setopt keepcache=0`
- Nettoyage `/var/*` sauf cache
- Migration users/groups → `/usr/lib/passwd` + `/usr/lib/group` via `relocate_accounts` (Bazzite `5b1a399`) : nettoie aussi `/etc/shadow` + `/etc/gshadow`, build en échec si entrée non persistée
- Nettoyage fichiers de verrou + `/usr/etc` ; PAS de `ostree container commit` (le rechunk du workflow s'en occupe)

### cpuid-fault — Module kernel CPUID faulting

Compile `cpuid_fault_emulation` (`src/cpuid-fault/`) : émule le CPUID faulting sur CPU AMD sans support natif (AM4, Steam Deck) — natif sur Intel 4th gen+ et Ryzen 7000+ (inutile).
- `make -C /usr/src/kernels/${KVER} M=/src/cpuid-fault modules`
- AMD SVM uniquement (ignoré sur Intel) ; KVM doit être déchargé avant chargement
- Signature Secure Boot via `/run/secrets/gablue-kmod-key` (secret CI `GABLUE_KMOD_KEY`) ; clé absente (build local) → compilé non signé ; certificat `gablue-kmod.der` dans `/etc/pki/akmods/certs/`
- Persistance opt-in (`ujust cpuid-emu-on/off`) : `on` écrit `/etc/modprobe.d/gablue-cpuid-emu.conf` (blacklist kvm_amd — bloque l'auto-charge udev, modprobe explicite possible) + `/etc/modules-load.d/` (chargé à chaque boot) ; écrit SEULEMENT après chargement réussi (pas d'échec boot si SVM off dans le BIOS) ; `off` supprime, décharge, recharge kvm_amd
