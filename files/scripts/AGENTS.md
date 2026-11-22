# Scripts de build (files/scripts/) — gablue

> Détail de chaque script appelé par les RUN du Containerfile-gablue, dans
> l'ordre du Containerfile. Conventions transverses (`set -eoux pipefail`,
> Bash, Containerfile), commandes de build, vérification de l'image, gestion
> des erreurs, tests et dépannage → `files/AGENTS.md`.
> Fichiers système posés par post-install → `files/system/AGENTS.md`.
> Même règle que la racine : à jour avant tout commit touchant files/scripts/
> ou le comportement d'une étape RUN.

## Scripts de build détaillés

### copr — Configuration des dépôts

- **keepcache=1** activé ici (désactivé dans finalize) → cache DNF persistant entre builds
- **COPR** : ublue-os/bazzite, ublue-os/bazzite-multilib, ublue-os/staging, ublue-os/packages, che/nerd-fonts, hikariknight/looking-glass-kvmfr, lizardbyte/beta
  - Migration bazzite-org → ublue-os (08/2026) : `bazzite-org/bazzite` abandonné ; `ublue-os/bazzite` fournit bees 0.11 (requis par `--throttle-factor` de `configure-beesd`) ; `bazzite-org/rom-properties` retiré → rom-properties vient de Terra (comme Bazzite)
- **Tiers** : Tailscale, Negativo17
- **Terra (FyraLabs)** : terra-release, terra-release-extras, terra-release-mesa
  - Exclusion `terra-glfw*` : Conflicts cross-arch → casse l'install de `mangohud.i686` (glfw doit rester Fedora)
  - Exclusion `zlib-devel*` (11/2026) : terra-extras publie zlib 1.4 sous ce nom, en `Conflicts` avec `zlib-ng-compat` (fedora) ; sans exclusion, tout `dnf5 install zlib-devel` échoue (build-c, 5 images). Les scripts demandent `zlib-ng-compat-devel` explicitement
  - Exclusion `waydroid*` (09/2026) : `waydroid-nvidia` (terra-extras) Provides `libvirglrenderer.so.1` + Obsoletes `waydroid` — avec Terra priority 3, dnf5 l'utiliserait dans la transaction virtualisation DX (`qemu-device-display-*-gl`) : fork waydroid complet + lxc + `waydroid-container.service` activé, virglrenderer Fedora évincé (DX uniquement ; exclusion → retombe sur virglrenderer Fedora, validé conteneur)

Exclusions importantes :
- **Swap ostree** (Bazzite `b771fae6`) : `dnf5 swap --from-repo=copr:copr.fedorainfracloud.org:ublue-os:staging ostree ostree` — ostree patché ublue (bug flatpak)
- Mesa et kernel restent Fedora (fournis par Terra)
- `noopenh264` exclu de `*fedora*`/`updates*` (Bazzite `5161562`) : stub vide Fedora qui Obsoletes le vrai openh264 Cisco
- Bazzite : pipewire-*, bluez-*, xorg-x11-server-Xwayland, wireplumber-* (alignement i686/x86_64 fc44)
- Staging : scx-tools, scx-scheds, kf6-*, mesa*, mutter* — staging sans priorité : un build plus récent gagnerait au tri par version → dérive kf6 (hors versionlock qt6/plasma) ou scx (fourni par Terra) ; `ostree` reste installable (swap)
- Priorité Terra = 3 (haute)

### kernel — Installation du kernel OGC + akmods

- Récupération depuis `ghcr.io/ublue-os/akmods` et `akmods-extra` ; kernel installé depuis `/tmp/kernel-rpms/`
- Helper `/ctx/install-kmods` : vérifie l'existence de chaque RPM avant install (évite l'échec si un module disparaît de l'image akmods)
- Kmods communs : framework-laptop, kvmfr, openrazer, v4l2loopback, xone, wl
- Kmods extras : zenergy, gcadapter, evdi, kvmfr, new-lg4ff, hid-tmff2, t150-driver, hid-fanatecff, ryzen_smu, sc0710, nct6687d, system76, vhba
- **displaylink userspace installé explicitement** (10/2026) : embarqué par akmods-extra à côté de libevdi (`/tmp/rpms/extra/displaylink*.rpm`, démon + `displaylink.service`) — il n'arrivait avant que via une dep faible de libevdi depuis fedora-multimedia (negativo17) ; disparition silencieuse du repo = docks DisplayLink sans démon + échec systemd (disable d'une unité absente) → install explicite, skip propre par `install-kmods` si akmods-extra cesse de l'embarquer
- Versionlock des versions ; scx-scheds/scx-tools depuis Terra (Bazzite `e5dd121c`, 10/2026 — plus de COPR `bieszczaders/kernel-cachyos-addons`)
- Terra fc44 : scx 1.1.3-1 quand le COPR cachyos servait 1.1.4-1 → un patch de retard accepté en échange d'un dépôt tiers en moins (revérifier la version avant de s'en étonner)

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

