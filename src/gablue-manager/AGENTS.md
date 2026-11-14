# Gablue Manager — paquet `gablue_manager` (src/gablue-manager)

GUI PySide6 (Wayland natif) : liste des postes SSH avec état et latence, bureau distant (`gremote`), fichiers (`dolphin sftp://`), terminal SSH, arrêt / redémarrage / mise à jour des postes, recherche sur le réseau local, ajout + copie de clé — **et la machine courante** : serveur SSH, annonce locale, mises à jour (image Gablue, Flatpaks, gwine, Homebrew) et options avancées (ex-`gablue-config`), services et variante de l'image (ex-recettes ujust). Nom affiché « Gablue Manager » ; binaire et `.desktop` = `gablue-manager` / `gablue-manager.desktop` (convention `gablue-*`), **catégorie Système** (`Categories=System;`) et icône maison `gablue-manager` (`/usr/share/icons/hicolor/scalable/apps/gablue-manager.svg` : carré bleu Gablue, anneau en **engrenage à 8 dents** formant un G — ouverture en haut à droite et barre horizontale, le tout en une seule silhouette), avec repli `network-server` hors image via `uiutil.app_icon()`. Remplace l'ancien `gablue-ssh`, `/usr/libexec/gablue-config` (script kdialog supprimé, ainsi que `gablueconfig.desktop` et son entrée de blacklist) et `gablue-update` (script Python et `gablueupdate.desktop` supprimés, ainsi que son entrée de blacklist, la recette `ujust gablue-update`, l'alias `gablue-update` et la ligne d'aide correspondante) : les mises à jour se lancent depuis la carte du poste.

## Documentation de ce sous-projet

OpenCode ne charge automatiquement que la racine du dépôt et, en travaillant
dans un dossier, son `AGENTS.md`. Les documents ci-dessous ne sont **pas**
chargés seuls : les lire quand le déclencheur de la colonne de droite est vrai.

| Domaine | Document | « À lire AVANT de… » |
|---|---|---|
| Postes distants : registre, état/latence, clés et empreintes, adresses, découverte mDNS, bureau distant, réglages d'un poste | `AGENTS-hosts.md` | toucher `hosts.py`, `ssh.py`, `util.py`, `dialogs/host.py`, `dialogs/key.py` ou la liste des postes |
| Mises à jour : sondes, application locale, timers automatiques, chaîne distante, lot multi-postes | `AGENTS-updates.md` | toucher `updates.py`, `local_updates.py`, `remote_updates.py`, `UpdateCheckWorker` ou un bouton « Mettre à jour » |
| Poste local : page, sshd et annonce, noyau/CPUID, services, démarrage, disques et thermique, variante, gwine, Tailscale, Wi-Fi | `AGENTS-local.md` | toucher `local.py` ou un dialogue de réglages local |
| Applications (AppImages) : catalogue, moteur, install/maj/suppression | `AGENTS-apps.md` | toucher `apps.py`, `dialogs/apps.py`, `apps.toml` ou un script `*-install` |
| Barre d'état et préférences : pastille, menu, mises à jour depuis l'icône | `AGENTS-tray.md` | toucher `tray.py`, `prefs.py`, `dialogs/prefs.py` ou l'autostart |
| Fenêtre et dialogues : onglets, cartes, panneaux, ordre des menus, fenêtres de tâche | `AGENTS-ui.md` | toucher `window.py`, `widgets.py`, `tasks.py`, l'ordre d'un menu ou une fenêtre de tâche |

Nouveau fait durable → dans le document du domaine **qui possède** le code
concerné, jamais dupliqué ailleurs ; une règle transverse reste ici. Ce qui
touche la structure (nouveau module, nouveau dialogue, nouvel onglet) va aussi
dans le tableau du paquet et, s'il change une zone, dans l'index du dépôt
(`AGENTS.md` racine).

## Paquet

Sources = ce dossier (`src/gablue-manager/`), installées par
`files/scripts/build-manager` :

