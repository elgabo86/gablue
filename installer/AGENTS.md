# ISO live et installateur (installer/) — gablue

> Sous-document du AGENTS.md racine. Couvre installer/, local-build/ et le
> fonctionnement du live. Mécanique CI du workflow ISO (triggers, chaînage,
> release) → .github/AGENTS.md.
> Même règle que la racine : à jour avant tout commit touchant installer/ ou
> local-build/.

## Build local d'ISO (local-build/)

```bash
./local-build/build-iso.sh main                    # build
./local-build/build-iso.sh main --run              # build + test QEMU
./local-build/build-iso.sh main --pull             # pull forcé image de base
./local-build/build-iso.sh main --skip-flatpaks    # build rapide sans flatpaks
```

Variantes : `main`, `main-dx`, `nvidia`, `nvidia-open`, `nvidia-open-dx`. ISO dans `local-build/output/` avec `chown` pour les permissions user. `sudo -v` au début (un seul mot de passe) ; bind-mount `installer/titanoboa_build_iso.sh` (patché sans `-all-root`) par-dessus celui de l'image Titanoboa.

## Dossier installer/

```
installer/
├── Containerfile                    # Build payload (FROM image Gablue, bind-mount build.sh, SKIP_FLATPAKS arg)
├── build.sh                         # Assemblage : flatpaks (requis + optionnels + runtimes, skippables via SKIP_FLATPAKS), swap kernel, dracut-live, livesys, Anaconda, pack cache gwine → /extra, pré-initialisation préfixe Wine live
├── iso.yaml                         # Config GRUB (label GABLUE_LIVE, timeout 3s, entrées sans apostrophes, enforcing=0)
├── flatpaks                         # Liste des flatpaks obligatoires (format : ref flatpak)
├── flatpaks-optional                # Liste des flatpaks optionnels (checklist yad)
├── titanoboa_hook_preinitramfs.sh   # Swap kernel OGC → vanilla Fedora (Secure Boot)
├── titanoboa_hook_postrootfs.sh     # Anaconda + kickstart bootc + live tweaks (Xvfb, gparted, etc.)
├── titanoboa_build_iso.sh           # Patch Titanoboa : retire -all-root de mksquashfs (préserve les UID)
├── extra/                           # Contenu local arbitraire copié dans /extra du live (gitignore sauf .gitkeep)
└── system_files/shared/             # Config Anaconda (pre-scripts + post-scripts), autostart, localisation live (fr_CH)
```

## Fonctionnement du live