- **Homebrew** : pas de RPM (`ublue-brew` déprécié, dernier build COPR 11/2025) — fourni par l'étape `FROM ghcr.io/ublue-os/brew:latest@sha256:<digest>` (digest épinglé, à bumper ponctuellement ; horaires et sérialisation des timers `brew-*` côté client → § post-install). Copie `/system_files/` : tarball `/usr/share/homebrew.tar.zst`, services brew-setup/update/upgrade, preset, intégration shell. Tarball SANS node/npm → `brew install node` à la demande (opencode2-install)
- **brew-setup.service activé explicitement** (09/2026) : les presets systemd ne sont évalués qu'à l'install de RPMs, JAMAIS au déploiement bootc → sans activation explicite, brew jamais extrait au premier boot (bug Bazzite #3788/#3817, même fix upstream). Oneshot idempotent (`ConditionPathExists=!/etc/.linuxbrew`)
- **profile.d gablue-brew.sh** : brew visible dans les shells NON-interactifs (le `/etc/profile.d/brew.sh` uBlue est gardé interactif) — session Plasma, `bash -lc`, OpenCode GUI. Exports `HOMEBREW_*`/`MANPATH`/`INFOPATH` + PATH en FIN (pas de shadowing système) ; garde `[ -d /home/linuxbrew/.linuxbrew ]` (1er boot), anti-doublon `HOMEBREW_PREFIX` + `case` PATH, POSIX-safe (sourcé par le DM). **Jamais patcher brew.sh** (réécrasé à chaque bump digest) → fichier dédié re-appliqué à chaque build
- **libxcrypt-compat** : `libcrypt.so.1` requis par Portable Ruby brew 4.0+ (fc44 ne fournit plus que `.so.2`) — sinon « Failed to upgrade Homebrew Portable Ruby! »
- **CLI** : fswatch, btop, fastfetch, git, atuin, tldr, amdsmi, jq, zoxide, bpftune-gaming (fork gaming bpftune, Terra 44, Bazzite `4333b30` — détection UDP burst des jeux ; service reste `bpftune.service`), etc.
- **Réseau** : tailscale, rar
- **Multimédia** : yt-dlp, openh264 x86_64+i686 (vrai codec Cisco, negativo17 `fedora-multimedia`, `--allowerasing` — voir exclusion `noopenh264`)
- **qemu-guest-agent (toutes variantes)** : ~300 Ko, inoffensif bare metal (lié au device virtio-serial), utile en VM (IP virt-manager, graceful shutdown, snapshots) — requis pour les ISOs live dans libvirt ; activé par preset Fedora
- **DX** : docker-ce, libvirt, virt-manager, virt-viewer, virt-install, swtpm, guestfs-tools, python3-libguestfs, qemu-kvm-core, qemu-system-ppc/m68k/arm/aarch64-core (émulation rétro), spice-server, modules QEMU externes (display qxl/virtio, audio spice/pipewire/pa/alsa, usb, ui, char)
  - **Trim ROCm** : après `rocm-hip rocm-opencl rocm-clinfo rocm-smi`, les paquets de compilateur (`rocm-llvm-static` 2,2 Go, llvm/clang-devel, lld…) sont retirés puis versionlock-exclus. Retirer **seulement les installés** (`rpm -q` filtré) : les dépendances ROCm bougent d'une release à l'autre — en fc45 (ROCm 7.2.2) `rocm-libc++-devel` n'est plus tiré, et un `rpm -e` en dur échouait sur « package rocm-libc++-devel is not installed » (les 2 variantes DX rouges, build 06/11/2026)
  - Modules virtio/net/pci/vfio statiques dans qemu-system-x86-core (tiré par qemu-kvm-core) : `--setopt=install_weak_deps=False` empêche l'auto-install des modules externes en dépendances faibles
  - `python3-libguestfs` requis explicitement (Optional du groupe Fedora `Virtualization`) : import `guestfs` silencieux sinon (try/except) → inspection/resize VM désactivée
- **Gaming** : sunshine ; terra-mangohud + terra-gamescope + terra-gamescope-libs (.x86_64/.i686 — migrés du COPR bazzite vers Terra, Bazzite `a390d80` : même spec/packager KyleGospo, `Provides: mangohud`, chemins/binaires identiques) ; steam-devices (règles udev Valve depuis Fedora, remplace `8bitdo-udev-rules` Terra — règles 8BitDo upstream 10/2025, Bazzite `e5dbc91` ; Gablue n'installe pas le RPM Steam → install explicite) ; game-devices-udev (Terra, Bazzite `0ee37802`, 10/2026 — règles udev `/usr/lib/udev/rules.d/71-*-gdu.rules` manettes/volants tiers)
  - **Conflit ublue-os-udev-rules (fc44 uniquement)** : ublue vendore game-devices-udev 0.23 (spec : Source1 = tarball codeberg, tous les `*.rules` installés) et fournit déjà `71-uinput-dev-early-creation.rules` → `dnf5 install` échoue sur ce fichier unique (constaté 26/10/2026, 5 variantes rouges 3 jours). Pas de `--excludepath` en dnf5 (contrairement à dnf4) → install par `dnf5 download` + `rpm -i --nodeps --excludepath=…71-uinput…`, ublue gardant la règle
  - **À révoquer seulement si ublue-os-udev-rules part** : ublue-os/main cesse de l'installer au-delà de Fedora 44 (`build_files/install.sh`), MAIS Gablue l'installe lui-même (ligne « ublue-os-udev-rules » du bloc uBlue) — le paquet f45 du COPR fournit toujours `71-uinput-dev-early-creation.rules` (vérifié 28/10/2026), donc le contournement reste requis en fc45 ; retour à `dnf5 -y install game-devices-udev` uniquement si on retire ublue-os-udev-rules
  - **Fix conflit mangohud i686** : Recommends conditionnelle `(mangohud(x86-32) if glibc(x86-32))` de terra-mangohud → dnf5 tirait le COPR `mangohud.i686` (seul *nommé* mangohud i686 ; terra-mangohud.i686 ne Provides pas `mangohud(x86-32)`) → conflit fichiers (mangoapp, mangohudctl, libMangoHud*.so) → Transaction failed. Fix : `--setopt=install_weak_deps=False` sur cette commande (Bazzite le fait global dans dnf.conf ; Gablue préfère le setopt local)
  - **Piège archs explicites** : arguments arch-less satisfaits par le i686 tiré en dépendance (require `terra-gamescope-libs = EVR` arch-less) → les x86_64 (layer Vulkan WSI `libVkLayer_FROG_gamescope_wsi_x86_64.so`) jamais installés sans le voir. Toujours archs explicites sur les paquets multilib (comme Bazzite)
- **BTRFS** : snapper, btrfs-assistant (non activés par défaut)
- **KDE (Kinoite)** : okular, gwenview, kcalc, yakuake
- **Polices** : nerd-fonts · **Runtime** : patch, bzip2, sqlite, uv
- **Python** : python3-evdev, python3-pyside6 (python3-uinput retiré : cassé sous Python 3.14 — distutils supprimé, `mouse.py` remplacé par le binaire C `gpad-mouse`)
- **SELinux** : checkpolicy, selinux-policy-devel
- **Libs 32-bit Wine/Proton** : fontconfig, freetype, X11 (composite, cursor, damage, fix, i, inerama, randr, render, tst, v), Wayland (epoxy, decor, cursor, egl), core (gnutls, unwind, cups, openldap), audio (pulseaudio, pipewire + libs, FAudio, alsa, openal, ogg, vorbis, flac, sndfile), vulkan-loader (terra-mesa), vidéo (libva, libvdpau)
  - **Piège multilib** : upgrade `libva libvdpau` x86_64 AVANT l'install i686 (même pattern pipewire-libs) — fichiers %doc en conflit si arches divergentes, et `dnf5 install` ne upgrade pas un paquet déjà installé. L'alignement était accidentel (cascade ffmpeg i686 du libheif negativo, morte depuis son retrait de fedora-multimedia) → explicite obligatoire
- **H.264/HEVC matériel Intel** : `libva-intel-media-driver` remplacé par le build **negativo17** (`fedora-multimedia`) via `distro-sync` ciblé — celui de Fedora (source `intel-media-driver-free`) est construit sans les noyaux brevetés, donc `vainfo` n'expose **ni H.264 ni HEVC, même en décodage**, et `ffmpeg -c:v h264_vaapi` échoue (constaté 01/11/2026 sur UHD 630 CometLake). Aligné sur Aurora (`build_files/base/01-packages.sh`) ; Terra ne fournit pas ce paquet. L'epoch 1 de negativo le garde devant celui de Fedora sans versionlock. Concerne **`gremote`/KRdp** : `krdpserver` encode en H.264 via **VAAPI** — avec le paquet de Fedora il retombe en x264 **logiciel** en silence (mesuré 02/11/2026 : `[libx264]` dans le journal et des centaines de ticks CPU, contre ~2 % d'un cœur et aucune trace de x264 avec le build negativo17)

**Upgrade initial restreint** (toutes variantes) : `dnf5 -y upgrade --refresh --repo=fedora --repo=updates` AVANT toute installation — image de base jusqu'à 48 h de retard ; dépôts officiels seuls (exclusions copr protègent mesa/kernel, NVIDIA vient des RPMs akmods) ; avant le versionlock qt6-*/plasma-* (Bazzite `c9ef733`, anti-dérive ABI Qt)

Supprimés : firefox, firefox-langpacks, htop, plasma-welcome-fedora, plasma-welcome, plasma-discover-rpm-ostree (Kinoite)

### pypi — Packages Python sans équivalent RPM

- `terminaltexteffects` via `uv` (appelé après rpm)
- **Conflit site-packages** : un RPM peut poser un fichier/symlink à n'importe quel niveau de `/usr/local/lib/pythonX.Y/site-packages/` (transition majeure Python) → `uv pip install` échoue (`File exists, os error 17`) ; le script remonte la hiérarchie, supprime ce qui n'est pas un répertoire, `mkdir -p` avant install. `X.Y` est déduit du Python de l'image (`python3 -c`), jamais codé en dur : un chemin figé visait 3.14 et serait resté muet sur 3.15 en fc45

### build-c / build-gwine / build-manager / build-manager-tray / build-glauncher / build-gtracker / build-vgmstream / build-tvqt / build-apk-installer / build-lgplaunch / build-share-daemon / build-share — Compilation des sources

**build-c** (après pypi) :
- Compile `/src/gablue-pad` (`make install` : gpad-trigger, gpad-mouse, gpad-inhibit, gpad-rumble, gpad-login, gpad-shot + l'arborescence de données dans `/usr/libexec/gablue-pad`, dont `login-bridge.sh`), `/src/ds2xbox`, `/src/gablue-dualsense` (`gablue-dualsense-ctl`, hidapi), `/src/gablue-isomount`
- `gpad-shot` (capture d'écran) lie `libdbus` (comme `gpad-inhibit`) et `zlib` : `zlib-ng-compat-devel` est ajouté à la liste dnf pour ça seul — contrat → `src/gablue-pad/AGENTS.md`. **Ne jamais demander `zlib-devel`** : ce nom virtuel résout chez terra-extras (priority 3) vers un vrai zlib 1.4 en conflit avec `zlib-ng-compat`, déjà installé → transaction refusée (5 images rouges, 21/11/2026) ; le nom fedora est explicite et le dépôt terra l'exclut aussi (`copr`)
- `make -C <dir> install DESTDIR=` ; sources nettoyées après (`rm -rf /src/<dir>`)
- `gcc`/`binutils`/`glibc-devel` viennent de l'image de base (`cpuid-fault` compile un module avant, sur toutes les variantes) et **restent dans l'image finale** : les retirer libère ~45 Mo compressés mais supprime `gcc`/`cc` du poste (brew depuis source, extensions pip, modules noyau à la main) — décision du 14/11/2026. `hidapi-devel`/`dbus-devel` non plus ne sont pas retirés (même raison : cohérence d'un poste où on compile)
- **`pkgconf-pkg-config` est intouchable** : `kmod` (donc `systemd-udev`) exige `/usr/bin/pkg-config` ; l'ajouter à une liste de retrait fait échouer la transaction dnf (vérifié 14/11/2026). `binutils` non plus (`supermin` → `libguestfs`)

**build-gwine** (après build-c) :
- Assemble le standalone depuis `/src/gwine-launcher/` : `build.sh` concatène les ~60 fichiers `lib/`, embarque les shims overlayfs (`composefs_statfs_shim.so` 32/64 bits, base64), installe `/usr/bin/gwine` + completions bash/zsh ; sources nettoyées
- **IMPORTANT** : toute modif de `lib/` nécessite un rebuild d'image pour être effective. Le gwine assemblé sert au pack cache de l'ISO → reconstruire l'image AVANT l'ISO
- Architecture interne → `src/gwine-launcher/AGENTS.md` ; modules et points
  techniques → `src/gwine-launcher/lib/AGENTS.md`

**build-manager** (après build-gwine) :
- Installe le paquet Python `/src/gablue-manager/gablue_manager/` dans `/usr/lib/gablue-manager/` et le lanceur `src/gablue-manager/gablue-manager` en `/usr/bin/gablue-manager` (755) ; sources nettoyées
- **Pas de `.pyc` dans l'image** (`__pycache__` supprimé) : ils seraient liés au Python du build, pas à celui du poste, et `/usr` est en lecture seule sur composefs
- Toute modif du paquet nécessite un rebuild d'image pour être effective — le lanceur ne lit que `/usr/lib/gablue-manager`
- Architecture interne → `src/gablue-manager/AGENTS.md`

**build-manager-tray** (étape `manager-tray-build`, avant l'image finale) :
- Même schéma que `build-share-daemon` (étape jetée, toolchain hors image, `CARGO_HOME=/cargo`, caches `/cargo` + `/build/target`, `cargo build --release --locked`, copies fichier par fichier) : l'étape finale ne reçoit que `/out/gablue-manager-tray` par `COPY --from`
- Livre `/usr/libexec/gablue-manager-tray` (~2,8 Mo), l'icône StatusNotifierItem du manager : `gablue-manager --tray` lui passe la main par `os.execv`, donc aucun Python ne reste en mémoire (icône Qt mesurée à 110 Mo, la Rust à 4 Mo)
- Le Containerfile copie désormais `gablue_manager/` et le lanceur **séparément** (jamais `tray/target/`, ~centaines de Mo)
- Contrat de l'icône → `src/gablue-manager/AGENTS.md`

**build-glauncher** (étape `glauncher-build`, avant l'image finale) :
- Même schéma que `build-manager-tray` / `build-gpad` (étape jetée, `CARGO_HOME=/cargo`, caches `/cargo` + `/build/target`, `cargo build --release --locked`) : l'étape finale ne reçoit que `/out/glauncher` par `COPY --from=glauncher-build /out/glauncher /usr/bin/glauncher`
- `dnf5 install cargo rust gcc` dans l'étape jetée ; `rusqlite` est en mode `bundled` (SQLite C voyage dans la caisse)
- **`assets/` doit être copié avec les sources** : glyphes de manette, étoile des favoris, icône et cover d'exemple sont embarqués par `include_bytes!` — sans lui, la compilation échoue
- Le launcher finit en `/usr/bin/glauncher` (binaire iced ~25 Mo) ; `.desktop` et icône viennent de `files/system/all/usr/share/{applications,icons/hicolor/scalable/apps}`
- Contrat de l'application → `src/glauncher/AGENTS.md`

**build-gtracker** (étape `gtracker-build`, avant l'image finale) :
- Même schéma que `build-glauncher` (étape jetée, `CARGO_HOME=/cargo`, caches `/cargo` + `/build/target`, `cargo build --release --locked` sur le *workspace*) : l'étape finale ne reçoit que `/out/gtracker` par `COPY --from=gtracker-build /out/gtracker /usr/bin/gtracker`
- `dnf5 install cargo rust gcc alsa-lib-devel pkg-config` : `cpal` (rodio) construit contre ALSA — en local sur Kinoite les en-têtes manquent, d'où le bricolage `alsa.pc` de `src/gtracker/build.sh`, inutile dans l'image
- **Rien à installer pour Wayland** : `eframe` est compilé avec la seule fonctionnalité `wayland`, et winit charge `libwayland-client`/`libxkbcommon` par `dlopen` à l'exécution (`readelf -d` ne montre aucune liaison) — les libs runtime de l'image suffisent. Ne pas remettre la fonctionnalité `x11` : un client XWayland échappe au rattachement `.desktop` de KWin et ignore `with_maximized`
- **`assets/` doit être copié avec les sources** : l'icône est embarquée par `include_bytes!`
- `.desktop` et icône viennent de `files/system/all/usr/share/{applications,icons/hicolor/scalable/apps}` ; l'app_id Wayland est fixé dans `main.rs` (sinon KWin affiche l'icône générique)
- Contrat de l'application → `src/gtracker/AGENTS.md`

**build-vgmstream** (étape `vgmstream-build`, avant l'image finale) :
- Pas de paquet Fedora : télécharge l'archive du tag `r2168` depuis GitHub, **vérifie son sha256** puis compile (cmake, étape jetée) → `/out/vgmstream-cli`, copié en `/usr/bin/vgmstream-cli`
- `USE_VORBIS=ON` + `USE_MPEG=ON` obligatoires : les banques FSB5 des sets Switch/Wii U sont du Vorbis (mesuré 20/11 : 39/207 fichiers lisibles sans libvorbis, 207/207 avec) ; mpg123 pour le MP3 en conteneur
- `USE_FFMPEG=OFF` : les libs runtime de l'image viennent de negativo17, un `ffmpeg-devel` Fedora donnerait un soname différent → binaire qui ne charge pas. Prix : Opus dans un FSB5
- Binaire ~2,9 Mo, n'ajoute aucune lib (libvorbis/mpg123-libs sont déjà dans l'image)
- Détail et contrat → `src/gtracker/AGENTS.md`

**build-tvqt** (après build-manager) :
- Installe le paquet Python `/src/tvqt/gablue_tv/` dans `/usr/lib/tvqt/` et le lanceur `src/tvqt/tvqt` en `/usr/bin/tvqt` (755) ; sources nettoyées
- Mêmes règles que build-manager : pas de `.pyc` dans l'image, rebuild d'image obligatoire (le lanceur ne lit que `/usr/lib/tvqt`)
- Architecture interne → `src/tvqt/AGENTS.md`

**build-apk-installer** (après build-tvqt) :
- Installe le paquet Python `/src/gablue-apk-installer/gablue_apk_installer/` dans `/usr/lib/gablue-apk-installer/` et le lanceur `src/gablue-apk-installer/gablue-apk-installer` en `/usr/bin/gablue-apk-installer` (755) ; sources nettoyées
- Mêmes règles que build-manager : pas de `.pyc` dans l'image, rebuild d'image obligatoire (le lanceur ne lit que `/usr/lib/gablue-apk-installer`)
- Architecture interne → `src/gablue-apk-installer/AGENTS.md`

**build-lgplaunch** (après build-apk-installer) :
- `build.sh` concatène `/src/lgplaunch/lib/*.sh` (ordre de `LIB_FILES`) + le `main` du lanceur → `lgplaunch-standalone.sh` (gitignoré), installé en `/usr/bin/lgplaunch` (755) ; sources nettoyées
- Assemblage = simple concat : **toute modif de `lib/` exige un rebuild d'image** ; ne pas éditer le standalone
- Architecture interne → `src/lgplaunch/AGENTS.md`

**build-share-daemon** (étape `share-build`, avant l'image finale) :
- `dnf5 install cargo rust` **dans l'étape jetée** : la toolchain (~300 Mo) n'entre jamais dans l'image, l'étape finale ne reçoit que `/out/gablue-shared` par `COPY --from=share-build`
- `cargo build --release --locked --manifest-path /build/Cargo.toml` ; `Cargo.lock` est dans le dépôt, donc pas de résolution surprise
- **Même image de base que l'étape finale** (`quay.io/fedora-ostree-desktops/${SOURCE_IMAGE}:${FEDORA_VERSION}`) et non une `fedora:XX` : mêmes blobs partagés et même glibc que le runtime — une image distincte risquait un décalage silencieux de glibc
- **Caches montés** : `/cargo` (`CARGO_HOME`), `/build/target` (sinon chaque build recompile tout `tiny_http`/`serde_json`), plus `/var/cache` et `/var/log`. **Jamais sous `/root`** : dans une image ostree `/root` est un lien vers `var/roothome`, absent de l'image, et le cache mount échoue au build (`stat /root: no such file or directory`, vu en CI le 14/11/2026 — le build rootless local, lui, créait le dossier et masquait le problème)
- Le Containerfile copie **fichier par fichier** (`Cargo.toml`, `Cargo.lock`, `src/`) : `COPY src/gablue-share/daemon` embarquerait `target/` (270 Mo). `.containerignore` à la racine exclut aussi `target`, `__pycache__` et `.git` du contexte envoyé au démon de build
- Mesure (15/11/2026, binaire de CI) : 861 904 octets, RSS 3,2 Mo au repos et 3,7 Mo sous charge (voir `src/gablue-share/AGENTS.md`)

**build-share** (après build-manager, comme les autres paquets Python) :
- Vérifie que `/usr/libexec/gablue-shared` existe (`COPY --from` fait avant le RUN) : sans lui, échec immédiat plutôt qu'une GUI qui prétendrait ne pas trouver son démon
- Installe `/src/gablue-share/gablue_share/` dans `/usr/lib/gablue-share/` + le lanceur `src/gablue-share/gablue-share` en `/usr/bin/gablue-share` (755) ; sources nettoyées
- Mêmes règles que build-manager : pas de `.pyc` dans l'image, rebuild obligatoire (le lanceur ne lit que `/usr/lib/gablue-share`)
- Le Containerfile ne copie que le paquet, le lanceur et `AGENTS.md` (jamais `daemon/target/`)
- Contrat complet → `src/gablue-share/AGENTS.md`

### post-install

- Permissions +x, setcap gamescope, modules SELinux (.te → .pp), binaires externes (`zxtune123` seul — décodeur de GTracker, ne pas le ré-exclure du tarball ; `zxtune-qt` et son .desktop/icône sont exclus —), branding os-release, config système (tuned, bluetooth, pipewire, timers), **ouverture firewalld du partage local** (`firewall-offline-cmd --zone=FedoraWorkstation --add-service=gablue-share`, gardé par l'existence du binaire et du fichier service ; `firewall-cmd` n'a pas de démon dans le conteneur). Effet de bord assumé : la zone est recopiée en entier dans `/etc/firewalld/zones/`, qui prime ensuite sur celle du paquet — une évolution upstream de la zone par défaut ne s'appliquera donc plus (vérifié en conteneur `fedora:45` le 14/11/2026 : XML accepté, service ajouté), désactivation dépôts, nettoyage .desktop, config DX (iptables, NetworkManager), MIME par défaut (Windows.desktop, LGP.desktop), chmod konsole-run (wrapper servicemenu Konsole → files/system/AGENTS.md § konsole-run), chmod des helpers et scripts utilisateur livrés 644 (`gablue-pad`, `gablue-clean`, `gablue-btrfs-compress`, `gablue-nvme-thermal`, `gablue-grub`, `gablue-bootnext`, `gablue-secureboot`, `gablue-ryzenadj`), et **repose des overrides `/usr` écrasés par les dnf** (servicemenu `konsolerun.desktop`, règle udev ThinkPad : leurs sources vivent en `/usr/share/ublue-os/gablue/{kio,udev}/` et sont installées ici, en dernier — les poser au `COPY files/system/all` ne tient pas, `files/scripts/rpm` fait un `dnf5 upgrade` après ce COPY et rpm réécrit les fichiers de ses paquets ; vécu 15/11/2026, konsole 26.08.3 et ublue-os-udev-rules 0.10)
- **Maj auto** : `AutomaticUpdatePolicy=stage` dans `/etc/rpm-ostreed.conf` (copié depuis ublue-os-update-services) + timers flatpak/rpm-ostree samedi 04:00 (`RandomizedDelaySec=10m`, posé **dans `[Timer]`** : écrit après `[Install]`, systemd jette la clé en silence — `RandomizedDelayUSec=0` constaté le 07/11/2026). **Livrés désactivés** par `files/scripts/systemd` (`systemctl disable`, `--global disable` pour `flatpak-user-update.timer`) : les mises à jour automatiques sont un opt-in depuis Gablue Manager (helper `gablue-auto-update`, domaines et rythme)
- **Brew, horaires** : `brew-update.timer` samedi **03:30**, `brew-upgrade.timer` samedi **04:00** (`RandomizedDelaySec=5m`), plus drop-in `/etc/systemd/system/brew-upgrade.service.d/10-after-update.conf` (`After=brew-update.service`). Les deux tiraient à 04:00:00 : le `brew update` concurrent réécrivait `vendor/bundle` (bindata 3.0.0 → 3.0.1) pendant l'installation de `libnghttp2` → `cannot load such file …/bindata-3.0.0/lib/bindata.rb`, unité en échec et 5 paquets en retard (vécu 07/11/2026, sucina). Le décalage ne suffit pas si l'update déborde, d'où l'ordre explicite ; les déclencheurs hérités (`OnBootSec`/`OnUnitInactiveSec`) sont supprimés avant réécriture car `OnCalendar` est une **liste** (sinon deux calendriers cumulés)
- **Linuxbrew** : `/home/linuxbrew/.linuxbrew/bin` au secure_path sudo
- `toggle-updates` upstream (RPM ublue-os-just) intact (flatpak + rpm-ostree) — variante globale avec timers brew = `toggle-updates-all` (60-custom.just) ; le dialogue « Mises à jour automatiques » de Gablue Manager bascule le même périmètre que `toggle-updates-all`, domaine par domaine et avec une fréquence propre à chacun (helper `gablue-auto-update`, brew compris)
- **fstrim** (Bazzite `8a76282f`, silverblue#689) : drop-in remplace ExecStart par `fstrim --listed-in /proc/self/mountinfo` — composefs : `/etc/fstab` ne reflète pas les montages réels. **Divergence volontaire** : `ExecStart=` vide avant la nouvelle ligne (Type=oneshot — sans reset, le drop-in ajoute une 2e exécution au lieu de remplacer ; omission dans le drop-in Bazzite)

**Correction composefs** (toutes variantes) :
- LD_PRELOAD `gablue-composefs-fix.so` (~12 Ko, `src/composefs-fix/`) intercepte statfs/statfs64 **et statvfs/statvfs64** : overlay composefs `/` rapporte 0 blocs libres → redirection `/`, `/home`, `/home/*` → `/var/home` (btrfs)
- `statvfs` n'expose pas `f_type` : le hook interroge `statfs` sur le chemin demandé pour ne rediriger que l'overlay (un btrfs réellement plein n'est pas déguisé). Indispensable depuis Dolphin 26.08 — la barre d'espace libre du status bar passe par `KIO::fileSystemFreeSpace` → `FileProtocol::fileSystemFreeSpace` (`kio_file.so`) → `statvfs`, plus par `QStorageInfo` (`statfs`) ; la barre affichait « 0 o libre » pourtant hook chargé (constaté 15/11/2026, fc45/dolphin 26.08.3, vérifié en A/B : `statvfs('/')` = 0 bloc avant, 105 Gio après). Le statvfs porte sur le **point de montage** résolu via `/proc/mounts` (`/home/gab/…` → `/` sur l'overlay, `/home` n'étant qu'un lien vers `/var/home`), d'où la redirection du seul `/`
- Injection : sed `.desktop` Dolphin (post-install) + drop-in user `plasma-dolphin.service.d` (activation D-Bus `org.freedesktop.FileManager1` démarre `dolphin --daemon` hors .desktop) + réinjection dans `gablue-isomount` (execlp direct) et dans Gablue Manager (`util.dolphin_command` → `env LD_PRELOAD=… dolphin`, fichiers SSH et dossier `.stversions`)
- Enfants (kioworker) héritent de l'env → un Dolphin lancé sans le hook propage le bug à toute la session (jusqu'au reboot). Non couvert (rare, accepté) : lancement manuel terminal

**plasmalogin settle udev** (TEMPORAIRE — à supprimer quand Fedora/KDE corrige) :
- `udevadm settle --timeout=10` du plasmalogin.service = écran noir 10 s sur certaines cartes mères (queue udev jamais vide) → drop-in `90-gablue-settle.conf` → `/usr/libexec/gablue-wait-devices`
- Script : attend un connecteur `/sys/class/drm/card*-*/status` = connected (le 1er suffit, écrans suivants hotpluggés) + `/dev/input/event*`, puis 1 s fixe (docks/HID lents), timeout 5 s (headless = sortie au timeout). Attendre la carte seule ne suffit pas : GPU récents (ex. RX 9060 XT/gfx12) → KWin greeter sans output → écran noir
- ~10,2 s → ~1,1 s (fixe upstream KDE `a8c752fe` trop conservateur)

### systemd

- **Activés (toutes variantes)** : cec-poweroff-tv, cec-active-source, dmemcg-booster, bpftune (Bazzite `4333b30`), brew-setup (voir section rpm), gablue-pad-gate (système, gate BPF) et gablue-pad (`--global` : unité **utilisateur** de session ; opt-out par masque et trois options depuis Gablue Manager)
- **Désactivés — mises à jour automatiques** : rpm-ostreed-automatic, flatpak-system-update, brew-update, brew-upgrade (`systemctl disable`) et flatpak-user-update (`systemctl --global disable`) — opt-in depuis Gablue Manager, voir § rpm
- **Désactivés** : scx_loader, tailscaled, displaylink — via garde d'existence `systemctl list-unit-files` (pattern Bazzite, hook titanoboa) : disable sous `set -e` sur une unité absente casse le build (10/2026 : la dep faible qui tirait displaylink depuis fedora-multimedia a sauté → les 5 variantes en échec ; displaylink est désormais installé explicitement depuis akmods-extra, voir § kernel) ; la garde reste en défense contre toute future dérive de la base image ; si l'unité est présente, elle est re-désactivée
- **Masqués** : systemd-remount-fs, flatpak-add-fedora-repos (le service natif flatpak réajoute fedora/fedora-testing tant que `/var/lib/flatpak/.fedora-initialized` n'existe pas, annulant le kickstart — on garde Flathub seul via `/etc/flatpak/remotes.d/`)
- **Conditionnels (DX)** : ublue-os-libvirt-workarounds, gablue-dx-groups, incus-workaround

### initramfs

- Détection version kernel via dnf5 repoquery ; génération dracut avec options ostree et fido2 ; permissions 0600

### cleanup / finalize

**cleanup** (après chaque RUN) :
- Suppression `/tmp/*`, `/var/log/dnf5.log`, `/boot/*`
- **Relink du rpmdb** (Bazzite `9085704b`) : `ln -f` (hardlink, jamais symlink) de `/usr/share/rpm/rpmdb.sqlite{,-shm,-wal}` vers `/usr/lib/sysimage/rpm-ostree-base-db/` après chaque transaction rpm. Sans lui l'image porte deux bases (168 Mo + 100 Mo mesurés fc44) et réinstaller un paquet retiré de la base (firefox, htop, plasma-welcome*) diff contre une base périmée (coreos/rpm-ostree#4554) ; contrôle post-build : `stat -c %h` = 2
- PAS de `dnf5 clean all` (le cache mount `/var/cache` n'entre pas dans l'image, le nettoyer détruirait le cache persistant)
- PAS de `ostree container commit` (inutile avec bootc/rechunk)

**finalize** (une seule fois, à la fin) :
- `dnf5 config-manager setopt keepcache=0`
- Nettoyage `/var/*` sauf cache
- Migration users/groups → `/usr/lib/passwd` + `/usr/lib/group` via `relocate_accounts` (Bazzite `5b1a399`) : nettoie aussi `/etc/shadow` + `/etc/gshadow`, build en échec si entrée non persistée
- Nettoyage fichiers de verrou + `/usr/etc` ; PAS de `ostree container commit` (le rechunk du workflow s'en occupe)
- **Garde-fou des overrides `/usr`** : `cmp` de chaque override reposé par `post-install` (source `/usr/share/ublue-os/gablue/{kio,udev}/…` ↔ cible `/usr/share/kio/servicemenus/…`, `/usr/lib/udev/rules.d/…`) → build en échec si un paquet a repris le dessus. Ajouter la paire à la liste à chaque nouvel override posé hors `COPY`

### cpuid-fault — Module kernel CPUID faulting

Compile `cpuid_fault_emulation` (`src/cpuid-fault/`) : émule le CPUID faulting sur CPU AMD sans support natif (AM4, Steam Deck) — natif sur Intel 4th gen+ et Ryzen 7000+ (inutile).
- `make -C /usr/src/kernels/${KVER} M=/src/cpuid-fault modules`
- AMD SVM uniquement (ignoré sur Intel) ; KVM doit être déchargé avant chargement
- Signature Secure Boot via `/run/secrets/gablue-kmod-key` (secret CI `GABLUE_KMOD_KEY`) ; clé absente (build local) → compilé non signé ; certificat `gablue-kmod.der` dans `/etc/pki/akmods/certs/`
- **Sources supprimées entières après installation** (`rm -rf /src/cpuid-fault`) : le `.ko` installé sous `/lib/modules/${KVER}/extra/` est autonome (`depends:` vide, pas de dkms) et aucun fichier de l'image ne cite `/src` — vérifié le 15/11/2026. Nettoyer seulement les `.o` laissait `.cmd`, `inc/`+`src/` et le `.ko` dans l'image publiée (856 K, constaté en inspectant `gablue-main:latest`). `rmdir /src` échoue tant que les composants suivants n'ont pas fini, d'où le `|| true` (chacun retire son dossier)
- Persistance opt-in (`ujust cpuid-emu-on/off`) : `on` écrit `/etc/modprobe.d/gablue-cpuid-emu.conf` (blacklist kvm_amd — bloque l'auto-charge udev, modprobe explicite possible) + `/etc/modules-load.d/` (chargé à chaque boot) ; écrit SEULEMENT après chargement réussi (pas d'échec boot si SVM off dans le BIOS) ; `off` supprime, décharge, recharge kvm_amd