| Fichier | Rôle |
|---|---|
| `gablue-manager` | lanceur `/usr/bin/gablue-manager` : ajoute `/usr/lib/gablue-manager` (ou le dossier courant en développement) au `sys.path` puis `gablue_manager.app.main()` |
| `gablue_manager/app.py` | `HELP` et `main()` : fenêtre principale, ou modes CLI (`cli.py`) |
| `gablue_manager/cli.py` | modes `--progress`, `--confirm`, `--choose` (une boîte, sans `MainWindow` ni sondes SSH) et `--update` (mises à jour locales d'un domaine ou de tous, voir `local_updates.py`) |
| `gablue_manager/tray.py` | mode `--tray` : icône de barre d'état, pastille d'événement, menu |
| `gablue_manager/prefs.py` | préférences (`preferences.json`) : barre d'état et vérification des mises à jour |
| `gablue_manager/journal.py` | échecs et exceptions vers journald (protocole natif, anti-spam, aucun fichier) |
| `gablue_manager/instance.py` | instance unique fenêtre/tray (socket locale), lancement de la fenêtre |
| `gablue_manager/__main__.py` | `python3 -m gablue_manager` (développement) |
| `gablue_manager/constants.py` | chemins, binaires (`_which`), couleurs et textes d'état, constantes kargs/helpers |
| `gablue_manager/util.py` | cible ssh, nom affiché, URL KIO, normalisation MAC, options ssh communes |
| `gablue_manager/hosts.py` | registre `ssh-hosts.json`, recherche sur le réseau local, adresses du poste |
| `gablue_manager/ssh.py` | empreintes d'hôte, copie de clé, exécution distante, sudo distant |
| `gablue_manager/local.py` | état/commandes de la machine (sshd, avahi, kargs, CPUID, services, variante, atuin, gwine, booster, Tailscale) |
| `gablue_manager/updates.py` | mises à jour disponibles (image, Flatpaks, Homebrew, gwine, applications) : sondes, analyse, résumés |
| `gablue_manager/local_updates.py` | application des mises à jour **locales** : étapes par domaine et leur ordre (`step_for`, `steps`), titres des fenêtres, conversion d'une étape vers `LocalTaskDialog` — partagé par la fenêtre et le mode `--update`, sans Qt |
| `gablue_manager/remote_updates.py` | mises à jour d'un **poste distant**, à la demande : script SSH balisé, analyse (`parse_check`, domaine applications compris), chaîne « tout mettre à jour » (`update_chain(pending, states)`, section `apps_section`), et **`HostProgress`** (progression d'un poste du lot : étapes `::gablue:step`, JSONL de bootc `::gablue:progress`, lignes de protocole retirées du journal — sans Qt, donc testable). **Marqueurs émis par `printf '\n::gablue:…\n'`** (idem `##…` d'`apps.REMOTE_STATE_SCRIPT`) : `rpm-ostree status --json` ne finit pas par un saut de ligne, un `echo` collait `::gablue:image-check` au `}` du JSON — section perdue, `json.loads` raté, image « vérification impossible » sur tous les postes distants (vécu 09/11/2026) |
| `gablue_manager/remote_hardware.py` | résumé matériel d'un **poste distant** en une connexion SSH (script balisé `::gablue:*`, analyse réutilisant `local`) |
| `gablue_manager/apps.py` | applications (émulateurs + OpenCode 2) : état, GearLever, install/config/suppression (installées en AppImage dans `~/AppImages`) ; catalogue lu dans `/usr/share/ublue-os/gablue/apps.toml` (partagé avec le moteur `/usr/libexec/gablue-apps`) |
| `gablue_manager/workers.py` | threads : vérification des postes, clé, commande ssh, découverte |
| `gablue_manager/style.py` | QSS adapté au thème Plasma |
| `gablue_manager/uiutil.py` | `restyle_label`, `fit_dialog_height`, `recommendation_badge` |
| `gablue_manager/tasks.py` | `start_task`, dialogues de tâche locale/distante (non modaux), séquence globale (`SequentialTaskDialog`), **lot parallèle sur plusieurs postes** (`RemoteBatchDialog`), lien d'enregistrement (`_AuthLinkRow`), **proposition de redémarrage** en fin de succès (`offer_reboot`, `_reboot_row`) |
| `gablue_manager/widgets.py` | `BusyProgressBar`, `StatusDot`, `HostCard`, `LocalConfigMenu`, `InfoPanel`, `UpdatesPanel`, `AccessPanel`, `NoticeBar` |
| `gablue_manager/dialogs/` | un fichier par dialogue (`host`, `key`, `kernel`, `grub`, `secureboot`, `services`, `variant`, `btrfs`, `thermal`, `ryzen`, `clean`, `stversions`, `gwine`, `booster`, `tailscale`, `wifi`, `apps`, `updates`, `prefs`) |
| `gablue_manager/window.py` | `MainWindow` |
| `tests/` | tests unitaires stdlib des fonctions pures |

**Règle** : le paquet est la source de vérité ; ne pas réintroduire de fichier
unique assemblé. Un nouveau dialogue → un fichier dans `dialogs/` + une entrée
de menu dans `window.py`. Les imports relatifs vont dans un seul sens
(`constants ← util ← hosts/ssh/local ← workers/tasks ← dialogs ← window ← app`) :
un cycle (ex. `_ssh_opts` entre `hosts` et `ssh`) se règle en descendant la
fonction dans le module le plus bas, pas par un import tardif.

## Build

- `Containerfile-gablue` : `COPY src/gablue-manager /src/gablue-manager`, puis
  `RUN /ctx/build-manager` juste après `build-gwine`.
- `files/scripts/build-manager` : copie `gablue_manager/` dans
  `/usr/lib/gablue-manager/`, installe le lanceur en `/usr/bin/gablue-manager`
  (755) et retire les `__pycache__` (le Python du build n'est pas celui du
  poste, `/usr` est en lecture seule sur composefs).
- Développement : `src/gablue-manager/gablue-manager --help` marche depuis le
  dépôt ; `PYTHONPATH=src/gablue-manager python3 -c "import gablue_manager.app"`.
- L'image doit être reconstruite pour valider l'installation (pas l'ISO).

## Conventions transverses

- **Fermeture** : `closeEvent` demande l'arrêt de toutes les sondes de fond (annulation coopérative : `stop()`, `threading.Event`, `cancel`) puis attend avec un **budget global de 3 s** — jamais `terminate()`, un `pthread_cancel` sur un thread Python fait tomber le processus (vécu 07/11/2026). Si une sonde résiste, `has_running_workers()` est vrai et `app.main` sort par `os._exit` : le processus s'arrête avec la fenêtre, au lieu d'attendre la fin d'un `brew update` de deux minutes. `updates.cancel_running()` tue les commandes externes en cours (registre de `Popen` dans `updates._run`), et `apps.pending_updates(stop=…)` cesse d'attendre les flux restants.
- **Confirmations courtes** (`QMessageBox.question`) : une ligne qui dit le geste et sa cible, puis au plus une ligne d'effet réel (redémarrage, sessions coupées, réversibilité). Le fonctionnement interne (comment `gwine --init` compare les versions, règles sudoers, détails de bootc) va dans le journal ou l'infobulle — jamais dans la confirmation ; et le sous-titre des fenêtres de tâche parle lui aussi à l'utilisateur, sans ligne de commande (`Runner Wine et composants`, pas `gwine --init — …`). Vécu 07/11/2026 : paragraphes explicatifs et `--init` affichés jugés « moches » par l'utilisateur final.
- **Opérations simultanées** (`start_task`) : chaque opération (mise à jour, redémarrage, réglages, copie de clé) est une fenêtre **non modale** rattachée à la fenêtre principale — plusieurs postes peuvent être traités en même temps, et la fenêtre principale reste utilisable. Les dialogues de configuration se ferment après « Appliquer » et laissent la tâche tourner ; `_tasks` garde les fenêtres vivantes, `completed` déclenche le rafraîchissement des cartes. Un `exec()` (modale) ne doit plus servir qu'aux dialogues qui rendent un résultat immédiat (ajout/édition de poste, choix de candidats).
- **Sondes de fond et destruction des QThread** : un slot reçoit **son** worker en argument (`lambda w=worker: …`), il ne relit jamais un pointeur partagé — `_on_check_done` relisait `self._checker`, et quand une passe démarrait entre l'émission de `finished_all` et sa livraison, le slot attendait 2 s **la nouvelle** (jusqu'à 8 s par poste, `CHECK_DEADLINE`) puis faisait `deleteLater()` dessus : Qt tue le processus (« QThread: Destroyed while thread is still running », 12 fois du 05 au 09/11/2026, vidé à la sortie de la boucle Qt donc vu « à la fermeture »). Le `deleteLater` d'un `QThread` se branche **uniquement** sur `finished` (`worker.finished.connect(worker.deleteLater)`), jamais à la main ni sur un pointeur périmé. À la sortie, un worker qui résiste garde sa référence (PySide détruirait son QThread) : la fenêtre et le tray sortent par `os._exit` ; le tray demande `stop()` avant `wait()` (sinon `brew`/`gwine --check` survit à l'interpréteur).
- **Privilèges** : `privileged_prefix()` sonde `sudo -n /usr/libexec/gablue-cpuid-emu status` (0 = règle NOPASSWD présente) puis retombe sur `pkexec` (dialogue PolicyKit) — `sudo -n true` échouait toujours, même sur Gablue, et faisait passer toute action root par PolicyKit malgré la règle sudoers. **Même piège à distance** : `remote_sudo_nopasswd()` sonde `sudo -n gablue-cpuid-emu status`, seul helper dont `status` sort 0 quelle que soit la configuration ; `sudo -n true` n'a aucune règle, et `gablue-auto-update status` sort 1 dès qu'un timer est désactivé — un poste aux mises à jour coupées passait pour « sans règle NOPASSWD » et réclamait le mot de passe pour redémarrer, éteindre ou faire `bootc upgrade` pourtant autorisé. Les réglages noyau, émulation CPUID, services, couche Vulkan, pilote Wi-Fi, mises à jour automatiques, rebase et Tailscale n'utilisent ni l'un ni l'autre : ils passent par les helpers `/usr/libexec` en `sudo -n` (sans invite). Serveur SSH et annonce locale ont leurs règles exactes (systemctl sshd, `gablue-avahi`), redémarrage/extinction distants aussi (`systemctl reboot|poweroff`). Aucune commande root n'est lancée sur le thread GUI.
- **Actions externes** : `gremote [-p port] user@host` (port non standard) et `dolphin sftp://user@host:port/` (IPv6 entre crochets). Lancement détaché (`start_new_session`) : fermer la fenêtre ne tue pas la session RDP ; sortie capturée et affichée seulement si le code retour est ≠ 0, sauf le 131 de `sdl-freerdp` (fermeture normale de la fenêtre RDP) et les arrêts demandés depuis l'app.
- **Journal** (`journal.py`) : **aucun fichier**, aucune rotation — les journaux de tâche restent en mémoire (bornés par Qt) et le seul log durable est celui de systemd. On y écrit les **échecs de tâche** (`RemoteTaskDialog`/`LocalTaskDialog`/`SequentialTaskDialog`, avec code retour et 4 000 derniers caractères), les **exceptions non attrapées** (`install_excepthook` au démarrage) et l'échec d'une sonde de mises à jour (une fois par processus et par domaine, avec le code retour et la fin de la sortie), y compris le verdict `unknown` de la sonde gwine (un « vérification impossible » muet ne se diagnostiquait pas) ; **jamais les sondes qui réussissent** ni les passages périodiques. Protocole natif `/run/systemd/journal/socket` (`SYSLOG_IDENTIFIER=gablue-manager`, `PRIORITY=3`, `GABLUE_CODE`), repli sur `/dev/log` ; toute erreur d'envoi est silencieuse. Lecture : `journalctl --user -t gablue-manager -b`. Anti-spam maison, en plus du `RateLimitBurst` de journald : même première ligne + même priorité → une seule entrée par 10 min, et 200 entrées maximum par processus (la dernière annonce la coupure) — un bug qui boucle ne peut donc pas noyer le journal. Piège du protocole : le datagramme **doit** finir par un saut de ligne, sinon journald perd le dernier champ (constaté 07/11/2026 — `GABLUE_CODE` disparaissait ; revérifier si journald évolue).

