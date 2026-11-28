# Fichiers système (files/system/) — gablue

> Sous-document du AGENTS.md racine. Couvre files/system/ : configs /etc,
> binfmt, scripts et binaires /usr/bin, widget plasmoid, tuned, recettes ujust.
> Architecture interne : src/gablue-pad/AGENTS.md (overlay manette, déclencheur,
> gate BPF), src/gwine-launcher/AGENTS.md (gwine), src/tvqt/AGENTS.md (tvqt),
> src/gablue-apk-installer/AGENTS.md (apk), src/lgplaunch/AGENTS.md (lgplaunch).
> Même règle que la racine : à jour avant tout commit touchant files/system/
> (les générateurs — ex. post-install — sont dans files/scripts/AGENTS.md).

## Configurations système (/etc)

- **distrobox/distrobox.conf** : Configuration Distrobox
- **firewalld/zones/nm-shared.xml** : Zone firewall partagée
- **xdg/autostart/gablue-manager-tray.desktop** : lance `gablue-manager --tray` à l'ouverture de session — icône de barre d'état seule, la fenêtre n'est chargée qu'au clic. `NoDisplay=true`, `OnlyShowIn=KDE;`, `X-KDE-autostart-after=panel`. Le mode tray relit `~/.config/gablue/preferences.json` et sort si l'utilisateur a coupé l'icône dans Gablue Manager : l'autostart n'est jamais modifié
- **profile.d/customperso.sh** : Alias et personnalisations shell
- **security/limits.d/memlock.conf** : Limites mémoire
- **skel/.config/gtk-4.0/** : Configuration GTK par défaut — `gtk.css` importe `colors.css` et `kde_window_geometry.css`, ce dernier alignant les coins des fenêtres GTK4 sur Plasma (**7px**, Bazzite `e0d6aae2` : en 4px les apps GTK4 tranchaient avec le reste du bureau)
- **sudoers.d/nopasswd** : Sudo sans mot de passe pour `%wheel` — `ryzenadj`, `cpupower`, `efibootmgr --bootnext|--bootorder *`, `bootc upgrade`, `flatpak update --system -y` (arguments **exacts**, aucun joker sur flatpak : ni install, ni remote, ni app id) et les helpers `/usr/libexec/gablue-kargs`, `gablue-cpuid-emu` (`on|off|status`), `gablue-tailscale` (`on --login-server …|off`), `gablue-service` (unités `displaylink.service` et `docker.service` seulement, `on|off`), `gablue-low-latency` (`on|off`), `gablue-broadcom-wl` (`on|off`), `gablue-wifi-powersave` (`on|off|status`), `gablue-rebase` (les cinq variantes publiées seulement), `gablue-auto-update` (`status`, `image on|off`, `flatpak on|off`, `brew on|off`, `schedule daily|weekly` — neuf formes exactes, aucun joker), `gablue-update` (mise à jour bootc avec progression JSONL, **aucun argument**) `gablue-avahi` (`on|off|status`, annonce `_ssh._tcp`), `gablue-clean` (`flatpak`), `gablue-btrfs-compress` (`zstd|none <montage>` — le helper revérifie que le chemin est un montage Btrfs de `findmnt`) `gablue-nvme-thermal` (`status` ; `set <dev> <t1> <t2>` ; `off <dev>` — bornes revérifiées côté root) `gablue-grub` (`status` ; `set 0|1|2` ; `unset` — `menu_auto_hide` dans grubenv, `/boot` remonté en écriture seulement le temps de l'écriture) `gablue-bootnext` (`list` ; `boot <id>` — `id` 4 hexadécimaux ou `esp:/dev/sdX1`, revérifié côté root ; disque du système principal refusé ; entrée NVRAM créée pour une ESP inconnue puis retirée du BootOrder ; `BootNext` une seule fois puis redémarrage) `gablue-secureboot` (`status` ; `enroll` — demande MOK des deux certificats Gablue, mot de passe public `gablue`) et `gablue-ryzenadj` (`status` ; `set <45-110>` ; `off` — limite Tctl dans `/etc/gablue/ryzenadj.conf`, bornes revérifiées côté root, réappliquée au démarrage par `gablue-ryzenadj.service`) `gablue-login` (`status` ; `pad on|off` ; `dark on|off` — réglages de l'écran de connexion, relus par le greeter à son démarrage) — plus trois commandes systemd aux arguments exacts, `enable|disable sshd.service --now` (pastille « Serveur SSH » de Gablue Manager), `reboot|poweroff` (actions distantes, équivalentes au polkit qu'un membre de wheel peut déjà déclencher en session locale) et `reboot --firmware-setup` (menu « Redémarrer dans le BIOS ») — jamais `rpm-ostree kargs` ni une référence d'image libre en direct, un kargs libre en NOPASSWD permettrait `init=/bin/sh` sur le prochain démarrage et un rebase libre permettrait de démarrer n'importe quelle image en root
- **systemd/** : Timeouts et configuration systemd
- **yum.repos.d/docker-ce.repo** : Dépôt Docker

## tmpfiles.d (/usr/lib/tmpfiles.d)

- **btrfs-reclaim.conf** : `w /sys/fs/btrfs/*/allocation/data/{dynamic,periodic}_reclaim - - - - 1` (Bazzite `06732aa5`, 11/2026) — sans réclamation dynamique, btrfs peut refuser une écriture avec `ENOSPC` alors qu'il reste de l'espace libre. Posé en `tmpfiles.d` et non par une unité : systemd-tmpfiles l'applique à chaque démarrage, quel que soit l'ordre de montage. À retirer quand le kernel active le reclaim par défaut

## Configuration d'installation bootc (/usr/lib/bootc/install)

- **usr/lib/bootc/install/20-gablue.toml** : `root-fs-type = "btrfs"` + `enforce-container-sigpolicy = true` — lus par `bootc install` (Anaconda via `ostreecontainer`, VMs de test, install depuis l'ISO) ; sans `enforce-container-sigpolicy`, le système installé ne vérifie plus la signature de l'image aux mises à jour (les images Gablue sont signées cosign). Même fix upstream Bazzite (#6084, `20-bazzite.toml`) ; la base Kinoite n'a aucun fichier dans ce dossier (créé par ce COPY)

## Exécution des binaires Windows (binfmt_misc)

- **usr/lib/binfmt.d/gablue-windows.conf** : règle binfmt_misc (magic `MZ` → `/usr/bin/gwine`), chargée au boot par `systemd-binfmt.service` (statique, présence du fichier suffit)
- Permet `./jeu.exe` en terminal et le bouton « Exécuter » de Dolphin/KIO pour les `.exe` avec bit `+x` (copies NTFS/exFAT, archives)
- Complément du patch runner `no_exe_executable_bit.mypatch` (dépôt `elgabo86/gwine`) : le wineserver gwine ne pose plus `+x` sur les `.exe`/`.com` créés par les installateurs → KIO les ouvre nativement via Windows.desktop → gwine (icônes + « Ouvrir avec » intacts). KIO ne traite un `.exe` comme binaire natif QUE s'il a `+x` (hardcodé, sans option)

## Session graphique console (/usr/share/wayland-sessions)

- **gablue-gaming.desktop** : la session « Gablue Gaming », choisie dans
  plasmalogin. `Exec` = le même enrobage que Plasma
  (`plasma-dbus-run-session-if-needed`) puis
  `/usr/libexec/gablue-gaming-session`. **`DesktopNames=GablueGaming;KDE`** :
  le premier nom devient `XDG_SESSION_DESKTOP` (c'est ce que lit
  `glauncher::mode` pour savoir qu'il est en console) et la liste complète
  devient `XDG_CURRENT_DESKTOP`, où **« KDE » doit rester** — c'est lui qui
  fait choisir le backend KDE à `xdg-desktop-portal`, donc sans lui plus de
  partage d'écran dans les jeux. `desktop-file-validate` râle sur
  `DesktopNames` (« clé non standard ») : c'est une clé SDDM, Plasma utilise
  la même, l'avertissement est attendu.
- **gablue-gaming-session** : KWin **nu** (`--drm --xwayland`), sans
  plasmashell — ni panneaux, ni notifications, ni Discover. Garantit
  lui-même les trois variables `XDG_*` (utile lancé depuis un VT, sans
  greeter), démarre `plasma-powerdevil.service` (sans lui, logind applique son
  défaut `HandlePowerKey=poweroff` : le bouton power ÉTEINT la machine au lieu
  de la mettre en veille) **et charge l'effet KWin `hidecursor`** à chaud
  (`loadEffect` par D-Bus, jamais d'écriture dans `kwinrc`) pour que le
  pointeur disparaisse au repos, y compris par-dessus un jeu — c'est le
  compositeur qui tient le curseur, pas le launcher (qui masque le sien depuis
  `src/cursor.rs`). **La veille est désactivée dans tous les cas** (23/11/2026)
  par deux inhibiteurs tenus tant que la session vit : logind
  `idle:handle-lid-switch` (aucune action d'inactivité, rien à la fermeture du
  capot) et `org.freedesktop.ScreenSaver` via `gpad-inhibit` en mode
  `GPAD_INHIBIT_TOUJOURS=1` (ni extinction d'écran, ni verrouillage, ni veille
  automatique côté PowerDevil). On n'inhibe **pas** `sleep` : l'entrée « MISE
  EN VEILLE » du menu Home (`systemctl suspend`) doit rester possible. Il démarre aussi **`gablue-pad.service`** — l'overlay
  manette est `WantedBy=graphical-session.target`, cible que Plasma démarre et
  qu'une session KWin nue ne démarre PAS : sans ça, aucune manette, donc aucun
  menu Home et **aucune sortie propre** de la session. Deux précautions : KWin
  reçoit un **socket au nom fixe** (`--socket gablue-gaming`) et
  l'environnement (`WAYLAND_DISPLAY`, `XDG_*`) est copié dans le gestionnaire
  utilisateur par `systemctl --user set-environment`, sinon l'unité se
  connecterait à un compositeur inexistant (elle n'hérite pas du shell). Le
  démarrage de l'unité attend le socket (30 s au plus, en fond). Il `exec`
  ensuite KWin avec `--exit-with-session /usr/libexec/gablue-gaming-launcher`.
  Jamais `set -e` : un service absent n'est pas une panne de session.
- ~~**gablue-gaming-removable**~~ : **retiré le 24/11/2026**. Ce helper lancé
  par `gablue-gaming-launcher` montait les disques en écriture (contre la
  décision du 23/11 : lecture seule) et, surtout, restait dans le scope de la
  session console après la sortie du launcher : la session ne finissait jamais
  de se fermer (`closing`), le gestionnaire utilisateur survivait avec son
  `WAYLAND_DISPLAY=gablue-gaming`, et l'overlay du bureau pointait sur un
  socket mort. `gablue-disk-automount.service` (unité utilisateur) couvre le
  besoin, en lecture seule.
- **gablue-display** : préférence d'**écran du jeu**, appliquée au démarrage de
  la session console (par `gablue-gaming-launcher`, qui tourne dans KWin donc
  a un compositeur à qui parler). `plasma` (défaut) ne touche à rien : KWin
  applique la disposition enregistrée par Plasma, étendue ou clonée ;
  `clone` réplique l'écran principal sur les autres (`output.<autre>.mirror.<principal>`),
  `primary` éteint les autres (`disable`). Une **seule** invocation atomique de
  `kscreen-doctor` (deux appels laisseraient un état intermédiaire, écran noir
  compris). Principal = priorité la plus basse (1 = principal, 0 = non
  renseignée), sinon premier allumé. Préférence dans
  `~/.config/gablue/gaming.conf` (`display=`), réglée depuis la page Réglages
  du launcher ; `--dry-run` affiche la commande sans rien appliquer.
- **gablue-gaming-launcher** : boucle du launcher. Contrat de sortie : code
  **0 = fin voulue** (KWin s'arrête, retour au greeter), **≠ 0 = plantage** →
  relance après 1 s. En session console le launcher n'expose aucune sortie
  (retour/Échap inertes) : la sortie passe par le menu système de `gpad`
  (Home maintenu). Il honore aussi **une** entrée d'autostart XDG — celle de
  Syncthing — parce qu'une session sans plasmashell ne lance aucun autostart
  et que la synchronisation de dossiers n'a rien à voir avec l'interface : il
  relit `Exec=` telle quelle (une entrée `flatpak run`, cas de SyncThingy,
  démarre donc l'application flatpak sans code spécial), respecte
  `Hidden`/`X-GNOME-Autostart-enabled`/`OnlyShowIn`, retire les codes de champ
  et ne double pas une instance déjà lancée (`flatpak ps`). Il applique aussi
  la préférence d'écran du jeu (`gablue-display`). C'est le bon endroit et non
  `gablue-gaming-session` : ce script tourne **dans** KWin, donc
  `WAYLAND_DISPLAY`, le bus utilisateur et le compositeur existent déjà.
- **gablue-session-mode** : session graphique, **deux niveaux** (27/11/2026).
  La **session par défaut** vit dans `/etc/gablue/session.conf`
  (`[Session] Default=console|bureau|greeter`, `User=`) et ne se règle que par
  `defaut` — c'est ce qu'écrit le dialogue « Session graphique » de Gablue
  Manager. Une **bascule** (`console`, `bureau`, `greeter`, demandée par le
  menu Home de `gpad` ou par `FERMER LA SESSION`) est **ponctuelle** : elle
  change la session maintenant et ne touche pas au défaut ;
  `gablue-session-reset.service` réécrit l'autologin effectif depuis le défaut
  au démarrage suivant (avant plasmalogin). Sans ça, un « mode jeu » d'une
  soirée restait le démarrage de tous les suivants. L'ancien état est **adopté**
  au premier démarrage de cette image (le défaut est repris de l'autologin en
  place), donc personne ne change de session par surprise. `status` (lisible,
  sans privilège), `etat` (`session=`, `autologin=`, `defaut=`,
  `autologin_defaut=` — sortie stable du dialogue, les deux premiers décrivent
  le **prochain** démarrage et peuvent donc montrer une bascule en cours, les
  deux derniers le réglage persistant), `defaut` sans argument (le défaut seul),
  `reset` (root, démarrage). Root via pkexec, autorisé sans mot de passe par
  `60-gablue-session-mode.rules` (groupe wheel, session locale, programme
  exact). Le fichier lu par plasmalogin est
  `/etc/plasmalogin.conf.d/zz-gablue-session.conf` ; le défaut vit **hors** de
  ce dossier et porte `Default=` — deux fichiers `[Autologin]` y seraient lus
  tous les deux. **Aucun autre autologin ne doit exister** —
  `plasma-setup.service` (assistant de premier démarrage de KDE) écrit
  `User=plasma-setup` dans `99-plasma-setup.conf` : en mode « écran de
  connexion » le `zz-` est donc écrit avec `User=` **vide mais présent**, qui
  écrase cette clé. Le retirer (ancien comportement) laissait le wizard
  s'autologiner à la place du greeter (constaté le 25/11/2026). Le paquet et le
  wizard sont **conservés** (premier démarrage d'une installation) : c'est
  `gablue-session-mode` qui impose l'écran de connexion, pas un drapeau posé au
  build (voir `files/scripts/AGENTS.md`). Trois précautions,
  toutes mesurées le 23/11/2026 :
  (1) **redémarrer plasmalogin** plutôt que se déconnecter — il lit
  `[Autologin] Session=` à son démarrage et le garde en mémoire (bug KDE
  522181), donc une déconnexion relance la session précédente ;
  (1bis) **arrêter plasmalogin AVANT que la session ne se termine**, le
  relancer à la fin — jamais `restart` : le plasmalogin vivant réagit à la fin
  de session. Avec `Relogin=true` (valeur d'origine du fichier) il relançait
  aussitôt la session qu'il avait EN MÉMOIRE (bug KDE 522181) : le bureau
  Plasma réapparaissait une seconde avant la session jeu (mesuré le
  25/11/2026 — session créée avec `startplasma-wayland` juste après la fin de
  la précédente, puis tuée par la bascule). Avec `Relogin=false`, il ouvre
  l'**écran de connexion**, dont le serveur d'affichage ne peut pas reprendre
  `/dev/tty1` (sddm#1844) : il sort en 23, systemd le relance en rafale et il
  meurt sur `start-limit-hit`, la bascule finissant **sans aucune session**
  (mesuré le même jour, 14:33). D'où l'ordre du script : `stop` en tête,
  sessions arrêtées, fichier d'autologin écrit, puis `reset-failed` + `start`
  (le compteur de démarrages d'une unité déjà en échec ferait sinon refuser le
  démarrage). `Relogin=false` reste obligatoire : c'est lui qui empêche le
  rejeu de la session mémorisée.
  (2) **arrêter la session sortante avant** — `plasma-kwin_wayland.service` et
  `plasma-plasmashell.service` vivent dans le gestionnaire UTILISATEUR, pas
  dans le scope de session : ils survivent, gardent `org.kde.KWin`, le siège
  DRM et le grab de la manette, et la session suivante se fige (image figée,
  overlay accroché à un compositeur mort, puis session Plasma sans compositeur) ;
  KWin ignore SIGTERM, d'où le SIGKILL explicite ;
  (3) **se détacher** dans une unité transitoire (`systemd-run --collect`) :
  la bascule tue la session qui l'a demandée, un script resté dedans
  s'arrêterait au milieu du chemin ;
  (4) **l'utilisateur demandeur** vient de `PKEXEC_UID` **ou** `SUDO_UID`,
  jamais de `id -u` : lancé par sudo, le script prenait root et écrivait un
  autologin pour root — deux sessions graphiques sur le même siège, et root
  connecté à la place de l'utilisateur ;
  (5) **la sortie se juge sur les processus et leur cgroup**, pas sur l'état de
  l'unité : `MainPID` ne désigne que `kwin_wayland_wrapper` (le compositeur est
  son enfant), `cgroup.procs` est un pseudo-fichier de taille 0 (donc `-s` est
  toujours faux) et « deactivating » n'est pas « arrêté ». Et une session
  laissée en « closing » par `loginctl terminate-session` part au
  `loginctl kill-session -s SIGKILL`.
  Piège transverse, celui qui a rendu la bascule lente puis silencieusement
  inopérante : **aucun préfixe arithmétique dans un chemin** —
  `UID_NUM=10#$(id -u …)` donne la chaîne `10#1000`, donc `/run/user/10#1000`,
  et toutes les commandes `systemctl --user` échouent sans un mot.
  Et **le fichier doit être exécutable** : livré en 644 (comme tous les helpers
  de `files/system/`), il n'était pas dans la liste `chmod +x` de
  `post-install` — `pkexec` répondait `Error executing … Permission denied`
  dans le journal de la session et `MODE JEU` / `MODE BUREAU` **ne faisait
  rien**, en silence (constaté le 25/11/2026, deux appuis tracés dans
  `journalctl`) : tout helper lancé par `pkexec` ou directement doit figurer
  dans cette liste.
  (6) **l'autologin est rattrapé par `gablue-login-guard`** : plasmalogin
  démarre son écran de connexion, puis demande sa fin pour ouvrir la session
  autologin — le serveur d'affichage du greeter (`kwin_wayland`, compte
  `plasmalogin`) ne sort pas quand sa session logind se termine et garde
  `/dev/tty1` : `plasmalogin-helper` boucle alors sur « Failed to take control
  of "/dev/tty1" ("root"): Operation not permitted », écran noir, **aucune
  session** (constaté le 25/11/2026, ~25 tentatives/s, après bascule comme au
  démarrage). C'est le bug amont **sddm#1844** (PR sddm#1969 « if autologin is
  used, avoid starting a display server for the greeter »), non repris par le
  fork : le helper démarre donc le garde en fin de bascule, et le garde achève
  le greeter resté en vie avant de laisser le démon réessayer.
  `FERMER LA SESSION` de `gpad` l'appelle en mode `greeter` **quand un
  autologin est armé**, sinon il garde la déconnexion polie de Plasma.