1. **Swap kernel** : OGC (non signé) remplacé par le kernel vanilla Fedora (signé) pour Secure Boot
2. **Flatpaks** :
   - Listes `installer/flatpaks` (8 requis) + `flatpaks-optional` (25 optionnels) = flatpaks pré-téléchargés dans l'ISO
   - **MangoHud** : runtime obligatoire, version freedesktop détectée dynamiquement (`flatpak remote-ls flathub --runtime | awk -F'\t'`), installé dans le live + liste requise post-install
   - **OBS VkCapture** : version détectée dynamiquement **indépendamment de MangoHud** (Flathub peut publier MangoHud sur une branche plus récente — ex. 26.08 vs 25.08 → `No remote refs found` en réutilisant la branche) ; ne suit PAS OBS Studio — OBS décoché → OBS VkCapture désinstallé aussi
   - **Proton-GE** (branche `stable`) : **suit Steam** — Steam décoché → désinstallé
   - Variantes NVIDIA : runtimes `org.freedesktop.Platform.GL[32].nvidia-XXX` ajoutés aux obligatoires (version depuis `rpm -q nvidia-driver`)
   - **Questions interactives regroupées en `%pre-install`** (`pre-scripts/gablue-questions.ks`, `%include` avant `ostreecontainer`) : interactions yad posées après formatage, avant déploiement — plus d'interruption ensuite. `%pre-install` et `%post --nochroot` tournent dans l'env installateur → `/tmp` partagé (choix écrits puis lus). Trois questions :
     1. **Compression BTRFS zstd** (oui par défaut) : appliquée immédiatement via `btrfs property set <subvol> compression zstd` sur `/mnt/sysroot*` — avec composefs, le `compress=zstd` du fstab Anaconda est **sans effet** (racine = overlay, pas un montage btrfs direct) ; propriété héritée par tous les nouveaux fichiers → ostree, /var, flatpaks compressés dès l'écriture (zstd:3, comme `ujust btrfs-compress`)
     2. **Sélection flatpaks optionnels** (checklist, tout décoché par défaut) → `/tmp/gablue-selected-flatpaks`
     3. **Cache gwine** (oui par défaut) → `/tmp/gablue-install-gwine-cache`
     - yad via `run0 --user=liveuser env XDG_RUNTIME_DIR=... yad --on-top --center --skip-taskbar` (sinon le dialogue s'ouvre derrière Anaconda plein écran → install semble figée)
   - `install-flatpaks.ks` (`%post --nochroot`) lit `/tmp/gablue-selected-flatpaks` (absent = aucun optionnel) puis :
     1. Copie `/var/lib/flatpak` (live) → déploiement ostree via `rsync -aAXUHKP --open-noatime --filter="-x security.selinux"` — **filtre `-x security.selinux` indispensable** : fichiers du live étiquetés `unlabeled_t`, SELinux enforcing refuse le `lremovexattr`/`lsetxattr` sur btrfs (`Permission denied` → rsync 23 → échec %post → crash Anaconda). Les xattrs `user.ostree*` (critiques) restent copiés ; label SELinux assigné à la création par le kernel + `restorecon` explicite par `restore-flatpak-selinux.ks` (filet, Bazzite `f0bafa6`)
     2. Désinstalle les optionnels non désirés DIRECTEMENT dans la cible ostree (pas dans le live) : installation `gtarget` (`/etc/flatpak/installations.d/gtarget.conf` → `<deployment>/var/lib/flatpak`) + `flatpak --installation=gtarget uninstall`. **Pourquoi pas dans le live** : `/var/lib/flatpak` y est overlayfs (bind RO `var-lib-flatpak.mount`) → `flatpak uninstall` échoue en `Invalid cross-device link` (EXDEV, hardlinks repo/objects ↔ checkouts ne traversent pas les couches overlay) ; cible ostree = btrfs RW → pas d'EXDEV. En root avec `--installation`, flatpak opère directement sur le dépôt sans le helper D-Bus
     3. Itère par ref complète (`awk -F/` sur `flatpak list --columns=ref`) — MangoHud et OBS VkCapture ont plusieurs branches (24.08 + 25.08), uninstall par ID échoue (« Multiple installed refs match »)
     4. Dépendances conditionnelles : Proton-GE si Steam, OBS VkCapture si OBS
     5. Runtimes orphelins : `flatpak --installation=gtarget uninstall --unused`
     - Dépôt Flathub déjà dans `/etc/flatpak/remotes.d/` (posé par build.sh) — pas de `flatpak remote-add`
     - **`restore-flatpak-selinux.ks`** (`%post` chrooté, `%include` juste après) : `setenforce 0` + `restorecon -R /var/lib/flatpak` dans le déploiement ostree (le labeling implicite du kernel couvre le nominal, restorecon garantit la conformité si non joué ; aligné Bazzite `f0bafa6`, partie flatpak seule — Gablue ne touche pas au policy-store). Non bloquant (`|| :`). Cible le `/var/lib/flatpak` du système installé, pas celui RO du live
3. **Compte utilisateur** : aucun pré-rempli — spoke Anaconda visible, l'utilisateur choisit ; Plasma gère la création au 1er boot si spoke sauté
4. **Session live** : bureau Plasma complet via `livesys-scripts` ; Anaconda pas lancé auto (l'utilisateur lance `liveinst`). Flatpaks pré-cachés visibles dans le menu (`XDG_DATA_DIRS` via `/etc/environment.d/99-gablue-flatpak-live.conf`)
5. **plasma-welcome** retiré du live (hook postrootfs — pas de lancement auto au boot)
6. **Dossier Bureau** : livesys-scripts crée `Desktop` (anglais) avec `liveinst.desktop` avant `xdg-user-dirs-update` (empêche le renommage) — reste en anglais
7. **Installation** : kickstart Anaconda `ostreecontainer` (bootc), BTRFS par défaut, compression zstd:1
8. **Secure Boot** : enrollment auto clé MOK Gablue, mot de passe `gablue`
9. **Post-install** : `bootc switch --mutate-in-place` pour activer la signature
10. **Services désactivés dans le live** : flatpak-update, cec-poweroff, dmemcg-booster, tailscaled, brew, greenboot…
11. **NVIDIA live** : fix `GSK_RENDERER=gl`, réinstallation mesa-vulkan-drivers + nvidia-gpu-firmware (kernel vanilla = pas de driver proprio, on utilise nouveau)
12. **Localisation live** : fr_CH.UTF-8 + clavier QWERTZ suisse romand — fichiers dans `system_files/shared/etc/` : `locale.conf` (LANG+LANGUAGE), `vconsole.conf` (KEYMAP=ch-fr), `X11/xorg.conf.d/00-keyboard.conf` (XKB). Copiés dans le payload live SEULEMENT (pas l'image installée). Langpacks depuis l'image Gablue de base. **Anaconda préconfiguré** via kickstart (`titanoboa_hook_postrootfs.sh`) : `lang fr_CH.UTF-8`, `keyboard --vckeymap=ch-fr --xlayouts='ch (fr)'` — écran langue/clavier prérempli (spoke reste modifiable)
13. **GRUB** : noms d'entrées SANS apostrophes (Titanoboa génère `menuentry '...'` sans échapper → parsing cassé, une seule entrée visible)
14. **Dossier `/extra`** (live uniquement → déployé à l'install, JAMAIS dans l'image container — l'install redéploie l'image propre via `ostreecontainer` + `bootc switch`) :
    - **Pack cache gwine** : build.sh télécharge le pack pré-construit du repo `elgabo86/gwine-cache` (release hebdo `latest`, produite en amont par `gwine --download-components` + `--cachepack`) — assets `gwine-cache.tar.xz` (vérifié `.sha256`, min 100 Mo), `install-cache.sh`, `README.txt` → `/extra/gwine-cache-installer/`. Plus de download composant par composant ni re-génération. **Fail-fast** (`exit 1`) : gwine absent (requis pour l'init du préfixe, point 16), download en échec après 3 tentatives (curl `--retry`), checksum invalide, archive < 100 Mo — jamais d'ISO sans le pack. URL surchargeable via `GWINE_CACHE_BUNDLE_URL` (miroir/test). Cache ré-extrait temporairement pour l'init du préfixe puis supprimé de l'ISO finale (seule l'archive `/extra` persiste)
    - **Contenu local** : `installer/extra/` (bind-monté sur `/src/extra`, gitignore sauf `.gitkeep`) copié dans `/extra` pour les builds locaux — fichiers/dossiers arbitraires. Absent/vide en CI → ignoré
15. **Post-script `install-extra.ks`** (`%post --nochroot`, juste après `install-flatpaks.ks`) : lit `/extra` dans le live, déploie chaque item dans le système installé
    - **Résolution du déploiement ostree** : `/mnt/sysimage/etc` et `/usr` ne sont PAS peuplés directement (le système réel vit dans `<deployment>`) → `/mnt/sysimage` non chrootable. Résolution : `deployment=$(ostree rev-parse --repo=/mnt/sysimage/ostree/repo ostree/0/1/0)` puis `DEPLOY_ROOT=/mnt/sysimage/ostree/deploy/default/deploy/${deployment}.0` ; passwd lu dans `${DEPLOY_ROOT}/etc/passwd` (sinon `awk: cannot open file` → ScriptError) ; `chown`/`restorecon` via `chroot "$DEPLOY_ROOT"` (home `/home -> var/home` accessible)
    - Utilisateur : 1er UID ≥ 1000 créé par Anaconda dans `${DEPLOY_ROOT}/etc/passwd`
    - Cache gwine : `~/.cache/gwine` avec `chattr +C` (nodatacow) AVANT extraction (anti-CoW btrfs) ; extraction `gwine-cache.tar.xz`, `chown -R` + `restorecon` non récursif (seul `.cache` labellisé, pas les 2 Go). Le runner gwine N'est PAS extrait (gwine l'installe à la volée depuis le cache en mode offline — évite de dupliquer l'espace disque)
    - Extensible : chaque nouvel item (ex. cores RetroArch) = une section avec ses `chown`/`restorecon`
    - Fallback : aucun utilisateur (spoke sauté) → log + skip sans échec ; `/etc/skel` laissé en option (commenté)
16. **Pré-initialisation du préfixe Wine dans le live** : `build.sh` pré-initialise un préfixe complet pour la session live — préfixe + runner dans `/usr/share/gablue/wine-home/` + `wine-runner/` (squashfs), symlink `~/Windows → /usr/share/gablue/wine-home` (chemins du registre → `/home/liveuser/...`). Init via `xvfb-run` (Xvfb requis pour PhysX/OpenAL — installateurs `.exe` nécessitant `CreateWindow`) ; cache extrait temporairement puis supprimé. `find ... -exec chown 1000:1000 {} +` (pas `chown -R` — race sur les `.tmp` Wine) sur `/usr/share/gablue/` → liveuser (UID 1000) propriétaire au boot ; livesys crée l'utilisateur normalement
    - **Deux protections contre le crash Xvfb sur images NVIDIA** : (1) variables GLVND côté client GL (`__GLX_VENDOR_LIBRARY_NAME=mesa`, `__EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json`, `LIBGL_ALWAYS_SOFTWARE=1`) ; (2) `mv` temporaire de `libglxserver_nvidia.so` + `libnvidia-egl-gbm.so.1` côté serveur X (Xvfb charge son module GLX via le mécanisme Xorg, qui ignore les variables GLVND) — restaurés immédiatement après `xvfb-run`
17. **Patch Titanoboa `-all-root`** : `installer/titanoboa_build_iso.sh` = copie modifiée du `build_iso.sh` Titanoboa, bind-mountée par `local-build/build-iso.sh` ET le workflow CI. Seule différence : `-all-root` retiré de l'appel `mksquashfs` → préserve les UID/GID du payload (UID 1000 pour les fichiers Wine) ; sinon tout le squashfs est root:root et liveuser ne peut pas écrire dans le préfixe. Le CI utilise le même script patché (bind-mount via `-v` dans le `podman run` Titanoboa)