## Modes CLI (sans fenêtre principale)

`gablue-manager` sans argument ouvre la fenêtre principale. Quatre modes
n'instancient **que** la boîte demandée : `MainWindow` n'est jamais créée, donc
aucune sonde SSH (les `CheckWorker` du timer de 30 s), aucun scan Avahi, aucun
accès au registre des postes.

| Mode | Rôle |
|---|---|
| `--progress --title T [--subtitle T] [--auto-close-success\|--auto-close] -- commande…` | `LocalTaskDialog` sur la commande (canaux fusionnés, annulation SIGTERM puis SIGKILL). La commande publie `::gablue:step n total texte`, `::gablue:busy texte` (total inconnu) et `::gablue:done [texte]` ; ces lignes de protocole sont retirées du journal (`local.parse_gablue_progress`), l'outil écrivant sa version lisible à côté. Échec → journal révélé ; `--auto-close-success` ferme la fenêtre en cas de succès. Code de sortie = celui de la commande (130 si annulé). |
| `--confirm --title T --text T [--yes T] [--no T]` | `QMessageBox` ; sortie 0 = oui, 1 = non. |
| `--choose --title T --text T --item valeur [--item valeur…]` | `QInputDialog` ; imprime la valeur choisie (libellé après une tabulation), sortie 1 si annulé. |
| `--update all\|image\|flatpak\|gwine\|apps\|brew [--auto-close-success]` | Applique les mises à jour de ce poste — un domaine ou tous — avec les étapes de `local_updates` (mêmes commandes et analyseurs que la fenêtre), dans `LocalTaskDialog` ou `SequentialTaskDialog`. **Une mise à jour à la fois par poste** : le mode revendique `instance.update_socket()` pour toute sa durée (sortie **4** si une autre tourne, ce qui couvre le menu de l'icône et la fenêtre simultanés). Sorties : 0 réussi, 1 échec, 130 annulé, 2 usage. La sonde des AppImages (domaines `all` et `apps` seulement) tourne d'abord derrière une fenêtre d'attente indéterminée (`QProgressDialog` + boucle locale : `wait()` figerait l'affichage), puis la fenêtre de progression s'ouvre seule. En fin de passe le mode envoie `updates` à l'icône ; un redémarrage encore nécessaire garde la fenêtre ouverte pour son bouton, même avec `--auto-close-success`. |

