# Fichiers système (files/system/) — gablue

> Sous-document du AGENTS.md racine. Couvre files/system/ : configs /etc,
> binfmt, scripts et binaires /usr/bin, widget plasmoid, tuned, recettes ujust.
> Architecture interne : src/gamepadshortcuts/AGENTS.md (gamepadshortcuts +
> kbdnav), src/gwine-launcher/AGENTS.md (gwine).
> Même règle que la racine : à jour avant tout commit touchant files/system/
> (les générateurs — ex. post-install — sont dans files/scripts/AGENTS.md).

## Configurations système (/etc)

- **distrobox/distrobox.conf** : Configuration Distrobox
- **firewalld/zones/nm-shared.xml** : Zone firewall partagée
- **xdg/plasmakeyboardrc** : Presets clavier virtuel KDE (navigation clavier requise par kbdnav, layout fr_FR, clavier centré)
- **profile.d/customperso.sh** : Alias et personnalisations shell
- **security/limits.d/memlock.conf** : Limites mémoire
- **skel/.config/gtk-4.0/** : Configuration GTK par défaut
- **sudoers.d/nopasswd** : Sudo sans mot de passe
- **systemd/** : Timeouts et configuration systemd
- **yum.repos.d/docker-ce.repo** : Dépôt Docker

## Exécution des binaires Windows (binfmt_misc)

- **usr/lib/binfmt.d/gablue-windows.conf** : règle binfmt_misc (magic `MZ` → `/usr/bin/gwine`), chargée au boot par `systemd-binfmt.service` (statique, présence du fichier suffit)
- Permet `./jeu.exe` en terminal et le bouton « Exécuter » de Dolphin/KIO pour les `.exe` avec bit `+x` (copies NTFS/exFAT, archives)
- Complément du patch runner `no_exe_executable_bit.mypatch` (dépôt `elgabo86/gwine`) : le wineserver gwine ne pose plus `+x` sur les `.exe`/`.com` créés par les installateurs → KIO les ouvre nativement via Windows.desktop → gwine (icônes + « Ouvrir avec » intacts). KIO ne traite un `.exe` comme binaire natif QUE s'il a `+x` (hardcodé, sans option)

## Scripts utilisateur (/usr/bin)

- `gablue-apk-installer` : installateur APK par glisser-déposer (GUI PySide6, Wayland natif). .apk/.apkm/.xapk/.apks ; détection USB + WiFi (adb over TCP) ; appairage sans fil Android 11+ par QR manuel ; install avec remplacement/downgrade (`-r -d`) ; bundles extraits via `adb install-multiple` ; `.desktop` dans `/usr/share/applications/`
- `gablue-update` : maj système via bootc (Python autonome, GUI PySide6 + PTY progression temps réel, fallback CLI sans affichage)
- `gablue-bigscreen-swap-session` / `gablue-bigscreen-session-init` : swap-session Bigscreen + init session native (détail → files/scripts/AGENTS.md § post-install)
- `retroplayer` : TUI Go musiques rétro (GitHub Releases pendant le build, dépôt séparé)
- Scripts gaming : `azahar-install`, `eden-install`, `esde-install`, `shadps4-install`, `xenia-install`
- Scripts utilitaires : `dlv`, `dlcover`, `tv`, `tvqt`, `ventoy`, `wallpaper-import`, `clean-media`
- Gestion Wine/Proton : `gwine` (assemblé depuis `src/gwine-launcher/`), `scrap-win`
- `konsole-run` : wrapper « Lancer dans Konsole » (détail → § konsole-run ci-dessous)

### opencode2-install

Installe/met à jour OpenCode 2 (`@opencode/cli` npm global, symlink `~/.local/bin/opencode`), l'app OpenCode Desktop, les entrées Dolphin « Ouvrir avec » et le MCP Lightpanda (remplace les anciens scripts standalone, supprimés ; chrome-devtools retiré 10/2026 : ~490 Mo + 155 Mo RAM au repos pour un cas rare — lightpanda couvre lecture/extraction/interaction, Cloudflare actif → renoncer).

- **allow-scripts persistante additive** (`ensure_allow_scripts`, 10/2026) : tarball npm = stub 229 o, vrai binaire fabriqué par le `postinstall` — npm ≥ 11.19 bloque sans allowlist. L'updater interne (`opencode upgrade`) réinstalle SANS `--allow-scripts` → sans allowlist persistante, MAJ silencieusement cassée (stub jamais remplacé). `npm config set allow-scripts=X` REMPLACE la valeur entière (entrées écrasées mutuellement, constaté) → sed sur la ligne `allow-scripts=` de `~/.npmrc` (pas `npm config get`, un .npmrc projet prioritaire au cwd fausserait) + ajout, idempotent ; appelé dans tous les cas (même skip MAJ)
- **Spec `@latest` explicite sur tous les npm install** (10/2026) : sans spec, npm réifie l'existant sans monter de version (« changed N packages » trompeur)
- **Skip MAJ (étape 2bis)** : CLI + Desktop + Lightpanda à jour ET artefacts en place (symlink, AppImage, .desktop patché, entrées Dolphin, binaire Lightpanda, entrée lightpanda dans opencode.json, allowlist complète) → sortie immédiate. Réseau/version amont indéterminable → poursuite complète (fail-safe : répare ce qui manque) ; artefact absent/manquant → poursuite. Étape 4 : skip `npm install -g` si CLI déjà à jour ; étape 6 : skip download Desktop (préexistant). `--force` : bypass de tous les skips, argument inconnu refusé (usage)
- **Desktop via GearLever** (pattern `eden-install`) : `~/AppImages/opencode.appimage` + `.desktop`. Source `https://opencode.ai/download/stable/linux-x64-appimage` (redirection → assets v2 sur opencode.ai ; releases GitHub restent v1.18.x). Versions appariées npm (ex. 2.0.16 des deux côtés) : cible extraite de la redirection (fallback `npm view`), installée lue dans `X-AppImage-Version` — skip si identiques ; sinon curl fail-fast AVANT tout retrait, `gearlever --remove` (+ fallback manuel), réintégration, patch `.desktop` : `Name=OpenCode` sans suffixe + `--no-sandbox` (chrome-sandbox SUID absent sur Kinoite, comme lmstudio). Icône `$HOME/AppImages/.icons/opencode`
  - **Re-détection .desktop APRÈS intégration** (fix 25/09) : détection avant intégration → `DESKTOP_ENTRY` vide → patch jamais appliqué à la 1re install. Détection par grep après `gearlever --integrate`, patch dans `patch_desktop_entry` (idempotent), aussi appliqué quand déjà à jour (répare les installs non patchées)
- **Lightpanda** (étape 5bis) : `@lightpanda/browser` npm global — postinstall télécharge le binaire nightly (`~/.cache/lightpanda-node/lightpanda`), allow-scripts requis sinon binaire jamais téléchargé, MCP muet ; skip si version = dernière ET binaire exécutable ; maj ponctuelle `lightpanda upgrade`
- **Activation config** (étape 5ter) : entrée `mcp.servers.lightpanda` dans `~/.config/opencode/opencode.json` si absente (python stdlib, merge idempotent — serveurs/flags préservés, entrée `disabled` = no-op). JSONC illisible → warning + poursuite (binaires installés, non branchés). Présence = condition de sortie 2bis (réparée au run suivant si retirée à la main). Coût ~180 Mo + ~21 Mo RAM
- **`MCP_UPDATED`** : 1 si Lightpanda installé/maj OU entrée créée → étend la condition de redémarrage du service de fond ; échecs install non fatals (warning + poursuite) ; `--force` réinstalle aussi Lightpanda

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
- **Ordre « Ouvrir avec »** : position dans `[Added Associations]` de `~/.config/mimeapps.list` (1er listé = en haut ; `InitialPreference` 10 GUI/5 CLI = repli). Installateur écrit `inode/directory=gablue-opencode.desktop;gablue-opencode-cli.desktop;` en tête de section (idempotent, sections dupliquées/vides purgées). JAMAIS `[Default Applications]` : le double-clic d'un dossier doit rester Dolphin

### Scripts gaming et utilitaires

- `esde-install` : ES-DE (AppImage GitLab + GearLever), copie `custom_systems` → `~/ES-DE/`, lanceurs Tools (Gablue TV fullscreen, Jellyfin Desktop mode TV, YouTube VacuumTube) dans `<ROMPATH>/tools/` (ROMPath lu dans `es_settings.xml`, défaut `~/Roms` ; lanceurs existants non écrasés). Réinstallation refusée → fichiers Gablue copiés QUAND MÊME (fonction partagée). Nouvelle AppImage téléchargée et vérifiée AVANT suppression de l'ancienne. Sources : `/usr/share/ublue-os/gablue/esde/tools/`
- `wgpadd` / `lgpadd` : ajout en masse de packs ES-DE. `wgpadd` : `.wgp` → `--windows` défaut (lanceur `.sh` dans `.es-wgp` + lien `~/Roms/windows`), `--xbox360` (lien direct, système natif) ; `lgpadd` : `--desktop` défaut / `--switch` (lien direct, natif). Sans argument = tous les packs du dossier, avec fichiers = uniquement ceux-là ; cover téléchargée si absente
- `lgplaunch` : lancement `.lgp` (squashfs squashfuse, saves/extras/temp via symlinks `/tmp/lgp-*` + overlays kernel dans `unshare -U -m` ; amorçage par item — absent copié du pack, présent intact, jamais écrasé : un pack re-classé se répare au lancement ; même logique dans gwine `lib/wgp/symlinks.sh`) et exécutables Linux directs (ELF/.sh/.py/AppImage). `--exe` : sélection interactive d'un exécutable (menu kdialog/console, scan ELF/.sh/.py/.AppImage + symlinks, surcharge `.launch`), déclenché par l'action `EditLGP.desktop`. Symlink compat `launchlin.sh` conservé (raccourcis `.desktop` anciens en dur) ; `killthemall` matche les deux noms. Handlers MIME `LGP.desktop`/`LGPTerminal.desktop` (NoDisplay) pour `application/x-lgp` ; commandes ES-DE `desktop`/`switch`
- `limitw` : limites TDP CPU/APU AMD via ryzenadj (bash, sudoers nopasswd). `--temp` temp max, `--save`/`--reset` persistance autostart, `--info` (non lisible sur certains desktop — Raphael/Dragon Range, pas de chemin PM table)
- `dlv` : téléchargeur YouTube unifié (playlist : `--mp3`, `--mp4`, `--mkv`, `--mkv-1080`). Remplace `dlv-mp*`/`ytdl` ; completion bash `/usr/share/bash-completion/completions/dlv`

### konsole-run — action « Lancer dans Konsole » (Dolphin)

Wrapper `usr/bin/konsole-run` + service menu `usr/share/kio/servicemenus/konsolerun.desktop` qui REMPLACE celui du paquet konsole (même nom de fichier, le COPY files/system/all écrase ; MimeType étendu à `application/x-shellscript` — l'original ne couvre que `x-executable`) :

- Affiche chemin + nom au lancement, exécution DIRECTE (shebang respecté), shell interactif à la fin (`exec bash`) — le `--hold` natif laissait une session morte non saisissable
- Script sans +x : `+x` ajouté le temps de l'exécution puis permissions restaurées à l'identique (trap EXIT/INT/TERM/HUP — survit à Ctrl+C/fermeture de fenêtre ; bits spéciaux inclus via `stat -c %a`) ; chmod impossible → erreur claire, rien n'est exécuté
- chmod +x posé par post-install (convention repo)
- **Double-clic NON couvert** : KIO (`OpenUrlJob::handleScripts`, ex-KRun) exécute les +x en direct sans consulter l'association MIME ; 3 issues codées en dur (`kiorc` `[Executable scripts] behaviourOnLaunch` = execute/alwaysAsk/open) — aucune ne lance Konsole. Vérifié test bac à sable + code KIO 10/2026
- Validé local 10/2026 (perms 644 → 755 pendant → 644 après, interruption restaurée) avant intégration

### Binaire gamepadshortcuts (/usr/bin)

Gestionnaire principal des raccourcis manette en C natif (~500 Ko RAM) :
- Détection manette via `/dev/input/event*` (evdev, ioctl) ; multi-session Wayland : suivi VT via inotify `/sys/class/tty/tty0/active` — une instance par session (autostart KDE), événements filtrés si VT inactif (pas de conflit entre sessions), reprise au retour
- `gamepadshortcuts-mouse` : émulation souris/clavier (Home+R3), binaire C (evdev + uinput) remplaçant `mouse.py` (plantait sous Python 3.14 : distutils supprimé). Mapping : stick droit = souris (courbe FPS), R1/L1 = clics, D-pad = flèches, Croix/Rond/Carré/Triangle = Espace/Tab/Retour/F4, Start/Select = Entrée/F11, L3 = Échap, L2/R2 = Alt/Shift
- Mode clavier virtuel : Home+Carré lance `kbdnav` en lui transmettant le chemin de la manette suivie (le clavier répond à LA manette qui a déclenché la combo — le re-scan interne de kbdnav peut attraper l'autre pad avec plusieurs manettes) ; exclusivité mutuelle — tue la souris ; Home+R3 inverse. Pendant le grab, gamepadshortcuts est aveugle : le pont signale sa sortie Home+R3 via SIGUSR1 (`pending_mouse_launch`, lancement différé en boucle) ; `check_kbdnav()` récolte + `reset_button_states()` (releases passées pendant le grab = combo résiduelle sinon)
- Architecture complète → `src/gamepadshortcuts/AGENTS.md`

### Binaire gablue-isomount (/usr/bin)

Monteur d'images disque en C natif (~2,7 Mo RAM), remplace le plugin dolphin-plugins mountisoaction (bug KDE #471487) :
- Monte ISO/IMG/EFI via UDisks2 DBus (LoopSetup + Filesystem.Mount) ; ouvre une fenêtre Dolphin (panneau Devices à jour)
- Réinjecte le LD_PRELOAD composefs-fix avant son `execlp("dolphin")` (ne passe pas par le .desktop patché — voir files/scripts/AGENTS.md § composefs)
- Démontage auto quand toutes les instances Dolphin sont fermées ; device occupé → attend sa libération ; déjà montée → nouvelle fenêtre sans remonter
- Service menu « Monter » + app par défaut MIME ISO/IMG/EFI ; log `/tmp/gablue-isomount.log`

### Binaire kbdnav (/usr/bin)

Pont manette → clavier virtuel Plasma Keyboard en C natif (~25 Ko). Source `src/gamepadshortcuts/kbdnav.c`, même Makefile que gamepadshortcuts :
- D-pad/stick = flèches, A = Entrée (appui court valide, maintien ≥ 600 ms = popup d'accents — flèches + Entrée, Échap ferme), B = Échap, Carré = Backspace, Triangle = Espace, L1/R1 = Tab, Start = fermer
- Cible `plasma-keyboard` (IM KDE, défaut Kinoite 44) via son KCM « Keyboard navigation » — touches injectées uinput, captées par le grab de l'IM (jamais re-transmises aux apps pendant la frappe)
- Cycle de vie auto-porté, lancé par Home+Carré : affichage clavier (KWin AnyInput + forceActivate) → frappe → sortie = masquage (setMode KWin `Never` puis `NonMouseInput` — le défaut Kinoite, pour que kwinrc ne persiste pas `Never` + kill -9 plasma-keyboard pour empêcher les apps de ré-afficher le panneau fermé)
- Grab evdev exclusif (EVIOCGRAB) pendant la frappe (limite assumée : lecteurs hidraw SDL/Steam non coupables — aucun mécanisme noyau) ; quarantaine (poll D-Bus 250 ms) : touches injectées seulement si IM active ET panneau visible ; auto-sortie quand le clavier se ferme (active && !visible 0,5 s) ou Start/Home+Carré/Home+R3 (→ souris, avec SIGUSR1)
- **Manette imposée par le parent** (argv `/dev/input/eventX` = device suivi par gamepadshortcuts) : grab + frappe sur LA manette qui a ouvert la combo ; device disparu → repli scan interne ; `--no-grab`/`--find` conservés
- VT tracking (inotify, pattern gamepadshortcuts) ; presets : `/etc/xdg/plasmakeyboardrc` (keyboardNavigationEnabled=true, enabledLocales=fr_FR — pas de fr_CH dans plasma-keyboard, repli fr_CA ; layout fr_CH custom QML = v2 ; panelFillScreenWidth=false — centré max 3:1, hauteur hardcodée 30 % Breeze)
- Architecture complète → `src/gamepadshortcuts/AGENTS.md`

### Interface tvqt (/usr/bin)

TV Gablue en Python (PySide6 + libmpv) : ~170 chaînes, navigation manette D-pad, HLS via `libmpv` embarqué, logos en cache, filtre par pays (pastilles), accélération progressive au maintien.
- **Multi-manettes** : TOUTES les manettes connectées pilotent l'app (dict devices + état par device, fronts boutons par device, direction fusionnée up>down>left>right) ; re-scan incrémental 2 s (hotplug, renumérotation des nodes après reconnexion BT), device retiré sur erreur lecture. Axes normalisés par device via `absinfo` — les pads virtuels ds2xbox (-32768..32767, repos 0) étaient lus collés haut/gauche (volume qui montait en boucle en lecture). Entrée/Espace clavier = lecture directe de la chaîne sous le curseur (cartes = QFrame, pas de `.click()`)
- **Focus Wayland** : evdev lit les manettes même sans focus → suivi activation via `changeEvent(QEvent.ActivationChange)` (fiable sous Wayland : événements du compositor KWin). Fenêtre active → manettes OK ; autre app au premier plan → ignorées
- **Lecteur intégré** (refonte 2026) : `libmpv` + `QOpenGLWidget` (API `mpv_render_context`) — **Python 3.14 : `c_void_p` retourne un int → wrapper explicite `c_void_p(handle)` après `mpv_create()`, sinon segfault (handle passé en 32-bit)**. Plus de fenêtre/sous-process mpv externe. [A] lance/stop, [B] grille, D-pad = volume/seek ; fullscreen auto au lancement d'une chaîne (double-clic = toggle, clic droit = grille) ; GUI masquée en vidéo (barre + OSD cachés) ; curseur auto-masqué après 2,5 s (`CURSOR_HIDE_MS`, tout mouvement le réaffiche, restauré à la grille)
- **Guide TV / EPG — données** (10/2026) : source xmltvfr.fr (`xmltv.xml.gz` ~150 Mo décompressés) parse SAX streaming ~2 s, thread + signal `_epg_ready` (jamais de parse sur le thread GUI). Mapping **statique compact** `EPG_MAP_RAW` (80 paires `xmltv:appli`, repris de l'appli Android + CARAC4/Canal9 vérifiées) : ids netplus irréguliers (`CStar.fr`→`d17`, `Trek.fr`→`escales`…) → auto-match refusé (faux positifs mesurés : `Sat1.de`→`sat1gold`, `RTLNitroTV.de`→`rtl`, `BLUEZOOM.ch`→`bluezoomde`) ; ~75 % des paires seraient dérivables mais choix user = statique (scan des nouvelles chaînes à la demande). Caches `~/.cache/gablue-tv/` : `epg.xml.gz` (brut, écriture atomique .part+rename) + `epg_parsed.tsv` (TTL 12 h, format identique au cache Android → reprise instantanée + maj en fond). Titre « en cours » sous les cartes (cyan `#00bcd4`, elidé, tooltip now/next) : maj **incrémentale** via `_card_epg_state` (seules les cartes dont le programme a changé — l'appel était dans la boucle de `_rebuild_grid` → O(n²), corrigé). Chaînes sans id xmltvfr (Italie, Autriche, une partie DE/UK/Est) = pas de guide, compteur `N/M` dans le statut
- **Guide TV — rendu virtualisé** (10/2026) : le guide complet = ~90 widgets/chaîne × 79 ≈ 7000 widgets → transitions 1,2 s (« rien ne se passe » au [Start]). Depuis : **virtualisation** — métadonnées (`_guide_data`) séparées des widgets (`_guide_row_widgets`), seules les lignes visibles ±4 (`GUIDE_BUFFER_ROWS`) sont construites, géométrie manuelle (plus de QScrollArea par ligne : clip + blocs repositionnés par `_guide_set_offset`, molette horizontale/Maj = décale les blocs, molette verticale = défile + suit la sélection) → ouverture ~35 ms, 11 lignes construites sur 79. **Dirty flags** `_grid_dirty`/`_guide_dirty` + `_rebuild_visible_page` : une seule page reconstruite, jamais au toggle si rien n'a changé (mêmes objets carte préservés) ; `_sync_pills_for_view` ne refiltre que si le pays effectif change (`_active_country`). Flip de programme : rebuild seulement si le guide est visible, ligne restaurée par ch_id **et bloc par position** (le focus reste sur le programme choisi). Logos multi-cibles + **cache mémoire** `_logo_pixmem` (plus de relecture PNG à chaque rebuild) ; polish QSS seulement si la propriété change. Navigation : [A]/Entrée/clic = lancer la chaîne depuis **n'importe quel bloc** (en cours ou à venir — une live diffuse toujours son programme présent), [B] = retour à la vue d'origine (`_return_view` : lancé du guide → retour au guide, position + bloc conservés, `_epg_tick` au retour), D-pad via `_nav_step` (accélération conservée ; haut/bas → programme en cours de la nouvelle ligne, gauche/droite conserve le décalage), Home/End. Pastilles contextuelles : en guide, seuls les pays avec ≥ 1 chaîne EPG (sélection sans guide stashée/restaurée, cycle L1/R1 borné à la liste active). OSD contextuel grille/guide via `_show_view` centralisé (fix : OSD restauré au retour vidéo)

### Widget panel synthetic-quota (/usr/share/plasma/plasmoids)

`org.gablue.synthetic.quota` (Plasma 6) — quota Synthetic dans le panel, équivalent du plugin TUI OpenCode :
- Compact : `TOK 96.52% · REQ 99.87%` (code couleur Kirigami : positif > 50 %, neutre > 20 %, négatif ≤ 20 %) ; panel vertical = % seul ; étendu (clic) : barres tokens hebdo + requêtes 5 h, crédits, dates de régénération, badge limite
- Helper Python `contents/code/gablue-synthetic-quota-helper` : lit `~/.local/share/opencode/opencode.db` (table `credential`, `integration_id='synthetic'`, `active=1`, SQLite read-only WAL-friendly) puis `https://api.synthetic.new/v2/quotas` — pas de dépendance au serveur OpenCode, rotation de clé ramassée au poll suivant
- Cache `~/.cache/gablue/synthetic-quota.json` (tmp+rename) : échec réseau → dernières valeurs `stale=true` ; `no-key` → `TOK ?` + tooltip
- Data engine `P5Support.DataSource` (`executable`), poll 60 s (`main.xml` : `refreshInterval`, `decimals`, `showReq`) ; clé jamais en clair hors la db OpenCode
- **Plasma 6.7** : `PlasmaCore.Theme` n'existe plus (→ `Kirigami.Theme.defaultFont`) ; signal `newData` = `(sourceName, data)` (2 paramètres, pas 3) ; `Component` top-level interdits en QML (inline components dans le `PlasmoidItem`)
- Essai local : symlink `~/.local/share/plasma/plasmoids/org.gablue.synthetic.quota` → répertoire du repo + restart `plasma-plasmashell.service`

### Scripts gamepadshortcuts (/usr/share/ublue-os/gablue/scripts/gamepadshortcuts)

- `launchgamepadshortcuts` (lockfile par user), `menuvsr.py` (menu VR PySide6 + evdev), `decoblue` (déconnexion BT), `launchyt`, `openes` (EmulationStation), `takescreenshot`/`startstoprecord`, `changefps`/`showhidemango` (FPS / overlay MangoHud)
- `killthemall` : tue les émulateurs de la session. Tue aussi tvqt, Jellyfin (flatpak) et VacuumTube/YouTube (flatpak) UNIQUEMENT s'ils ont été lancés depuis ES-DE (`GABLUE_ES_LAUNCH=1` exporté par les lanceurs Tools). Pièges : Jellyfin — le wrapper `flatpak run` disparaît au lancement et l'env est filtrée des bwrap → détection `pgrep jellyfin-desktop` puis `flatpak kill` + pkill des bwrap restants (le sous-sandbox WebEngine survit au `flatpak kill`) ; VacuumTube — la variable n'est visible que dans le bash wrapper `startvacuumtube` → détection `pgrep startvacuumtube` puis `timeout 10 flatpak kill rocks.shy.VacuumTube` ; JAMAIS de kill direct des process du sandbox avant `flatpak kill` (orphelins, instance incohérente, `flatpak kill` suspendu sur D-Bus)

### Configuration tuned (/usr/lib/tuned)

Profils Gablue : `balanced-gablue`, `balanced-battery-gablue`, `throughput-performance-gablue`, `powersave-gablue`, `powersave-battery-gablue`

### Just commands (/usr/share/ublue-os/just/)

- **Système** : `configure-grub`, `kernel-setup`, `mitigations-on/off`
- **Réseau** : `tailscale-up`, `ssh-on/off`, `toggle-wol`
- **GPU** : `amd-corectrl-set-kargs`, `toggle-i915-sleep-fix`, `configure-amd-hdmi21` (karg `amdgpu.dcfeaturemask=0x402` — HDMI 2.1 ; Bazzite `f92d411`, renommé de `configure-amd-vrr` par Bazzite `150cf40e`)
- **Gaming** : `scx-enable/disable`, `cpuid-fix-on/off`, `cpuid-emu-on/off` (persistant via `/etc/modprobe.d` + `/etc/modules-load.d`, blacklist kvm_amd)
- **Virtualisation** : `docker-enable/disable`, `dx-group`, `setup-kvmfr`, `libvirt-reset-cache` (cache capabilities libvirt, corrige « video model 'virtio' unsupported » dans virt-manager)
- **Maintenance** : `gablue-update`, `brew-reset`, `pyenv-remove`, `snapshots-enable/disable`, `btrfs-compress`, `btrfs-compress-defrag`, `ssd-thermal-limit` (HCTM NVMe, interactif, persistant), `toggle-updates-all` (système + flatpaks + brew — contrairement à `toggle-updates` upstream sans brew)
- **Affichage** : `kwin-display-reset` (met de côté avec horodatage `~/.config/kwinoutputconfig.json` + `/var/lib/plasmalogin/.config/kwinoutputconfig.json` — dépannage écran noir / hors portée au login), `vrr-fix`
- **Rebase** : `gablue-rebase-*` pour changer de variante

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