- **Pas encore là** (lots suivants) : la session « choix » avec compte à
  rebours (autologin retardé sans passer par le greeter), les réglages
  `session`/`autologin`/`delay` de `/etc/gablue/login.conf` et la barre de mise
  à jour du launcher.

## Unités systemd utilisateur (/usr/lib/systemd/user)

Drop-ins dans `*.service.d/`, appliqués à l'échelle du système (priorité plus faible que `~/.config/systemd/user/`, donc un drop-in utilisateur de même nom écrase le nôtre — laisser le même nom de fichier pour que le retrait côté utilisateur soit transparent).

- **plasma-dolphin.service.d/gablue-composefs-fix.conf** : réinjecte `LD_PRELOAD` du composefs-fix quand Dolphin est lancé par activation D-Bus (voir § composefs de `files/scripts/AGENTS.md`)
- **gablue-share.service** : démon de partage en **mode permanent** (opt-in, livré désactivé ; réglé depuis Gablue Share ou gablue-manager). Démarre avec la session : pour un poste sans session graphique, `loginctl enable-linger` serait nécessaire — hors périmètre. Contrat → `src/gablue-share/AGENTS.md`
- **gablue-pad-gate.service** (système) : démon du **gate BPF** de l'overlay manette, lancé en root au boot (`/usr/libexec/gablue-pad/gpad-gate --socket /run/gablue-pad/gate.sock --timeout 0`, `RuntimeDirectory=gablue-pad`). Le programme BPF est chargé une fois et **attaché seulement entre `open` et `close`** : hors menu, aucun hook dans le noyau. Les cibles (`target`/`clear`) sont posées par l'utilisateur à chaque ouverture, donc un démon qui vit depuis le boot ne garde aucun `dev_t` périmé après une reconnexion. Contrat → `src/gablue-pad/gate/AGENTS.md`
- **gablue-pad.service** : overlay manette de la session (`ExecStart=/usr/bin/gpad-trigger /usr/bin/gpad`, `Restart=always`/`RestartSec=1`, `After=` + `PartOf=graphical-session.target`, `StartLimitIntervalSec=0`) ; l'unité exporte tous les chemins (`RECORDER`, `SCRIPTS`, `KWIN_SCRIPTS`, `GPAD_GATE`…), le banc les déduit du binaire. **Activée pour tous les comptes** (`systemctl --global enable` dans `files/scripts/systemd`) ; `gablue-pad off` la neutralise par **masque utilisateur** et les trois options (`menu`, `shortcuts`, `inhibit`) se coupent séparément, à chaud (SIGHUP). **Au login Plasma**, `etc/xdg/autostart/gablue-pad.desktop` →
  `/usr/libexec/gablue-pad/session-start.sh` la **redémarre** (attente du socket Wayland importé dans le gestionnaire, puis `restart` — jamais `start` : revenue de la session console, elle est déjà active avec l'ancien `WAYLAND_DISPLAY`). Contrat → `src/gablue-pad/AGENTS.md`
- **gablue-login-guard.service** (système, oneshot, `After=plasmalogin.service`,
  `WantedBy=graphical.target`, `TimeoutStartSec=240`) : `/usr/libexec/gablue-login-guard`
  au démarrage **et** depuis `gablue-session-mode` après chaque redémarrage du
  gestionnaire de connexion. Sans autologin armé il ne fait **rien** ; avec, il
  attend la session du compte visé et, si elle ne vient pas, achève l'écran de
  connexion resté en vie (**SIGKILL du seul compte `plasmalogin`**, jamais
  celui de l'utilisateur) puis laisse plasmalogin réessayer, avec un
  redémarrage du démon en dernier recours. Raison et vécu : bug amont
  **sddm#1844**, bullets `gablue-session-mode` ci-dessus.
- **gablue-session-reset.service** (système, oneshot, `DefaultDependencies=no`,
  `After=local-fs.target`, `Before=plasmalogin.service`,
  `WantedBy=multi-user.target`) : `gablue-session-mode reset` — réécrit
  l'autologin effectif depuis `/etc/gablue/session.conf` **avant** que
  plasmalogin ne le lise, donc une bascule (`MODE JEU` / `MODE BUREAU`,
  `FERMER LA SESSION`) ne survit pas au redémarrage. Ne bascule rien, ne
  redémarre rien. `DefaultDependencies=no` sert uniquement à passer avant
  plasmalogin, qui n'est pas un service d'early boot.
- **gablue-pad-wake.service** (système, oneshot) : déclare les manettes
  Bluetooth appariées comme sources de réveil (`bluetoothctl wake <mac> on`),
  filtrées sur `Icon: input-gaming` — un téléphone ou un casque apparié ne doit
  pas sortir la machine de veille. Jouée au boot **et** relancée par la règle
  udev à chaque manette qui se connecte (`ENV{ID_INPUT_JOYSTICK}=="1"`, donc
  toutes les manettes : DualShock/DualSense, Xbox, Switch Pro), ce qui couvre
  un appairage fait en session. BlueZ garde `WakeAllowed=true` dans l'info de
  l'appareil, donc le réglage survit au redémarrage mais pas à un nouvel
  appairage, d'où les deux points de passage.
- **Réveil par Home** (`/usr/lib/udev/rules.d/99-bt-wake.rules`) : deux moitiés,
  mesurées le 23/11/2026 sur un ASUS USB-BT500 (`0bda:b85b`).
  (1) `ATTR{power/wakeup}="enabled"` sur la clé (classe USB `e0`, donc toute
  clé Bluetooth) : sans ça la connexion entrante de la manette s'arrête au
  contrôleur et la machine ne bouge pas, même avec `WakeAllowed` posé ;
  (2) la règle `input` qui déclenche l'unité. Vérifié : DS4 appariée, réveil
  **6 s** après `rtcwake` avec le récepteur Logitech mis hors réveil — seule la
  manette pouvait réveiller, et elle se reconnecte ensuite toute seule (plus
  besoin de la rallumer après une veille). Contrepartie assumée : tout appareil
  Bluetooth apparié qui se reconnecte peut réveiller la machine (le réveil USB
  part avant le filtrage de BlueZ), donc retirer de BlueZ les appareils non
  voulus. BlueZ journalise `Controller resume with wake event 0x0` (« source
  non Bluetooth ») **même quand c'est la manette** — le dongle ne remonte pas
  d'événement HCI de réveil, ne pas s'y fier pour diagnostiquer.
- **gablue-disk-automount.service** : montage **lecture seule** des disques
  externes de la session jeu (`/usr/libexec/gablue-disk-automount`, démarré par
  `gablue-gaming-session` avec `restart`, jamais activé par défaut). En console
  il n'y a pas de plasmashell, donc personne ne demande à udisks de monter un
  disque branché : le service le fait pour l'utilisateur, ce qui donne **le même
  point de montage que Plasma** (`/run/media/<utilisateur>/<libellé>`) et les
  mêmes droits ; un disque que Plasma a déjà monté (bascule de session) n'est
  pas touché, et il n'est jamais démonté par nous. Filtre bus : `ID_USB_DRIVER`
  ou `ID_BUS=usb|mmc` — `ID_BUS` seul ne suffit pas, un boîtier USB-SATA (le T5,
  constaté le 23/11/2026) présente ses disques en `ata`. Deux garde-fous sur le
  chemin, qui est écrit dans `config.toml` : après un montage le point obtenu
  est comparé à celui que Plasma aurait pris (écart journalisé), et après un
  retrait le service vérifie qu'aucun montage mort ne reste dans `/run/media`
  (périphérique disparu = montage mort, udisks le démonte ; sinon le disque
  reprendrait un autre nom au rebranchement, donc un autre chemin). **Démontage
  en quittant la session** : `gablue-session-mode` arrête le service avant de
  tuer la session (le disque ne doit pas rester en lecture seule dans la
  session bureau), et le service s'arrête seul si la session « jeu » disparaît
  (`loginctl`). `udevadm monitor` tourne dans un **tube nommé** (pas un
  `| while`) : la boucle reste dans le shell principal et le moniteur est tué à
  la sortie — un `udevadm` orphelin garde le tube ouvert et fige l'appelant
  (constaté au banc). Testé au banc le 23/11/2026 (montage/parcours d'un
  évènement udev factice/démontage, guetteur de session) ; le branchement réel
  en session console reste à vérifier.
- **gablue-login-theme.service** et **gablue-login-greeter.service** (utilisateur, greeter seulement) : l'écran de connexion — `login-bridge.sh theme` écrit le `kdeglobals` du compte `plasmalogin` **avant** `plasma-login-kwin_wayland.service` (qui le lit à son démarrage), puis `login-bridge.sh pad` ouvre le pointeur manette et le clavier visuel **au premier mouvement** d'une manette (jamais au simple branchement). Les deux sont `WantedBy=plasma-login-wayland.target` (cible qui n'existe que dans la session de `plasmalogin`), réglées par `/etc/gablue/login.conf` (`pad`, `dark`), écrites par `/usr/libexec/gablue-login` et exposées par le dialogue « Écran de connexion… » de Gablue Manager. Contrat → `src/gablue-pad/AGENTS.md`
- **plasma-foreground-booster.service.d/10-after-plasmashell.conf** : `After=plasma-plasmashell.service`, correctif upstream kcgroups 0.2 absent du paquet Terra 0.1-3. Sans lui le booster démarre en parallèle du compositeur, Wayland n'existe pas encore, Qt ne trouve aucun plugin de plateforme et **abort** (coredump à chaque login, service relancé ensuite par `Restart=on-failure`). Après le bump Terra vers 0.2 le fichier devient un doublon inoffensif, à retirer. `plasmashell` plutôt que `plasma-kwin_wayland` : en `Type=dbus`, son « started » prouve un démarrage réel, alors que KWin en `Type=simple` peut être lancé sans avoir créé le socket

## Scripts utilisateur (/usr/bin)

- `gablue-apk-installer` : installateur APK par glisser-déposer — le code vit dans `src/gablue-apk-installer/` (paquet `gablue_apk_installer`, installé en `/usr/lib/gablue-apk-installer/` par `files/scripts/build-apk-installer`) ; contrat complet → `src/gablue-apk-installer/AGENTS.md`
- `gtracker` : GUI Rust (egui, Wayland natif) — lecteur et arrangeur multitrack de
  bandes-son rétro, compilée depuis `src/gtracker` (étape `gtracker-build`) ;
  `.desktop` + icône `hicolor/scalable/apps/gtracker.svg` (app_id et
  `StartupWMClass` doivent rester `gtracker`, sinon KWin affiche l'icône générique)
- Scripts gaming : `azahar-install`, `eden-install`, `esde-install`, `lichii-install`, `xenia-install` — **sauf `esde-install`, ce sont des coquilles** de `/usr/libexec/gablue-apps` (voir « Moteur d'installation des AppImages »)
- Scripts utilitaires : `dlv`, `dlcover`, `tvqt`, `ventoy`, `wallpaper-import`, `clean-media`
- `ssh-key-add` : ajoute la clé publique à un serveur (crée `~/.ssh/id_ed25519` ed25519 sans passphrase si absente, `ssh-copy-id`, vérifie la connexion sans mot de passe). Port custom : `-p/--port N`, `user@hôte:N`, IPv6 `[adresse]:N` (sans crochets dans la cible finale) ; `-p` + `:port` = erreur ; sans argument → invite la cible. Script stocké 644, `+x` posé par post-install (convention repo)
- `gremote` : bureau Plasma distant d'un poste Gablue, **RDP tunnelisé en SSH**. **Port SSH** : `-p/--port N` ou suffixe `hôte:port` (`[ipv6]:port`), comme `ssh-key-add` ; un port explicite n'est transmis à `ssh` que s'il a été fourni — un `-p 22` écraserait un `Port` de `~/.ssh/config` ; cible sans `@` → utilisateur local (sinon `krdpserver` recevait l'hôte comme nom de compte). Le mode **waypipe** (compositeur imbriqué, `-a`, `-s`, `--video`, `--codec`, `--quality`, `--bpf`, `--compress`, `--no-gpu`, `--clean`) a été **retiré le 02/11/2026** : il n'encodait que des dmabufs opaques 8 bits, imposait la même version de waypipe des deux côtés et ne servait qu'à `-a`. Historique : `git log -p` avant ce commit. Le type de session est annoncé dans la poignée de main (`GREMOTE-RDP-READY <port> <physical|virtual>`) : **VIRTUELLE** = la session hors écran créée par gremote, que le serveur reconnaît à sa **sortie virtuelle de KWin** ; **PHYSIQUE** = tout le reste. Client RDP : **`sdl-freerdp`** (SDL3, Wayland natif) dans les deux cas — `wlfreerdp` est déprécié en amont. Une session VIRTUELLE suit la fenêtre (`+dynamic-resolution`) : rien de physique n'est redimensionné, l'image reste nette. Une session **PHYSIQUE** (tout le reste) ne peut pas changer de résolution — ce serait l'écran réel du poste — donc l'image est affichée à sa résolution, sans mise à l'échelle : la fenêtre a la taille de l'écran distant. L'échelle automatique (`xfreerdp` + `+smart-sizing`, options `--fit-size WxH` / `--no-fit`) et le recalage au ratio pendant le redimensionnement (`fit_for_aspect` + `aspect_watch`, xdotool/xev/xprop) ont été **retirés le 14/11/2026** : la fenêtre se battait contre le gestionnaire de fenêtres et faisait moins bien qu'un affichage 1:1 (demande de l'utilisateur ; historique `git log -p`). La poignée de main n'annonce donc plus de 4e champ de géométrie. Un hôte d'une image antérieure n'annonce que le port : le type est alors **demandé à l'hôte sur une seconde connexion SSH** (`BatchMode`, `ConnectTimeout 5`, une seule, uniquement dans ce cas) — c'est la règle de cette image-là (socket `gremote-session` présent = virtuelle, sinon physique, angle mort compris) ; sans réponse (clé absente, réseau), physique, le choix sûr. **Détection du type côté serveur** (13/11/2026) : la sortie du backend virtuel de KWin s'appelle `Virtual-N` (littéral UTF-16 `Virtual-%1` dans `libkwin.so.6.8.2`, vérifié par `strings -el`) et c'est désormais le critère ; le **nom du socket** ne dit plus rien du type depuis Plasma 6.8, où le compositeur est lancé par son unité utilisateur et `kwin_wayland_wrapper` lui impose `wayland-N` — le socket `gremote-session` de l'ancienne conception n'existe plus, si bien que TOUTE session virtuelle était annoncée PHYSIQUE : fenêtre figée à la résolution du moniteur virtuel et qualité adaptative au lieu du suivi de fenêtre (vécu 13/11/2026 ; le nom `gremote-session` reste accepté pour un hôte d'une image antérieure). Deviner à l'aveugle donnait les deux fautes : supposer « virtuelle » coupait un bureau physique (vécu 06/11/2026), supposer « physique » figeait un bureau virtuel sans suivi de résolution (vécu 09/11/2026, hôte encore en 20261106). Mot de passe RDP par `/from-stdin:force`, jamais en argv (`ps` le lirait). Le code retour du **client** RDP n'est pas un échec : fermer la fenêtre (ou une coupure réseau) fait sortir `sdl-freerdp` en 131, et `set -e` propageait ce code — `|| true` autour du client, sinon tout appelant (GUI incluse) crie au plantage sur une session pourtant réussie (vécu 04/11/2026). `--quality`/`--adaptive`/`--static-quality` sont **transmis au `--serve` distant** (défaut : qualité 75, **adaptatif sur session physique** / épinglé sur virtuelle — « auto » n'envoie aucun drapeau, le serveur tranche seul puisqu'il connaît le type avant de lancer `krdpserver`) — sans ça ils étaient parsés puis ignorés. Son : un puits tunnel **par connexion** (`sink_name=gremote-$$`), déchargé au nettoyage seulement si son identifiant désigne encore NOTRE puits, défaut restauré seulement s'il est encore le sien — **PipeWire réattribue les identifiants de module**, sinon chaque reconnexion perd le son. **Connexion SSH dédiée** (13/11/2026) : les `-R` Pulse/PipeWire ne partagent plus le TCP du `-L` RDP — chaque rafale d'image bloquait le PCM derrière elle (blocage en tête de file) et chaque segment perdu le retardait d'un aller-retour, d'où des saccades sur un lien qui jitte (mesuré : RTT 58 ms, pics à 197 ms sur le Wi-Fi de l'hôte, tampon du tunnel à 25 ms ; la veille radio Wi-Fi de l'hôte en était la cause). Le tunnel a donc son `ssh -N -C -o ExitOnForwardFailure=yes` à lui (`-C` sur le PCM seulement : ~1,3-1,5× pour un CPU négligeable, zlib ne gagne rien sur le H.264 du `-L`), monté **avant** la connexion principale ; le serveur attend le socket 5 s (`module-tunnel-sink` échoue net sur un socket absent) et le client signale un tunnel mort au lieu de laisser croire au son. Tampon : `latency_msec=1500` (`GREMOTE_AUDIO_LATENCY` pour essayer) — **quirk mesuré sur PipeWire 1.6.11** : `node.latency` vaut valeur × 6 échantillons (soit /8 en ms) et plafonne à 8192 échantillons (`default.clock.quantum-limit`), donc ~171 ms à 48 kHz ; au-delà de 1500 aucun effet, à revérifier à chaque version. Puits orphelins : nettoyés **inconditionnellement** au démarrage du serveur (défaut local restauré s'il pointait sur un `gremote-*` ou sur un puits disparu) — c'est ce qui rend le son au poste distant en `--no-audio`, où aucun puits n'est créé. **Sortie de session, lecture en cours** (15/11/2026) : les flux encore branchés sur notre puits sont **déplacés vers le puits local** (`pactl move-sink-input`, côté serveur donc invisible pour l'application) AVANT l'`unload-module` — Chromium/Electron (VacuumTube, FreeTube) met la lecture **en pause** quand le nœud qu'il joue est détruit, si bien que décharger le puits sous ses pieds arrêtait la vidéo à l'instant exact où le son revenait au poste ; la mort du socket seule (fermeture du `ssh -R`, puits laissé en place) ne casse rien, c'est bien le RETRAIT du nœud qui coupe. Mesuré le 15/11/2026 sur Chromium 143 avec audio PipeWire réel (puits `module-tunnel-sink` sur un socket socat, même séquence que gremote) : sans déplacement la lecture se fige (`paused=true`, position arrêtée) sur ~1 essai sur 8 d'une page locale — un lecteur YouTube, dont l'horloge vidéo suit le son, y est bien plus sensible ; avec déplacement, aucun échec sur 5. Fonction `move_streams_to_local`, appelée en tête de `cleanup_rdp`. **RDP par défaut** (02/11/2026) : `gremote` expose la session graphique **existante** de l'hôte en RDP — `krdpserver --plasma` (le drapeau contourne le dialogue de permission du portail ; sans lui KRdp passe par `xdg-desktop-portal`), lié à `127.0.0.1:3389` derrière un `ssh -L 127.0.0.1:13389:127.0.0.1:3389`, donc **aucun port ouvert sur le réseau**. Mot de passe tiré au hasard à chaque session et passé sur **stdin** (jamais dans argv, donc invisible dans `ps`), `--no-launch` affiche la commande du client au lieu de la lancer. Deux pièges supplémentaires vécus le 02/11/2026 : (a) un **`ssh -L` accepte la connexion TCP locale même quand le distant refuse** — un test de port côté client ne prouve rien, xfreerdp partait trop tôt et mourait sur « channel 2: open failed: connect failed: Connection refused » ; le distant doit écrire une ligne `GREMOTE-RDP-READY` sur stdout une fois krdpserver en écoute, et le client attend celle-ci ; (b) `module-tunnel-sink` enregistre son puits **de façon asynchrone**, donc un `set-default-sink` immédiat échoue en « Aucune entité de ce type » et la session continue de jouer sur la carte locale — attendre l'apparition du puits, poser le défaut, puis **vérifier par `get-default-sink`** (vécu : puits créé, défaut inchangé, silence total).  Deux pièges : (1) `krdpserver` **sort en 255 sans un mot** s'il n'a pas de certificat TLS — l'erreur part dans la *logging category* `KRDP`, désactivée par défaut (`Server::start()` renvoie `false` si `server.crt`/`server.key` manquent) ; certificat auto-signé généré dans `~/.local/share/krdp/{cert,key}.pem`. (2) **KRdp 6.8 ne transporte pas le son** (`freerdp_settings_set_bool(FreeRDP_AudioPlayback, false)` + `// TODO` dans `src/RdpConnection.cpp`) ; le son passe donc par `pactl load-module module-tunnel-sink server=unix:$GREMOTE_AUDIO_SOCK sink_name=gremote` (le socket `-R` de l'audio) mis par défaut, déchargé et ancien défaut restauré à la sortie. **S'il n'y a aucune session, `gremote` la crée** (02/11/2026) : `--serve --rdp` démarre `gremote-session@<utilisateur>.service` (unité système, `User=%i` + `PAMName=login` + `XDG_SEAT=seat0` + `XDG_VTNR=3`, `kwin_wayland --socket gremote-session -- plasma_session`), attend son socket puis y attache krdpserver ; `--no-start` rétablit l'échec explicite. `PAMName=login` est le point non négociable : sans lui pas de session logind, donc pas de siège, pas de DRM master et rien à capturer — `systemd-run --user` ne suffit pas. Aucun mot de passe n'est demandé grâce à `50-gremote-session.rules`, qui autorise chacun à démarrer/arrêter **uniquement** `gremote-session@<son propre compte>.service` (jamais un NOPASSWD large, jamais sudo). Depuis le passage au **virtuel** (02/11/2026), la session est hors siège (ni `XDG_SEAT` ni `XDG_VTNR`) : plus de VT à réserver, plus de `chvt`, et l'écran physique garde le greeter — KRdp capture par le screencast interne de KWin et injecte par `fake_input`, jamais par le siège. Troisième piège vécu le 02/11/2026 : une session créée ainsi arrive **verrouillée** (`LockedHint=yes`) et **l'écran de verrouillage de KDE ignore l'entrée synthétique** — le client RDP affiche l'heure et la date, clavier et souris paraissent morts ; `kwin_wayland --no-lockscreen` dans l'unité (comme plasmalogin) corrige, la session étant déjà protégée par SSH puis par le mot de passe RDP éphémère. Quatrième piège, le plus vicieux (02/11/2026) : `PAMName=login` place les processus dans le scope logind (`session-XX.scope`), **hors du cgroup de l'unité** — un `systemctl stop` ne les tue donc pas, et `kwin_wayland` ignore SIGTERM. Le KWin rescapé garde le socket *et* le nom D-Bus `org.kde.KWin`, si bien que krdpserver s'attache à l'**ancienne** session, verrouillée : le client affiche l'heure et la date, entrée morte, alors que la nouvelle session est saine. D'où `ExecStopPost` qui SIGKILL les KWin de l'utilisateur portant `--socket gremote-session` (jamais celui du greeter, autre compte) et `ExecStartPre` qui refuse de démarrer sur un socket déjà présent. Cinquième piège, la cause du « client noir » : **KWin n'ouvre la carte DRM que si sa VT est active au moment où il démarre**. Sur VT3 inactive il démarre sans aucune sortie (`wl_output` absent), le screencast de KRdp n'a donc rien à capturer et le client reste noir alors que la session a l'air saine — mesuré : `krdpserver` à **0 tick** pendant une connexion établie, contre 359 ticks/5 s une fois la sortie présente. `loginctl activate` ne peut pas être utilisé dans l'unité (à `ExecStartPre` la session PAM n'existe pas encore, et `session/auto` ne se résout pas pour un service PAMName=login) : c'est `/bin/chvt 3` en root (`ExecStartPre=+`) qui bascule la console avant KWin. **Obsolète depuis le backend virtuel** : sans siège, KWin n'ouvre pas la carte et `chvt` a été retiré de l'unité ; ce piège ne revient que si on refait une session DRM. Vécu 02/11/2026, à ne pas refaire : **`org.kde.KWin` et `org.kde.plasmashell` sont uniques par bus utilisateur**, donc une deuxième session pour le *même* uid est impossible — plasmashell sort sans un mot (rc=0) et la seconde instance de KWin ne prend jamais le nom ; une session par utilisateur, ce qui convient au cas « personne n'est connecté ». Second vécu : **un shell qui hérite de `WAYLAND_DISPLAY` fait choisir à KWin le backend Wayland imbriqué** (« No backend specified, automatically choosing Wayland because WAYLAND_DISPLAY is set ») et `--virtual` est alors ignoré en silence — dépouiller l'environnement (`env -u WAYLAND_DISPLAY -u DISPLAY`) avant tout test de backend, sinon on teste une fenêtre de plus dans la session locale. Backend virtuel **adopté le 02/11/2026** : l'unité dépose un drop-in `~/.config/systemd/user/plasma-kwin_wayland.service.d/10-gremote-virtual.conf` (`ExecStart=kwin_wayland_wrapper --xwayland --virtual`), car le compositeur n'est plus lancé par nous mais par l'unité **utilisateur** de Plasma. `kwin_wayland_wrapper` ne déclare pas `--virtual` mais transmet les arguments inconnus à KWin — mesuré : témoin sans l'option = **0 sortie**, avec = `Virtual-0`. Résultat : aucun descripteur DRM, aucune VT, aucun siège ⇒ **plusieurs comptes en parallèle** (vérifié gab + bdc : deux compositeurs virtuels, deux sorties, chacun son plasmashell), l'écran physique reste sur le greeter, et le port d'écoute KRdp dérive du nom de compte (`3400 + hash % 200`, même calcul des deux côtés) pour que deux utilisateurs ne se disputent pas `127.0.0.1:3389`. Le drop-in est **retiré à l'arrêt** (`ExecStopPost`) : sinon les sessions **locales** suivantes du compte naîtraient hors écran. Règle de choix, sans exception (02/11/2026) : **session graphique déjà présente pour ce compte (locale ou distante) → on s'y attache ; aucune → on crée une virtuelle**. Donc une session physique ouverte est *partagée* (l'écran du serveur montre la même chose que le client) et on ne crée du virtuel que s'il n'y a rien — c'est ce qui permet de travailler à côté d'une session physique et de n'en ouvrir aucune quand on ne travaille qu'à distance. Seule fenêtre à risque : quelqu'un se connecte sur la console *pendant* une session virtuelle distante, le drop-in étant encore en place, sa session locale naîtrait hors écran. `--mode RemoteAccess` streame du noir (moniteur virtuel jamais rempli), `SharedAccess` encode (356 frames x264 mesurées sur `Virtual-0`). **Ne jamais tester `gremote` en tuant `krdpserver` ni en stoppant `gremote-session@<compte>` d'un utilisateur connecté** : le `ssh` de gremote porte le serveur, donc tuer le serveur coupe la session de la personne (vécu 02/11/2026, trois connexions coupées coup sur coup). Un test se fait sur un compte de test, ou en acceptant explicitement de déconnecter. Deux autres vécus du même jour : `plasma-kwin_wayland` lancé en `Type=oneshot` **bloque `systemctl start`** (startplasma-wayland ne rend jamais la main) ⇒ `Type=simple` + `startplasma-wayland & exec sleep infinity` ; Deux autres vécus du même jour : (a) un `startplasma-wayland` **orphelin** (ppid 1) laissé par un arrêt précédent — y compris d'un *autre* compte — fait échouer la déconnexion de la même façon ; l'unité tue désormais les orphelins de son compte en `ExecStartPre` et `ExecStopPost` (filtre `ppid=1`, jamais celui d'une session locale en cours). (b) `xfreerdp` interroge NetworkManager, ce qui déclenche une demande d'autorisation polkit « contrôle des connexions réseau » au milieu de la connexion : `/network:lan` fige le profil et supprime le dialogue. **La déconnexion demandée depuis Plasma ne marche que si `startplasma-wayland` est le processus principal de l'unité** : lancé en tâche de fond derrière un `exec sleep infinity`, l'unité ne s'arrête jamais et le logout échoue sur « Transaction for graphical-session.target/stop is destructive … contradicts existing jobs » (mesuré 02/11/2026 : kwin, plasmashell et ksmserver restaient vivants). et l'arrêt est un **deadlock** si on stoppe l'unité utilisateur en bloquant (KWin ignore SIGTERM, la session reste en `State=closing` et le gestionnaire utilisateur **relance** le compositeur) ⇒ `systemctl --user stop --no-block` puis `loginctl terminate-session`. Pistes écartées le 01/11/2026, pour ne pas les refaire : **Sunshine** n'injecte l'entrée que par `uinput`/`uhid`, donc il exige une session sur un **siège** (vu, mesuré : `uinput` atterrit sur le siège de la session qui l'ouvre) ; **vkms** est une impasse — KWin y ouvre la carte mais ne peint qu'une image (framebuffer identique au bit près après ouverture d'une fenêtre : vkms ne renvoie pas les page-flip), et `capture=kwin` fait **segfaulter** KWin sur cette sortie (`status=11/SEGV`) ; `KWIN_DRM_NO_AMS=1` est pire (plus de framebuffer scannée). `capture=kms` fonctionne, mais capture une image gelée. Reste valable : Sunshine sur un **siège réel** (session sur `seat0`/VT2 + `loginctl activate`), avec son par `sink-sunshine-stereo` — mais il ne peut pas streamer l'écran de connexion, et un seul utilisateur à la fois par VT. **polkit, session sans siège** : classée « non locale », elle reçoit `implicit_any` au lieu d'`implicit_active` — d'où des dialogues (appstream au démarrage) qu'une session au siège n'a pas. `70-gremote-seatless-active.rules` lui accorde ce qu'une session locale ACTIVE obtient sans mot de passe, **restreint à `wheel`** ; la liste est générée au build par `usr/libexec/gablue-polkit-active-list`, qui parse les `.policy` (`pkaction` exige un bus système, absent au build). `71-gremote-wheel-flatpak.rules` reprend la règle Fedora d'install Flatpak sans sa clause `subject.local`. `60-gremote-seatless.rules` teste `subject.local` : `subject.seat` vaut `null` même à seat0, et `!subject.seat` refusait NetworkManager à tout le monde. Pièges : `polkit.spawn` depuis une règle bloque (polkit tue la règle après 15 s) ; `action.implicit_active` n'existe plus en polkit 127 ; une règle de `/etc` ne remplace pas celle de même nom dans `/usr/share`, donc tester un correctif livré exige un build. **Écran disparu (14/11/2026)** : une session PHYSIQUE dont le moniteur s'éteint perd sa sortie — KWin la détruit, KRdp n'a plus rien à encoder (`target not found`, fenêtre noire) ; un écran éteint en DPMS qui garde son lien, lui, ne casse rien. gremote ne refuse donc plus une session sans sortie : il demande un écran VIRTUEL à KWin (`--virtual-monitor` à la taille et à l'échelle de l'écran disparu, lues dans `wayland-info`) et annonce la session VIRTUELLE. Pendant toute la connexion il surveille les connecteurs (`/sys/class/drm/card*-*/status` toutes les 0,5 s, confirmé par `wayland-info`) : si la sortie disparaît, il relance `krdpserver` sur un écran virtuel, recalcule la qualité (VIRTUEL = `--static-quality`), réannonce `GREMOTE-RDP-READY … virtual`, et le client relance `sdl-freerdp` sur le même port avec le même mot de passe (reprise en 2-4 s, sans reconnexion manuelle). Vérifié sur gen1 le 14/11/2026 : session à zéro sortie → `Virtual-1920x1080@1`, bureau capturé normalement ; et bascule physique → virtuelle en cours de connexion, client relancé seul. Pas de retour à l'écran physique quand il revient : on reste sur le virtuel jusqu'à la déconnexion. **Taille de fenêtre (14/11/2026)** : la poignée de main porte un 4e champ `<L>x<H>` (taille du bureau diffusé). Côté client, `/size:` est **calculé** par `window_size_for` pour les DEUX types de session : le ratio du distant, réduit pour tenir dans 80 % de l'écran local (`screen_size` lit `wayland-info`, échelle comprise). Jamais la taille exacte du distant — un 1080p sur un écran 720p ouvrirait une fenêtre plus grande que l'écran. Sans ce `/size:` en virtuel, le client ouvre sa fenêtre par défaut (souvent 4:3) et `+dynamic-resolution` donne cette forme au bureau : la session virtuelle n'avait plus le ratio de l'écran qu'elle remplace (vu le 14/11/2026). Une session PHYSIQUE ajoute `+smart-sizing` (redimensionnable, résolution distante intacte), une VIRTUELLE `+dynamic-resolution` (le bureau suit la fenêtre). Quand AUCUN écran n'est connecté, la taille diffusée vient de `remembered_geometry` (la plus grande sortie non virtuelle de `kwinoutputconfig.json`) et non de 1920x1080 en dur : un 21:9 éteint serait sinon diffusé en 16:9. Sans AUCUN écran physique (session créée par gremote, backend `--virtual` de KWin), KWin ouvre sa sortie en 1024x768 — 4:3, constaté sur sucina le 14/11/2026 — donc gremote annonce 1920x1080 (16:9) et `+dynamic-resolution` redimensionne la sortie virtuelle à la fenêtre : vérifié, `Virtual-0` passe de 1024x768 à 1920x1080 à la connexion. **Sortie de session (14/11/2026)** : `krdpserver` ne s'arrête pas quand le client part, donc le `--serve` restait bloqué sur son `wait` après la fermeture du tunnel — `sshd` ne lui envoie pas de SIGHUP, le trap de sortie ne tournait jamais et un `krdpserver` orphelin continuait d'écouter avec un mot de passe périmé. Le `--serve` détecte son reparentage à PID 1 (`ssh_session_gone`) et se nettoie ; le nettoyage inclut le retrait des entrées `Virtual-*` de `~/.config/kwinoutputconfig.json` et des dispositions qui les référencent (`forget_virtual_outputs`, après avoir attendu la destruction de la sortie, sinon KWin réécrit l'entrée) — sans quoi la géométrie mémorisée replace le bureau dans l'écran virtuel au retour de l'écran physique. Vérifié le 14/11/2026 : plus aucun `--serve`/`krdpserver` ~1 s après la sortie, trace `Virtual-*` injectée retirée, JSON resté valide. Script stocké 644, `+x` posé par post-install. **Banc d'essai : jamais sur un compte réel** — le serveur tue le `krdpserver` du compte et un `stop` de l'unité coupe la session en cours (vécu 04/11/2026 : bdc a perdu la sienne) ; mesurer sur une session déjà ouverte par son titulaire, ou demander.
- `gablue-atuin` : intégration historique ble.sh + atuin (ex-`ujust atuin-setup`, sans recette de retrait). **S'exécute en utilisateur** (aucun sudo) : `on` installe ble.sh dans `~/.local/share` et ajoute un bloc balisé dans `~/.bashrc`, les lignes héritées d'ujust étant fondues dedans pour ne pas charger deux fois ; `off` retire le bloc, le garde-fou `/run/.containerenv` qui l'entoure et ble.sh, après instantané `~/.bashrc.gablue-atuin.bak`. **Ne supprime jamais** `~/.local/share/atuin` (historique) ni `~/.config/atuin`. `status` sort 0 si l'intégration est présente. Appelé par Gablue Manager ; script stocké 644, `+x` posé par post-install
- `gablue-pad` : options du pad Gablue (service **utilisateur** `gablue-pad.service`, aucun sudo). `on|off|status` coupe ou relance tout (masque systemd, `stop`, démarre) ; `menu|shortcuts|inhibit on|off|status` règlent les trois fonctions séparément en écrivant `~/.config/gablue/gpad-options` puis en envoyant SIGHUP au service (relu à chaud, aucune reconnexion). **Migration one-shot** : un masque posé sur l'ancien `gablue-gamepadshortcuts` coupe le nouveau et écrit les trois options à `off`. Appelé par les trois cases de Gablue Manager. Script stocké 644, `+x` posé par post-install
- `gablue-dualsense` : firmware des manettes DualSense / Edge en USB (`status`, `check`, `update`, `--target`). **Aucun privilège** : hidraw est déjà en `uaccess` (`ublue-os-udev-rules` + `steam-devices`) ; le flash passe par `gablue-dualsense-ctl` (C, hidapi, `src/gablue-dualsense/`), le script détecte la famille de firmware (0004/000B/000E/0044), télécharge l'image Sony et vérifie. Protocole, pièges (le START accepte une mauvaise famille, refus de la même version) et maintenance de la table des versions → `src/gablue-dualsense/AGENTS.md`. Script stocké 644, `+x` posé par post-install
- `gablue-manager` : lanceur (4 lignes) de « Gablue Manager » ; le code vit dans `src/gablue-manager/` et s'installe en `/usr/lib/gablue-manager/` — GUI unique pour les postes SSH (état + latence, `gremote`, Dolphin sftp, terminal, WoL, arrêt/redémarrage/maj, Avahi, import `~/.ssh/config`, ajout + copie de clé) **et pour la machine courante** (serveur SSH, annonce mDNS, mises à jour Gablue/Flatpaks/gwine, options avancées — ex-`gablue-config`, supprimé) ; remplace aussi `gablue-update` (script Python + `.desktop`, supprimés) ; ex-`gablue-ssh` (détail → `src/gablue-manager/AGENTS.md`)
- Gestion Wine/Proton : `gwine` (assemblé depuis `src/gwine-launcher/`), `scrap-win`
- `konsole-run` : wrapper « Lancer dans Konsole » (détail → § konsole-run ci-dessous)

### opencode2-install

Installe/met à jour OpenCode 2 (`@opencode/cli` npm global, symlink `~/.local/bin/opencode`), l'app OpenCode Desktop, les entrées Dolphin « Ouvrir avec » et le MCP Lightpanda (remplace les anciens scripts standalone, supprimés ; chrome-devtools retiré 10/2026 : ~490 Mo + 155 Mo RAM au repos pour un cas rare — lightpanda couvre lecture/extraction/interaction, Cloudflare actif → renoncer).

- **allow-scripts persistante additive** (`ensure_allow_scripts`, 10/2026) : tarball npm = stub 229 o, vrai binaire fabriqué par le `postinstall` — npm ≥ 11.19 bloque sans allowlist. L'updater interne (`opencode upgrade`) réinstalle SANS `--allow-scripts` → sans allowlist persistante, MAJ silencieusement cassée (stub jamais remplacé). `npm config set allow-scripts=X` REMPLACE la valeur entière (entrées écrasées mutuellement, constaté) → sed sur la ligne `allow-scripts=` de `~/.npmrc` (pas `npm config get`, un .npmrc projet prioritaire au cwd fausserait) + ajout, idempotent ; appelé dans tous les cas (même skip MAJ)
- **Désinstallation V1 = nom npm `opencode-ai`** (fix 10/2026, EEXIST constaté sur megafed) : le paquet V1 s'appelle `opencode-ai` — `@opencode-ai/opencode` n'existe PAS (404 registre) → l'étape 1 ne matchait jamais, la V1 restait en place et son bin-link `opencode` (préfixe npm Homebrew, possédé par l'autre paquet) faisait échouer `npm install -g @opencode/cli` avec `EEXIST: file already exists ... bin/opencode`
- **Fix TLS node sans privilège** (10/2026, constaté ideapad7) : brew node ≥ 26.11 lié à openssl@4.0.3 ignore le magasin de certificats intégré — TOUTE vérification TLS node échoue (`UNABLE_TO_GET_ISSUER_CERT_LOCALLY`) alors que curl/openssl système passent, le moteur est sain et `rootCertificates` est rempli ; node 26.10 lié à openssl@3 non affecté. Sonde https vers registry.npmjs.org → si cassée : `NODE_EXTRA_CA_CERTS=<bundle système>` exporté pour le script et persisté dans .bashrc/.profile, + `npm config set cafile` en ceinture de sécurité (survit hors shell interactif : services, cron). Idempotent, sans privilège, sans effet quand brew corrige (CAs additionnels redondants mais inoffensifs)
- **Spec `@latest` explicite sur tous les npm install** (10/2026) : sans spec, npm réifie l'existant sans monter de version (« changed N packages » trompeur)
- **Skip MAJ (étape 2bis)** : CLI + Desktop + Lightpanda à jour ET artefacts en place (symlink, AppImage, .desktop patché, entrées Dolphin, binaire Lightpanda, entrée lightpanda dans opencode.json, allowlist complète) → sortie immédiate. Réseau/version amont indéterminable → poursuite complète (fail-safe : répare ce qui manque) ; artefact absent/manquant → poursuite. Étape 4 : skip `npm install -g` si CLI déjà à jour ; étape 6 : skip download Desktop (préexistant). `--force` : bypass de tous les skips, argument inconnu refusé (usage)
- **Desktop via GearLever** (pattern `eden-install`) : `~/AppImages/opencode.appimage` + `.desktop`. Source `https://opencode.ai/download/stable/linux-x64-appimage` (redirection → assets v2 sur opencode.ai ; releases GitHub restent v1.18.x). Versions appariées npm (ex. 2.0.16 des deux côtés) : cible extraite de la redirection (fallback `npm view`), installée lue dans `X-AppImage-Version` — skip si identiques ; sinon curl fail-fast AVANT tout retrait, `gearlever --remove` (+ fallback manuel), réintégration, patch `.desktop` : `Name=OpenCode` sans suffixe + `--no-sandbox` (chrome-sandbox SUID absent sur Kinoite, comme lmstudio). Icône `$HOME/AppImages/.icons/opencode`
  - **Re-détection .desktop APRÈS intégration** (fix 25/09) : détection avant intégration → `DESKTOP_ENTRY` vide → patch jamais appliqué à la 1re install. Détection par grep après `gearlever --integrate`, patch dans `patch_desktop_entry` (idempotent), aussi appliqué quand déjà à jour (répare les installs non patchées)
- **Lightpanda** (étape 5bis) : `@lightpanda/browser` npm global = wrapper JS seul — depuis la 1.6.0 (10/2026, « Do not download binary on installation ») plus de postinstall : binaire nightly (~180 Mo) téléchargé via le CLI du paquet (`$(npm prefix -g)/bin/lightpanda install`, chemin explicite indépendant du PATH, checksum vérifié) vers `~/.cache/lightpanda-node/lightpanda` ; partiel non exécutable d'un téléchargement interrompu purgé avant (ETIMEDOUT constaté) ; warning EBADENGINE (node 22 requis vs node 26) non bloquant ; **fix manuel du binaire → `opencode service restart` obligatoire** : le statut « failed » (NotFound: ChildProcess.spawn) est porté par le serveur de fond et les sessions créées avant le fix, redémarrer le TUI ne suffit pas (clients se reconnectent au même serveur) — le script le gère via MCP_UPDATED ; restart transparent même depuis une session opencode active : les conversations reprennent automatiquement après le restart (vécu 10/2026) ; allow-scripts/ensure_allow_scripts conservés (sans effet depuis la fin du postinstall, défense si upstream re-modélise) ; skip si version npm = dernière ET binaire exécutable ; maj ponctuelle : npm update + `lightpanda upgrade` (binaire nightly seul)
- **Activation config** (étape 5ter) : entrée `mcp.servers.lightpanda` dans `~/.config/opencode/opencode.json` si absente (python stdlib, merge idempotent — serveurs/flags préservés, entrée `disabled` = no-op). JSONC illisible → warning + poursuite (binaires installés, non branchés). Présence = condition de sortie 2bis (réparée au run suivant si retirée à la main). Coût ~180 Mo + ~21 Mo RAM. **`opencode.json` peut être un symlink** (config partagée : `install.sh` du dépôt Opencode lie `config/*`, seul `cli.json` reste local) → l'écriture passe par un temporaire dans le dossier de la cible RÉSOLUE puis `os.replace` sur le chemin réel ; écrire en place (ou remplacer le lien) cassait la config synchronisée (vécu 17/10 : tous les MCP invisibles, `opencode mcp list` muet ; réparé par `install.sh`)
- **`MCP_UPDATED`** : 1 si Lightpanda installé/maj OU entrée créée → étend la condition de redémarrage du service de fond ; échecs install non fatals (warning + poursuite) ; `--force` réinstalle aussi Lightpanda

### opencode2-remove — désinstallation sans perte

Désinstalle OpenCode 2 en conservant **configuration et données** : `~/.config/opencode/`, `~/.config/ai.opencode.desktop/` (`drafts.sqlite`, Local Storage, sessions du desktop), `~/.local/share/opencode/` (historique) et `~/.npmrc` (allow-scripts, cafile) ne sont jamais touchés — `opencode2-install` retrouve tout tel quel.

- Ordre : arrêt du **serveur de fond** (`opencode service stop`, il porte toutes les sessions CLI et desktop) puis de l'app Desktop (SIGTERM au processus principal seulement — tuer l'arbre démonte le squashfs `/tmp/.mount_*` sous les enfants → SIGBUS, même piège que `opencode-open`), retrait GearLever + AppImage + `.desktop` + icônes, entrées Dolphin `gablue-opencode*.desktop`, **lignes `inode/directory=` de `mimeapps.list` retirées une par une** (celles de `[Added Associations]` ; les autres associations restent, section vidée supprimée) — le défaut `[Default Applications] inode/directory=org.kde.dolphin.desktop` posé par l'installateur reste, c'est le bon défaut système, `npm uninstall -g @opencode/cli` puis `@lightpanda/browser` (+ `~/.cache/lightpanda-node`), symlink `~/.local/bin/opencode` **seulement s'il s'agit d'un lien** et wrapper `opencode-open`
- Idempotent, chaque étape best-effort, sans dialogue : appelé par la ligne « Supprimer » de la section Applications de `gablue-manager`. L'entrée `mcp.servers.lightpanda` d'`opencode.json` reste en place (config non modifiée) — réinstaller la remet en service. Script stocké 644, `+x` posé par post-install

### opencode-open (« Ouvrir avec » → GUI)

Wrapper qui ouvre le dossier directement dans OpenCode Desktop. Mécanisme rétro-ingénierié du bundle 2.0.16, validé en test réel :

- **Route de démarrage = localStorage Electron** (pas le store SQLite) : clé `opencode.desktop.window.<uuid>.last-active-url` dans `~/.config/ai.opencode.desktop/Local Storage/leveldb/` — route ≠ `/` imposée au boot, fallback Home → projet le plus récent
- **Format LevelDB** : blocs 32 Ko, records `[crc32c masqué(4) len(2) type(1)]` + payload — types Full=1/First=2/Middle=3/Last=4, un record ne chevauche jamais deux blocs (batch long fragmenté) ; batch `[seq(8) count(4)]` + entries `[type=1][varint klen][key][varint vlen][value]` ; CRC32C Castagnoli masqué `((c>>15 | c<<17) + 0xa282ead8)`
  - **Bug corrigé 25/09** : count lu en offset 4-8 (poids fort du seq, toujours 0) au lieu de 8-12 → « WriteBatch has wrong count » → batch jeté par leveldb → route perdue, switch aléatoire. Depuis : scan par bloc + réassemblage, réparation des batchs (count réécrit + CRC recalculé), route en NOUVEAU batch final (`seq = max(seq+count)+1`, aucun batch de l'app modifié, dernier batch appliqué = gagnant) ; log actif = `*.log` au numéro max (compaction → patcheur crée un nouveau log ; la récupération rejoue tout log plus récent que le manifeste). Résidu assumé : un batch historique clobberé est jeté à chaque boot (1 ligne « Ignoring error », sans effet — le réparer réappliquerait des valeurs potentiellement corrompues). App obligatoirement arrêtée (LevelDB verrouillé + buffer mémoire)
- **Séquence** : (1) dossier canonique (`cd` + `pwd -P`, fichier → parent) ; (2) app ouverte ET dossier déjà actif (store `tabs.info`/`tabs.recent` — clé session OU `draft:<uuid>` avec directory dans le tab) → focus deep-link, fin ; (3) cible SPEC (« session:<sid> » ou « draft:<uuid> ») décidée en UN point unique (les 2 patchers consomment la même valeur) : sessions → réutilise la dernière racine non archivée (`GET /api/session?directory=<encodé>&order=desc&limit=10`, filtre `parentID`/`time.archived`) via PATCH title="" (bump `time.updated` session + `time.active` projet → rang 0) ; vierge → DRAFT client-side zéro serveur (tab draft + route `/new-session?draftId=<uuid>`, promu au 1er prompt ; draftID réutilisé) ; fallback : session connue de tabs.info → réutilisation, sinon draft local ; auth Basic `~/.config/opencode/service.json`, URL via `opencode service status` ; (4) arrêt : SIGTERM au process principal SEULEMENT (tuer l'arbre démonte le squashfs `/tmp/.mount_*` sous les enfants → SIGBUS cascade, coredumps), attente arbre complet (zygote/gpu/renderer/crashpad) 20 s, SIGKILL last resort ; (5) patch SQLite `drafts.sqlite` (table `state`, PK `(name,key)`, `updated_at` epoch ms obligatoire) : `projects.local` + `lastProject` + `home.selection` + `tabs`/`tabs.recent` — 3 tentatives avec relecture (verrou WAL de l'app qui s'arrête) ; (6) patch LevelDB ; (7) relance deep-link `opencode://open-project?directory=<encodé>`
- **Sessions jamais perdues** : serveur de fond partagé, la GUI n'est qu'un client (restart ~2-3 s)
- **Alias symlink = projet vide en double** (fix 26/09) : sur ostree `/home` → `/var/home`, un chemin `/home/...` créait une entrée `projects.local` distincte (GUI déduplique par chaîne exacte, les symlinks ne sont pas résolus) → projet sans aucune session. Fix triple : `pwd -P` partout, patcher SQLite dédoublonne par `os.path.realpath` (fusionne vers le canonique), patcher tourne à chaque ouverture (répare au fil de l'eau). Échec silencieux du patcher (verrou WAL transitoire, 3 tentatives) = store inchangé sans trace → test verbeux pour diagnostiquer
- **Auto-rename natif préservé** : titre généré seulement si `title === undefined` ou défaut « New session - <date> » — un titre explicite bloque le renommage pour toujours → wrapper ne pose jamais de titre (nouveaux dossiers = drafts, promus sans titre au 1er prompt)
- **Pas de switch sans fermeture en 2.0.16** (confirmé exhaustivement : 965 chunks scannés, zéro listener deep-link ; le plumbing upstream existe — `layout.tsx` handleDeepLinks, PRs anomalyco/opencode#15479/#45103) → dès qu'une version le livre, le deep-link fera le switch live, script inchangé
- **Entrées .desktop** : `gablue-opencode.desktop` (GUI, `NoDisplay=true`, défaut) + `gablue-opencode-cli.desktop` (`Terminal=true`, `Exec=... --cli` → TUI dans le terminal système, fallback `~/.local/bin/opencode` explicite — PATH de session Dolphin incomplet)
- **Ordre « Ouvrir avec »** : position dans `[Added Associations]` de `~/.config/mimeapps.list` (1er listé = en haut ; `InitialPreference` 10 GUI/5 CLI = repli). Installateur écrit `inode/directory=gablue-opencode.desktop;gablue-opencode-cli.desktop;` en tête de section (idempotent, sections dupliquées/vides purgées) **et épingle `[Default Applications] inode/directory=org.kde.dolphin.desktop`** si aucun défaut n'existe (défaut utilisateur — Krusader, Thunar — respecté). Le défaut est indispensable : sans lui KDE prend le **premier de la liste fusionnée**, donc notre entrée Added, et *tous* les dossiers (double-clic Dolphin, dialogues d'ouverture des autres applis) s'ouvraient dans OpenCode (constaté 08/11/2026). Piège de diagnostic : `xdg-mime query default inode/directory` répondait quand même `org.kde.dolphin.desktop` (GLib ordonne autrement) ; seul le comportement réel de KDE suit la liste fusionnée

### Scripts gaming et utilitaires

- `esde-install` : ES-DE (AppImage GitLab + GearLever), copie `custom_systems` → `~/ES-DE/`, lanceurs Tools (Gablue TV fullscreen, Jellyfin Desktop mode TV, YouTube VacuumTube) dans `<ROMPATH>/tools/` (ROMPath lu dans `es_settings.xml`, défaut `~/Roms` ; lanceurs existants non écrasés). Réinstallation refusée → fichiers Gablue copiés QUAND MÊME (fonction partagée). Nouvelle AppImage téléchargée et vérifiée AVANT suppression de l'ancienne. Sources : `/usr/share/ublue-os/gablue/esde/tools/`. Deux options pour Gablue Manager : `--yes` (aucune boîte kdialog, le journal de la tâche fait office de retour) et `--config-only` (recopie `custom_systems` + lanceurs Tools sans vérifier ni télécharger l'AppImage : ni jq, ni GearLever, ni `update-desktop-database` requis). Le manager lit le script avant de l'appeler : sur une image antérieure à ces options il fait la copie lui-même, sinon le script ignorerait l'argument inconnu et relancerait l'installation interactive
- `azahar-install` / `xenia-install` : **coquilles** — résolution de la release, téléchargement et intégration GearLever vivent dans le moteur `gablue-apps` (voir plus bas). Le moteur lit le flux Atom du dépôt sans `api.github.com` (quota anonyme 60 req/h par IP, partagé par tout le réseau) pour le tag (des hachages de commit chez Xenia), puis la page `expanded_assets/<tag>` pour le nom exact de l'asset (celui d'AzaharPlus porte la version : `azaharplus-2126.2-A-linux.AppImage`, impossible à construire d'avance ; chez Eden le `.zsync` porte le même préfixe, d'où un motif ancré en fin de nom). **Un amont qui glisse la version dans le nom de son asset ne casse que le motif du catalogue** (Xenia Edge : `xenia_edge_20261127.1_linux.AppImage`, constaté le 28/11/2026 ; le `probe` nomme l'asset absent, et Gablue Manager signalait l'application non vérifiée au lieu de « à jour ») : corriger `asset_regex`, pas le moteur. `jq` n'est plus requis
- `lichii-install` : **coquille** (Lichii, émulateur Switch moderne, fork d'Eglantine, en **build PGO** — la seule installée, c'est la recommandée par l'amont). Le moteur prend le tag par la **redirection** de `/releases/latest` **avec `Cache-Control: no-store`** — le flux Atom porte des releases sans préfixe (`build-121`) et des pré-releases que `/releases/latest` écarte, et sans cet en-tête GitHub a servi un 504 mis en cache (09/11/2026) ; asset `lichii-linux-<arch>-pgo.AppImage` (`uname -m`). GearLever nomme l'entrée « Lichii (build-125-cbb512e) » → `.desktop` repatché en `Name=Lichii` (même règle pour Azahar et Xenia Edge, `X-AppImage-Version` retirée, et renommage `eden.desktop` pour Eden : champs `desktop_name`/`desktop_file`/`desktop_drop` du catalogue)
- `eden-install` : **coquille** — Eden publie sur sa propre instance Gitea (`releases/latest`, assets `Eden-Linux-<version>-amd64-clang-pgo.AppImage`), pas sur GitHub : le moteur gère les deux
- `wgpadd` / `lgpadd` : ajout en masse de packs ES-DE. `wgpadd` : `.wgp` → `--windows` défaut (lanceur `.sh` dans `.es-wgp` + lien `~/Roms/windows`), `--xbox360` (lien direct, système natif) ; `lgpadd` : `--desktop` défaut / `--switch` (lien direct, natif). Sans argument = tous les packs du dossier, avec fichiers = uniquement ceux-là ; cover téléchargée si absente
- `openpackdir.sh` : ouvre le dossier de sauvegardes ou d'extras/cache d'un pack `.wgp`/`.lgp` (actions « Ouvrir le dossier de sauvegardes » / « Ouvrir le dossier des extras/cache » d'`EditWGP.desktop`/`EditLGP.desktop`). Lit `.savepath`/`.extrapath` par `unsquashfs -cat` (aucun montage) et ouvre le dossier RÉEL du lanceur — WGP : `~/Windows/UserData/$USER/LocalSavesWGP/<nom>` et `~/.cache/wgp-extra/<nom>` (gwine) ; LGP : `~/.local/share/lgp-saves/<nom>` et `~/.cache/lgp-extra/<nom>` (lgplaunch) — créé s'il n'existe pas (jeu jamais lancé) ; nom interne lu dans `.gamename` (repli basename) ; marqueur absent du pack → kdialog informatif, rien n'ouvre ; ouverture `setsid dolphin` (repli `xdg-open`, repli message si pas de GUI)
- `lgplaunch` : lancement `.lgp` (squashfs squashfuse, saves/extras/temp via symlinks `/tmp/lgp-*` + overlays kernel dans `unshare -U -m`) et exécutables Linux directs ; modules Bash dans `src/lgplaunch/` (`lib/*.sh` assemblés en `/usr/bin/lgplaunch` par `files/scripts/build-lgplaunch`). Contrat complet (montage, amorçage, `--exe`, overlays) → `src/lgplaunch/AGENTS.md`
- `limitw` : limites TDP CPU/APU AMD via ryzenadj (bash, sudoers nopasswd). `--temp` temp max, `--save`/`--reset` persistance autostart, `--info` (non lisible sur certains desktop — Raphael/Dragon Range, pas de chemin PM table)
- `dlv` : téléchargeur YouTube unifié (playlist : `--mp3`, `--mp4`, `--mkv`, `--mkv-1080`). Remplace `dlv-mp*`/`ytdl` ; completion bash `/usr/share/bash-completion/completions/dlv`

### konsole-run — action « Lancer dans Konsole » (Dolphin)

Wrapper `usr/bin/konsole-run` + service menu qui REMPLACE celui du paquet konsole (MimeType étendu à `application/x-shellscript` — l'original ne couvre que `x-executable`). Sa source vit **hors** des chemins possédés par un paquet (`usr/share/ublue-os/gablue/kio/konsolerun.desktop`) et `post-install` la repose en `/usr/share/kio/servicemenus/` : posée au `COPY files/system/all`, elle était écrasée par rpm, le `dnf5 upgrade` de `files/scripts/rpm` tournant **après** ce COPY (vécu 15/11/2026 : konsole 26.08.3 a réinstallé sa version, l'action retombait sur le `--hold` natif — ni en-tête, ni shell final) :

- Affiche chemin + nom au lancement, exécution DIRECTE (shebang respecté), shell interactif à la fin (`exec bash`) — le `--hold` natif laissait une session morte non saisissable
- Script sans +x : `+x` ajouté le temps de l'exécution puis permissions restaurées à l'identique (trap EXIT/INT/TERM/HUP — survit à Ctrl+C/fermeture de fenêtre ; bits spéciaux inclus via `stat -c %a`) ; chmod impossible → erreur claire, rien n'est exécuté
- chmod +x posé par post-install (convention repo) ; `finalize` revérifie que l'override n'a pas été re-écrasé (`cmp` source ↔ cible, build en échec sinon)
- **Double-clic NON couvert** : KIO (`OpenUrlJob::handleScripts`, ex-KRun) exécute les +x en direct sans consulter l'association MIME ; 3 issues codées en dur (`kiorc` `[Executable scripts] behaviourOnLaunch` = execute/alwaysAsk/open) — aucune ne lance Konsole. Vérifié test bac à sable + code KIO 10/2026
- Validé local 10/2026 (perms 644 → 755 pendant → 644 après, interruption restaurée) avant intégration

### Règle udev ThinkPad (`/usr/lib/udev/rules.d/99-thinkpad-thresholds-udev.rules`)

Même mécanisme d'override que § konsole-run : source en `usr/share/ublue-os/gablue/udev/`, reposée par `post-install`, revérifiée par `finalize`. Elle garde les `chmod` des seuils de charge derrière `ENV{POWER_CHARGE_*_THRESHOLD}!=""` (commit `36678410`, « fix battery error for non compatible thresholds ») : sans ces gardes, un portable dont le firmware n'expose pas ces attributs déclenche des `RUN+=chmod` en erreur. Le fichier appartient au paquet `ublue-os-udev-rules` → réinstallé par le `dnf5 install` des paquets ublue (0.10 le 15/11/2026), d'où la repose.

### Binaire manette (gpad, gpad-trigger, /usr/bin)

L'ancien `gamepadshortcuts` (et son `kbdnav`) est **supprimé** : remplacé par le
projet `src/gablue-pad/` — overlay Wayland `gpad`, déclencheur `gpad-trigger`,
gate BPF, plus `gpad-mouse` (Home+R3), le clavier virtuel de l'overlay
(Home+carré, table dérivée du keymap X11 — plus de presets
`/etc/xdg/plasmakeyboardrc`), `gpad-inhibit`
et `gpad-rumble`, `gpad-shot` (capture d'écran directe par KWin, sans
Spectacle). Gestes, options, services et architecture →
`src/gablue-pad/AGENTS.md` et ses sous-docs.

### Binaire gablue-isomount (/usr/bin)

Monteur d'images disque en C natif (~2,7 Mo RAM), remplace le plugin dolphin-plugins mountisoaction (bug KDE #471487) :
- Monte ISO/IMG/EFI via UDisks2 DBus (LoopSetup + Filesystem.Mount) ; ouvre une fenêtre Dolphin (panneau Devices à jour)
- Réinjecte le LD_PRELOAD composefs-fix avant son `execlp("dolphin")` (ne passe pas par le .desktop patché — voir files/scripts/AGENTS.md § composefs)
- Démontage auto quand toutes les instances Dolphin sont fermées ; device occupé → attend sa libération ; déjà montée → nouvelle fenêtre sans remonter
- Service menu « Monter » + app par défaut MIME ISO/IMG/EFI ; log `/tmp/gablue-isomount.log`
- Règle polkit `usr/share/polkit-1/rules.d/49-gablue-isomount.rules` : autorise `loop-delete-others` (session locale active + wheel). udisks perd le suivi du propriétaire d'un loop quand le fichier image est supprimé pendant le montage (backing file marqué « (deleted) » par le noyau) → le Loop.Delete d'auto-nettoyage exigeait `auth_admin_keep` = prompt mot de passe à la fermeture de Dolphin (vécu 18-19/10, Kirby + Tails .lgp) ; l'unmount reste silencieux, seul le Loop.Delete était touché

### Interface tvqt (/usr/bin)

TV Gablue en Python (PySide6 + libmpv) : chaînes HLS, navigation manette
D-pad, lecteur intégré (`libmpv` + `QOpenGLWidget`), logos en cache, filtre par
pays, guide TV virtualisé. Le code vit dans `src/tvqt/` (paquet `gablue_tv`,
installé en `/usr/lib/tvqt/` par `files/scripts/build-tvqt`) ; le binaire
`/usr/bin/tvqt` ne change pas (menu, `gablue-tv.desktop`, ES-DE, `killthemall`).

Contrat complet (paquet, vues, manettes, EPG) → `src/tvqt/AGENTS.md`.

### Gablue Manager (`/usr/bin/gablue-manager`)

Lanceur de l'application « Gablue Manager » : le code vit dans
`src/gablue-manager/` (paquet `gablue_manager`, installé en
`/usr/lib/gablue-manager/` par `files/scripts/build-manager`). Entrée de menu
`usr/share/applications/gablue-manager.desktop` en catégorie **Système**, icône
`usr/share/icons/hicolor/scalable/apps/gablue-manager.svg` (carré bleu Gablue,
anneau en engrenage à 8 dents formant un G : ouverture en haut à droite et barre
horizontale, sans rupture de silhouette) ; `QIcon.fromTheme("gablue-manager")` avec repli `network-server`
quand l'icône n'est pas encore installée (lancement depuis le dépôt) ; et
`gablue-manager-alert.svg` (même icône + disque rouge `#e01b24` sur anneau blanc
à 34 % du côté) pour la **pastille d'alerte** de la barre d'état : l'icône Rust
change de nom d'icône (`gablue-manager-alert`), elle ne repeint rien.

Contrat complet (paquet, dialogues, privilèges, comportement) →
`src/gablue-manager/AGENTS.md`.

### Gablue Share (`/usr/bin/gablue-share` + `/usr/libexec/gablue-shared`)

GUI de partage de dossiers sur le LAN et le tailnet : le code vit dans
`src/gablue-share/` (paquet `gablue_share` installé en `/usr/lib/gablue-share/`
par `files/scripts/build-share`), le démon Rust `gablue-shared` (~3 Mo de RSS)
est compilé par l'étape `share-build` du Containerfile et posé en
`/usr/libexec/`. Entrée de menu `usr/share/applications/gablue-share.desktop`
(catégorie **Network**) et icône
`usr/share/icons/hicolor/scalable/apps/gablue-share.svg` (carré bleu Gablue,
dossier dont le contenu part : flèche montante). Motif choisi pour la **masse
pleine** : la même icône sert de pastille de barre d'état (`tray.rs`) et
d'icône de notification, donc elle doit se lire à 22 px à côté de l'applet
réseau — un motif d'ondes ou de glyphe « partage » s'y confond avec le wifi ou
devient illisible, mesuré au rendu 16/22/32 px avant de trancher.

Trois fichiers accompagnent l'application, tous chargés de la même vérité :

- `usr/lib/systemd/user/gablue-share.service` : mode **permanent** (le partage
  survit à la fermeture de l'app, redémarre avec la session). Livré **non
  activé** : c'est un opt-in depuis la GUI ou depuis gablue-manager, et
  `systemctl --user is-enabled` reste la seule vérité (jamais un champ JSON).
- `usr/lib/firewalld/services/gablue-share.xml` : ouverture de `47823/tcp` ;
  l'ajout à la zone par défaut est fait par `post-install`
  (`firewall-offline-cmd`, voir `files/scripts/AGENTS.md`). Le tailnet n'en a
  pas besoin : `tailscale0` est en zone `trusted`.
- `usr/share/applications/gablue-share.desktop` : lance le seul binaire
  `gablue-share`, qui démarre le démon s'il n'y en a pas.

Contrat complet (protocole, découverte, file de téléchargement, invariants) →
`src/gablue-share/AGENTS.md`.

### Widget panel synthetic-quota (/usr/share/plasma/plasmoids)

`org.gablue.synthetic.quota` (Plasma 6) — quota Synthetic dans le panel, équivalent du plugin TUI OpenCode :
- 10/2026 : l'API `v2/quotas` ne renvoie plus `rollingFiveHourLimit` — un seul pourcentage (`weeklyTokenLimit.percentRemaining`), fallback `subscription` (limit/requests). L'ancien second quota « REQ » (requêtes 5 h, badge limite, `nextTickAt`, config `showReq`) est supprimé du helper, du QML et de main.xml (le plugin TUI `synthetic-quota/tui.tsx` adapté en miroir)
- 04/11/2026 : champ RENOMMÉ encore par l'API — `planWeeklyLimit.percentRemaining`, plus de `weeklyTokenLimit` (la réponse contient aussi `search.hourly`, non affiché) : le helper lisait l'ancien champ, ratait `percentRemaining` et retombait sur le fallback `subscription` (`requests: 0`) → widget figé à `TOK 100.00%` en permanence. Correctif : `planWeeklyLimit` lu en priorité, ancien nom gardé en fallback (miroir du plugin TUI). 2ᵉ rename du champ en un mois — s'attendre à une 3ᵉ maj et re-tester le JSON brut en cas de blocage
- Compact : `TOK 96.52%` (code couleur Kirigami : positif > 50 %, neutre > 20 %, négatif ≤ 20 %) ; panel vertical = % seul ; étendu (clic) : barre tokens hebdo, crédits, dates régénération/renouvellement
- Helper Python `contents/code/gablue-synthetic-quota-helper` : lit `~/.local/share/opencode/opencode.db` (table `credential`, `integration_id='synthetic'`, SQLite read-only WAL-friendly) puis `https://api.synthetic.new/v2/quotas` — pas de dépendance au serveur OpenCode, rotation de clé ramassée au poll suivant. PAS de filtre `active = 1` : le serveur OpenCode récent stocke `active = NULL` (lignes legacy) et le filtre strict faisait afficher « no key » au widget (18/10/2026) — ordre `ORDER BY (active = 1) DESC, time_created DESC` (actif d'abord, legacy lisible), même logique que le plugin TUI `synthetic-quota/tui.tsx`
- Cache `~/.cache/gablue/synthetic-quota.json` (tmp+rename) : échec réseau → dernières valeurs `stale=true` ; `no-key` → `TOK ?` + tooltip
- Data engine `P5Support.DataSource` (`executable`), poll 60 s (`main.xml` : `refreshInterval`, `decimals`) ; clé jamais en clair hors la db OpenCode
- **Plasma 6.7** : `PlasmaCore.Theme` n'existe plus (→ `Kirigami.Theme.defaultFont`) ; signal `newData` = `(sourceName, data)` (2 paramètres, pas 3) ; `Component` top-level interdits en QML (inline components dans le `PlasmoidItem`)
- Essai local : symlink `~/.local/share/plasma/plasmoids/org.gablue.synthetic.quota` → répertoire du repo + restart `plasma-plasmashell.service`

### Scripts partagés gardés (`/usr/share/ublue-os/gablue/scripts`)

L'ancien dossier `scripts/gamepadshortcuts/` est **supprimé** ; trois scripts qui
servent ailleurs dans Gablue remontent d'un cran et gardent leur chemin d'appel :

- `killthemall` : bouton `KillEmu.desktop` **et** menu système du pad
  (`TOUT FERMER`, `Home+SELECT`). Tue les émulateurs de la session, plus tvqt,
  Jellyfin (flatpak) et VacuumTube/YouTube (flatpak) uniquement s'ils ont été
  lancés depuis ES-DE (`GABLUE_ES_LAUNCH=1`). Pièges : Jellyfin — le wrapper
  `flatpak run` disparaît et l'env est filtrée des bwrap → détection
  `pgrep jellyfin-desktop` puis `flatpak kill` + pkill des bwrap restants ;
  VacuumTube — variable visible seulement dans le wrapper `startvacuumtube` →
  `timeout 10 flatpak kill rocks.shy.VacuumTube` ; jamais de kill direct des
  processus du sandbox avant `flatpak kill` ; LGP — double ancre
  (`pgrep -f /tmp/lgpackmount` **et** balayage `/proc/<pid>/exe`, un binaire
  lancé en chemin relatif perd le mount dans sa cmdline) puis
  `fusermount -uz /tmp/lgpackmount-*` (détail et vécus : `git log` sur le script).
- `changefps` : alias `changefps` de `/etc/profile.d/customperso.sh` (cycle
  60 → 90 → 120 → 165 → illimité). Le menu du pad ne l'utilise plus :
  `mangohud.sh fps` fait le même travail avec plus de crans.
- `showhidemango` : même raison, conservé pour l'alias.

Tout le reste (menus, captures, enregistrement, sons) appartient au projet
`src/gablue-pad/` et s'installe dans `/usr/libexec/gablue-pad/`.

### Configuration tuned (/usr/lib/tuned)

Profils Gablue : `balanced-gablue`, `balanced-battery-gablue`, `throughput-performance-gablue`, `powersave-gablue`, `powersave-battery-gablue`

### Moteur d'installation des AppImages (`/usr/libexec/gablue-apps`)

Source de vérité unique pour les applications AppImage : **catalogue déclaratif
`/usr/share/ublue-os/gablue/apps.toml`** (groupes, noms, motifs, dépôts, règles
de version, hooks `.desktop`), lu par le moteur **et** par Gablue Manager
(`src/gablue-manager/gablue_manager/apps.py`) — une application nouvelle se
décrit à un seul endroit. Le moteur fait : résolution de la dernière release
(flux Atom GitHub/GitLab, redirection de `/releases/latest`, API Gitea, registre
npm ; **jamais `api.github.com`**), téléchargement avec **reprise** (cache par
tag : `~/.cache/<clé>/<tag>/<asset>` + `.part`), **tentatives** et **anti-cale**
(abandon sous 200 Kio/s pendant 30 s, lecture muette > 20 s), contrôle ELF,
retrait de l'ancienne AppImage (GearLever, repli manuel), intégration GearLever,
patch `.desktop`, nettoyage du cache, `kbuildsycoca6`.

- CLI : `gablue-apps list` ; `probe <clé> [--json]` (dernière version publiée —
  c'est ce que le manager appelle, il ne recalcule plus les versions) ;
  `install <clé> [--dry-run] [--yes]` (`--dry-run` : télécharge et vérifie sans
  toucher à GearLever ; `--yes` accepté pour compatibilité, le moteur ne pose
  jamais de question).
- `azahar-install`, `eden-install`, `lichii-install`, `xenia-install` sont des
  coquilles de 3 lignes ; `esde-install` et `opencode2-install` gardent leur
  logique (ES-DE copie `custom_systems`, OpenCode touche npm/Dolphin) mais
  téléchargent par `gablue-fetch`.
- **Progression** : hors terminal, le moteur écrit `::gablue:busy`,
  `::gablue:percent <0-100> <libellé>` (~2/s pendant le transfert) et
  `::gablue:done` ; `local.parse_gablue_progress` en tire la barre du dialogue,
  avec un **pourcentage réel** — ce que `curl` ne savait pas donner (fenêtres
  figées à « 0 % » constatées le 08/11 et le 09/11/2026). Sur un terminal, aucun
  marqueur : une jauge `\r` lisible.
- `gablue-fetch <url> <dest> [libellé]` : même politique réseau en bash pour les
  scripts restés tels quels ; **sans reprise** (destination au nom fixe, une
  reprise collerait deux builds) — la reprise n'existe que dans le moteur, grâce
  au dossier de cache par tag. Les appelants l'invoquent par **chemin absolu**
  `/usr/libexec/gablue-fetch` : `/usr/libexec` n'est pas dans le `PATH` de
  l'utilisateur (vécu 10/11/2026 : « gablue-fetch: commande introuvable » cassait
  `opencode2-install` et `esde-install`).
- Vécu 09/11/2026 : la même URL d'asset GitHub a été servie à 1,1 s (38 Mio/s)
  puis à 59,7 s (2,3 Mio/s) selon la connexion, et une connexion morte ne
  rendait plus rien — `curl` sans borne attend indéfiniment. Le moteur rend la
  main en 4 essais et garde le partiel pour la fois suivante.

### Helpers privilégiés (/usr/libexec)

Tous appelés en `sudo -n` par Gablue Manager (règles NOPASSWD exactes) et fermés sur une liste :
`gablue-kargs` (kargs connus seulement), `gablue-cpuid-emu on|off|status`, `gablue-tailscale`,
`gablue-service <displaylink.service|docker.service> on|off|status`, `gablue-low-latency on|off`
(écrit `/etc/profile.d/vulkan-low-latency.sh`), `gablue-avahi on|off|status` (écrit ou retire
`/etc/avahi/services/ssh.service`, aucun chemin en argument), `gablue-broadcom-wl on|off` (écrit ou retire
`/etc/modprobe.d/wl-enable.conf` : `install wl` + blacklist `b43`/`bcma`/`brcmfmac`, redémarrage
requis), `gablue-wifi-powersave on|off|status` (drop-in
`/etc/NetworkManager/conf.d/99-gablue-wifi-powersave.conf` : `wifi.powersave` 2/3, appliqué à chaud par
`iw` et à chaque activation par NM — la veille radio Wi-Fi faisait des pics RTT à 197 ms et 40 ms de gigue
sur gen1, mesuré 13/11/2026 : 2,2 ms de moyenne une fois coupée, c'est ce qui faisait saccader le son de
gremote), `gablue-rebase <variante>` (`rpm-ostree rebase` vers
`ostree-image-signed:docker://ghcr.io/elgabo86/gablue-<variante>:latest`, variantes publiées
seulement, effet au redémarrage) et `gablue-auto-update` (`status` ;
`image|flatpak|brew on|off` ; `schedule image|flatpak|brew daily|weekly` (fréquence propre à chaque domaine) — timers
`rpm-ostreed-automatic.timer`, `flatpak-system-update.timer` et les deux de brew,
livrés **désactivés** par l'image : les mises à jour automatiques sont un opt-in
depuis Gablue Manager ; le timer Flatpak de la session reste à l'utilisateur, le
manager le bascule sans privilège et lui écrit son calendrier), plus `gablue-update` (aucun argument) : il lance `bootc upgrade
--progress-fd 3` avec le fd 3 sur `/run/gablue/update-progress.fifo` (FIFO 0644 créé par le
helper), parce que sudo ferme les fd ≥ 3 — l'app ne peut donc pas lui passer un descripteur, et
Gablue Manager lit le FIFO sans privilège pour sa barre de progression (bootc exige un tube :
son écriture passe par epoll, un fichier ordinaire est refusé ; le FIFO fait 64 Kio, donc un
lecteur doit suivre, et le helper le retire à sa sortie (`trap EXIT`) pour qu'une mise à jour
annulée ne laisse pas un tube mort que la suivante relirait ; bootc tourne en arrière-plan et
`TERM`/`INT`/`HUP` le tuent lui-même (TERM puis KILL) — sinon « Annuler » ne coupait que sudo
et le helper quand sudo isole la commande dans son propre pty, et le téléchargement continuait
(vécu 08/11/2026). `gablue-hwinfo` (aucun argument)
rend en JSON les barrettes mémoire (`dmidecode -t memory`, root) **et** la température des
disques SATA (`smartctl -n standby,3 -d sat -j -H -A`, root — la température **et** le verdict
SMART `smart_status.passed`, une seule commande ATA) pour le résumé matériel du manager —
emplacements vides écartés, erreur de lecture → liste vide, jamais d'échec visible. Les disques
sont rendus **tous**, y compris ceux en veille (`celsius: null`) : c'est ce qui garde la
numérotation stable d'une lecture à l'autre. `-n standby` fait sortir smartctl **avant** toute
lecture SMART sur un disque endormi (seul `CHECK POWER MODE` est envoyé, sans plateaux en
rotation) et `-d sat` coupe l'autodétection de type, dont le manuel de smartctl prévient qu'elle
peut réveiller le disque — **jamais** de `drivetemp` pour ça : ce pilote lit le SMART sans
regarder le mode d'alimentation. La température est lue dans `raw.string` (« 25 (0 17 0 0 0) »),
pas dans `raw.value` qui vaut l'entier 48 bits complet sur certains disques (ST4000VN008). `gablue-clean` traite une
sous-commande (`flatpak` : Flatpaks système inutilisés).
`gablue-btrfs-compress` pose ou retire le drapeau `compression=zstd` sur un montage Btrfs
(chemin revérifié contre `findmnt -t btrfs`) : il vaut pour les fichiers créés ensuite.
`gablue-nvme-thermal` lit et écrit HCTM (`set-feature -f 0x10 -s`) : `status` rend un JSON
(modèle, `mntmt`, seuils courants), `set`/`off` valident bornes et plages avant d'écrire ;
lecture comme écriture exigent root sur nvme-cli. `gablue-grub` expose `status`, `set 0|1|2`
et `unset` : `grubenv` est en 0700 root (même la lecture passe par root), et le helper
remonte `/boot` en écriture puis restaure son état (`mount`/`findmnt`). `gablue-bootnext` expose `list` (options `id<TAB>libellé<TAB>détail`, système principal exclu par le disque de `/boot/efi` ; les ESP sans entrée EFI sont montées en lecture seule pour trouver un chargeur shim/Windows/BOOTX64) et `boot <id>` : il revérifie l'identifiant (4 hexadécimaux existants, ou partition typée ESP), refuse le disque courant, crée au besoin l'entrée NVRAM puis restaure le BootOrder, arme `BootNext` et redémarre — le firmware le consomme une fois. UEFI uniquement : un démarrage hérité (MBR/CSM) n'est jamais proposé.
`gablue-secureboot`
expose `status` (JSON : EFI, Secure Boot, état de chaque certificat) et `enroll` : il pose la
demande MOK (`mokutil --timeout -1` puis `--import` des deux `.der` de
`/etc/pki/akmods/certs`, dossier 0700 root) avec le mot de passe public `gablue` de l'ISO ;
l'effet est au démarrage suivant, dans MokManager. `gablue-ryzenadj` expose `status` (JSON :
support, limite, état de l'unité), `set <n>` (45…110 °C : écrit la configuration puis active
l'unité) et `off` (retire la configuration et l'unité) ; `apply` n'est appelé que par
`gablue-ryzenadj.service` (oneshot, `ConditionPathExists=/etc/gablue/ryzenadj.conf`, démarré
par `enable --now` à la première pose) — ryzenadj perd sa limite au reboot, l'unité la
réapplique avant toute session, ce que l'autostart de `limitw --save` ne garantit pas.
Le CPU est reconnu au **fabricant** (`AuthenticAMD`), pas au mot « Ryzen » : l'échantillon
d'ingénierie de gen1 annonce « AMD Eng Sample » et refusait la limite (constaté 15/11/2026,
`ryzenadj -i` y lit pourtant la table du SMU) ; un `-f` refusé par le matériel remonte dans le
journal de la tâche, jamais masqué.
Aucun de ces helpers n'accepte d'argument
libre sans revérification : c'est la contrepartie du NOPASSWD. Scripts stockés 644, `+x` posé
par post-install.

### Just commands (/usr/share/ublue-os/just/)

- **Système** : `configure-grub`, `kernel-setup`, `mitigations-on/off`
- **Réseau** : `tailscale-up`, `ssh-on/off`, `toggle-wol`
- **GPU** : `toggle-i915-sleep-fix`, `configure-amd-hdmi21` (karg `amdgpu.dcfeaturemask=0x402` — HDMI 2.1 ; Bazzite `f92d411`, renommé de `configure-amd-vrr` par Bazzite `150cf40e`)
- **Gaming** : `scx-enable/disable`, `cpuid-fix-on/off`, `cpuid-emu-on/off` (persistant via `/etc/modprobe.d` + `/etc/modules-load.d`, blacklist kvm_amd)
- **Virtualisation** : `docker-enable/disable`, `dx-group`, `setup-kvmfr`, `libvirt-reset-cache` (cache capabilities libvirt, corrige « video model 'virtio' unsupported » dans virt-manager)
- **Maintenance** : `brew-reset`, `pyenv-remove`, `snapshots-enable/disable`, `btrfs-compress`, `btrfs-compress-defrag`, `ssd-thermal-limit` (HCTM NVMe, interactif, persistant), `toggle-updates-all` (système + flatpaks + brew — contrairement à `toggle-updates` upstream sans brew)
- **Affichage** : `kwin-display-reset` (met de côté avec horodatage `~/.config/kwinoutputconfig.json` + `/var/lib/plasmalogin/.config/kwinoutputconfig.json` — dépannage écran noir / hors portée au login). Règles VRR par application : livrées dans `/etc/xdg/kwinrulesrc`, appliquées seules par KWin 6.8 qui complète `[General] Order` avec tous les groupes présents (défauts système inclus, lus même en `NoGlobals`). L'ex-`vrr-fix` est retiré : il écrivait la clé legacy `[General] rules`, que KWin 6.8 traite comme liste exclusive et qui supprime donc les règles de fenêtre de l'utilisateur ; le nécessaire sur Plasma 6.7 (qui n'énumérait que par `rules`) est couvert par la montée en 6.8. **`wmclass` = ce que KWin voit réellement**, pas le nom du paquet Flatpak : une app Wayland native donne son *app id* (`org.kde.dolphin`), une app XWayland son *WM_CLASS* (`vlc` — le VLC 3 flatpak tourne sous X11, d'où un scintillement plein écran resté des mois avec une règle visant `org.videolan.VLC`, mesuré 21/11/2026 ; règle `vlc` posée + second groupe `vlc-wayland-vrr` pour un futur VLC 4). Vérification avant d'ajouter une app, deux mesures et pas de devinette : `WAYLAND_DEBUG=1 <app> 2>&1 | grep set_app_id` pour une app Wayland, `xprop -id <fenêtre> WM_CLASS` (fenêtre listée dans `_NET_CLIENT_LIST`) si elle tourne sous XWayland — une app qui n'émet aucun `set_app_id` est XWayland. Audit du 21/11/2026 sur cette base : `vlc`→`vlc`, `vacuumtube`→`rocks.shy.VacuumTube`, `Rustdesk` (l'app id `com.rustdesk.RustDesk` gardé dans `rustdesk-wayland-vrr`), `qemu-system-`→`qemu` (app id du display GTK), `cool-retro-term` retirée (app absente) ; mesurées correctes déjà : `mpv`, `org.kde.dolphin`, `org.kde.konsole`, `gwenview`, `app.zen_browser.zen`, `org.telegram.desktop`, `io.freetubeapp.FreeTube`, `org.jellyfin.JellyfinDesktop`, `virt-manager`, `virt-viewer`, `gablue-tv` (`tvqt`), `com.moonlight_stream.Moonlight`. La source de vérité est **ce que KWin résout**, jamais le seul côté client : `glauncher` envoie `set_app_id("")` et KWin le voit pourtant en `resourceClass=glauncher` (dump KWin 21/11/2026, fenêtre plein écran mesurée) — sa règle marche, une trace `WAYLAND_DEBUG` seule aurait fait conclure l'inverse. Une app douteuse se tranche donc par un dump des fenêtres vues par KWin (script `workspace.windowList()` → `resourceClass`, dépose du résultat sur un service D-Bus jetable ou dans le presse-papier via `org.kde.klipper`), `xprop` ne valant que pour XWayland.
- **Rebase** : `gablue-rebase-*` pour changer de variante (le manager le fait par `/usr/libexec/gablue-rebase`, sans passer par ujust)

### Justfile (60-custom.just) — conventions d'écriture

**Format** :
```just
# Description de la commande
command-name:
    #!/usr/bin/bash
    echo "Hello World"
```

**Conventions** : kebab-case, shebang obligatoire, description sur une ligne avant la commande, indentation 4 espaces

**Gestion des subvolumes BTRFS** :
- `btrfs filesystem defrag -r` ne traverse pas les limites de subvolumes → `findmnt -t btrfs` filtré par UUID pour lister les points de montage individuels
- Fallback (disques externes, subvolumes non montés séparément) : `sudo btrfs subvolume list` + reconstruction des chemins
- Exclusions communes : `.beeshome` (BEES), `root*` (ostree système, reflinks), `*.snapshots` (snapper, reflinks)
- `btrfs-compress-defrag` exclut en plus `/var` et `var*` (risque reflinks Docker/Podman) — `btrfs-compress` (property set, safe) ne les exclut pas
- Filesystems par label (`findmnt -o TARGET,UUID,LABEL`) quand disponible ; parsing compression via `cut -d= -f2` (`btrfs property get` renvoie `compression=valeur`) ; `mapfile -t` pour lire les subvolumes

**Complétion bash** :
- `files/system/all/usr/share/bash-completion/completions/ujust` : surcharge la complétion buggy du paquet `ublue-os-just` (n'enregistrait jamais la complétion pour `ujust`) ; génère la liste via `ujust --summary`