Sans affichage, `--progress` exécute la commande telle quelle (sans protocole),
`--confirm` répond non et `--update` refuse (code 1) : gwine reste utilisable en
terminal. gwine s'en sert au premier lancement (initialisation du préfixe dans la
fenêtre de progression),
dans `conflict.sh`/`wgp/mount.sh` (oui/non) et `wgp/modes.sh` (choix de
l'exécutable d'un WGP). Les mises à jour locales de gwine dans la fenêtre
principale passent aussi `GABLUE_PROGRESS=1` et `parse_gablue_progress`, donc
la barre d'étapes remplace le journal, resté derrière « Terminal ».


## Tests

```bash
PYTHONPATH=src/gablue-manager python3 -m unittest discover -s src/gablue-manager/tests -v
```

Tests unitaires sans affichage (fonctions pures : kargs, découverte, services, variante, atuin, Tailscale, gwine,
booster). Les dialogues se vérifient hors écran :
`QT_QPA_PLATFORM=offscreen` avec un `HOME` de bac à sable.

`tests/test_appimage_engine.py` charge le moteur `/usr/libexec/gablue-apps` **par chemin** (ce n'est pas un `.py`)
et couvre le catalogue, la résolution par type de dépôt, la reprise et l'anti-cale du téléchargement, le patch
`.desktop` et le flux d'installation complet avec un `flatpak` factice. **Tout passe par un `$HOME` de bac à
sable** : patcher `Path.home()` seul a laissé un faux GearLever écrire dans le vrai `~/AppImages` et écraser un
`.desktop` (vécu 09/11/2026) — d'où le helper `sandbox()` qui patche aussi `HOME`, et le garde-fou du shim qui
refuse un `HOME` hors `/tmp`.
