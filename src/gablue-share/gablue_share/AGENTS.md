# Gablue Share — GUI Python (`gablue_share/`)

PySide6, ~70-120 Mo **seulement fenêtre ouverte** : elle ne sert rien et ne
survit pas à sa fermeture. Le serveur (démon Rust, voir `../daemon/AGENTS.md`)
est un autre processus, qui lui survit. Lancée depuis les sources, elle prend
le binaire de `daemon/target/release/` avant celui de l'image — sinon on teste
une interface neuve contre un démon qui ignore les champs récents.

| Fichier | Rôle |
|---|---|
| `gablue-share` | lanceur `/usr/bin` (sys.path puis `gablue_share.app.main`) |
| `gablue_share/app.py` | `main()`, `--check` (construit la fenêtre et sort, sans démon), `_leave` |
| `gablue_share/config.py` | **les dossiers partagés** : `share.json`, écriture atomique, fichier corrompu mis de côté |
| `gablue_share/daemon.py` | démarrage/arrêt du démon (détaché), adoption d'un démon déjà là, `probe()` |
| `gablue_share/discovery.py` | registre des postes, mDNS (avahi-browse) + sonde Tailscale |
| `gablue_share/api.py` | client HTTP du protocole (threads) |
| `gablue_share/downloads.py` | transfert fichier (reprise) et dossier (tar), `.part`, pause, reprise auto |
| `gablue_share/window.py` | quatre zones : barre de l'application (tiroir « Mes partages »), liste des partages trouvés et sa recherche globale, navigateur d'un partage, file de transferts |
| `gablue_share/service.py` | permanence : unité `systemd --user`, `loginctl enable-linger`, `MODES` |
| `gablue_share/style.py`, `util.py`, `constants.py` | QSS dérivé de la palette, formatage, chemins |

## Découverte — règles non négociables

- **Le user ne saisit jamais un nom de machine.** Aucun champ « ajouter un
  poste », aucune adresse à taper : la liste se remplit et se vide seule.
- **Un poste n'apparaît que s'il a au moins un partage.** Le démon ne publie
  son annonce mDNS que si `shares` est non vide ; à la suppression du dernier
  partage, l'annonce est retirée (le poste disparaît des listes). Côté
  Tailscale, la sonde lit `/api/info` et écarte les `shares: []`.
- Identité = `peer_id` (machine-id), **pas** le nom d'hôte ni l'adresse : un
  poste joignable en LAN et en Tailscale est une seule entrée.
- **Son propre poste n'est jamais dans « Partages trouvés »** : la liste ne
  contient que les **autres**. Ce qu'on partage se règle dans le tiroir
  « Mes partages », et s'y voir invitait à s'y connecter comme ailleurs
  (demandé le 15/11/2026 — la vue d'ensemble de tous les partages, y compris les
  siens, a été retirée pour la même raison : elle ajoutait un intermédiaire
  sans répondre à un besoin réel).
- **La recherche globale vit sur l'accueil**, sous le titre « Partages
  trouvés » : elle interroge tous les partages trouvés en parallèle (6 fils,
  `limit` 60 par partage, 300 ms après la dernière frappe) et chaque résultat
  porte son partage, son poste et son dossier. Double clic = on ouvre le dossier
  qui le contient ; sélection multiple et clic droit permettent de
  **télécharger directement** depuis les résultats, et la colonne « Taille » des
  dossiers se remplit à la demande (`…` puis la valeur, `+` si le calcul est
  tronqué). Une recherche en cours est rejouée quand la liste des partages
  change.
- **« Mes partages » est un tiroir** ouvert depuis la **barre de l'application**
  (bouton `☰ Mes partages (N)` en haut à gauche, hors des pages) : fermé par
  défaut, la navigation reçoit toute la largeur. Ouverture/fermeture animées
  (`QPropertyAnimation` sur `pos`, 160 ms), **voile** cliquable sous le tiroir
  (`Scrim`), **Échap** ferme, et le bouton rebascule. Deux pièges Qt rencontrés :
  un `QWidget` nu ignore un fond de feuille de style sans
  `Qt.WA_StyledBackground`, et le nom d'objet (`#scrim`) doit être posé — sans
  l'un ou l'autre le voile ne se peignait pas (constaté 15/11/2026). Le tiroir
  n'est pas dans une disposition : géométrie recalculée à chaque ouverture et à
  chaque redimensionnement. Le bouton était d'abord dans l'en-tête de chaque
  page, à côté du titre : il s'y lisait comme une action de la page (retiré le
  15/11/2026).
- **L'écran d'accueil liste les partages, pas les postes** : une ligne par
  partage, sous-titre « poste · Réseau local|Tailscale », clic = ouverture
  directe de ce partage. Un écran intermédiaire « postes à proximité » obligeait
  à deux clics pour ce qu'on cherche (un dossier), et le nom du poste reste
  visible sur chaque ligne.
- **Sans aucun partage trouvé, l'écran ne garde que l'explication** : ni
  « Partages trouvés », ni barre de recherche, ni « Actualiser » — trois
  commandes au-dessus du vide. À la place, un arc animé (`Spinner`, peint à la
  main : la barre indéterminée de Qt ressemble à un trait qui saute à 26 px) et
  deux lignes qui disent qu'on cherche. Pendant ce temps la découverte rescane
  toutes les **5 s** (`MainWindow._scan_quick`, fenêtre visible) au lieu des 30 s
  ordinaires : sinon le premier poste mettait une demi-minute à apparaître et
  l'application semblait en panne (demandé le 15/11/2026). Dès qu'un partage est
  trouvé, le rescan s'arrête — mDNS écoute déjà en continu ; il reprend si la
  liste se vide. `Discovery._cycle()` porte un verrou : le rescan rapide ne
  double jamais une passe de 30 s encore en cours sur les mêmes adresses.
- **Adresse** : le LAN est essayé d'abord (un aller-retour sur le réseau local
  coûte moins qu'un détour par le tailnet), et la sonde retient l'adresse qui a
  **réellement répondu** (`active_addr`). Sans ça, un LAN mort mais un tailnet
  vivant faisait échouer la navigation : la sonde basculait, pas la GUI.
- **Deux gestes de navigation, deux boutons** : `←` (en-tête) revient à la page
  d'où l'on vient (la liste des partages trouvés) ; `↑ Dossier parent` remonte
  d'un cran, se désactive à la racine, et **vit au-dessus de la liste avec le
  chemin** — c'est là qu'on regarde où l'on est, donc là où l'on veut remonter.
  Un seul ← pour les deux laissait croire qu'on ne pouvait pas sortir d'un
  sous-dossier (vécu 15/11/2026).
- **Colonnes réglables, « Nom » plus large** (`QHeaderView.Interactive` +
  `ColumnFitter`) : la première colonne absorbe la largeur restante, sinon la
  grille s'arrêtait à « Modifié » en laissant un vide à droite ; élargir
  « Taille » ou « Modifié » rogne « Nom », et une fenêtre plus large lui rend le
  surplus. Le minimum est de 140 px : au-delà, une barre horizontale apparaît,
  ce qui est alors le choix de l'utilisateur. L'ajustement suit les
  redimensionnements du tableau (filtre d'événements, donc aussi l'apparition
  d'une barre de défilement) et se rejoue une fois la boucle rendue quand la
  mise en page bouge encore.

## Démon vu de la GUI

- **Sans aucun dossier déclaré, rien ne démarre** : `MainWindow._start_daemon`
  s'arrête avant de lancer quoi que ce soit et arrête le démon qu'il possédait
  (`share.json` est relu à chaque changement). Le réglage de permanence, lui,
  reste tel quel : c'est le démarrage qui est sauté, pas le réglage. La
  découverte, elle, tourne toujours — on voit les partages des autres sans rien
  partager soi-même.
- **Détaché** : `subprocess.Popen(..., start_new_session=True)`, plus de
  `--parent-pid` ; fermer la fenêtre ne l'arrête pas. Au lancement suivant,
  `Daemon.start()` le retrouve par `/api/info` et l'adopte
  (`Daemon.external`), sans en lancer un second.
- **`--launcher`** : la GUI transmet son propre chemin (`constants.launcher()`)
  pour que « Ouvrir Gablue Share » depuis l'icône relance la bonne installation
  (dépôt ou image).

## Second compte sur le poste

**Un seul compte à la fois sur le poste.** Le port est fixe (47823) et le
`peer_id` est le machine-id : deux comptes se ressemblent trait pour trait.
`/api/info` porte donc `uid`, et `Daemon.probe()` répond qui écoute — `mine`,
`absent`, `other`, `foreign`. Si c'est un **autre compte**, la GUI ne l'adopte
pas. Elle le faisait, en silence : le second compte croyait partager alors
qu'aucun de ses dossiers n'était servi (vécu 15/11/2026). Elle affiche donc une
bannière dans le tiroir, renomme le bouton en « ☰ Mes partages (N) — partage
indisponible », désactive « Ajouter un dossier… », « Démarrage » et « Visible
sur » (raison en infobulle), alerte une fois par lancement, et le statut du
tiroir dit « rien n'est partagé pour l'instant » au lieu d'annoncer une
visibilité qui n'existe pas. Le message nomme le compte (`pwd.getpwuid`) ou son
uid s'il n'existe plus. Le minuteur de disponibilité (2 s) retente : dès que
l'autre compte ferme son application, le partage reprend et une notification le
dit. Les fichiers de partages restent séparés par utilisateur, rien n'est perdu.

## Style : ce qui se voit

- **Boutons d'action des lignes en `#chip`** : contour tracé avec le **texte du
  thème rendu translucide** (`rgba`, 30 %), jamais `QPalette.Mid`. Dans un thème
  sombre, `Mid` se confond avec le fond du panneau : les boutons des transferts
  n'avaient plus de contour du tout (constaté 15/11/2026). Une teinte dérivée du
  texte est lisible en clair comme en sombre.
- **Barres de défilement dessinées par la feuille de style** (poignée arrondie,
  piste transparente, sans flèches) : sinon Qt peint la sienne, qui jure avec le
  reste.
- Les captures de vérification doivent appliquer `style.stylesheet()` — sans
  elle, Qt rend son style par défaut et la capture ne dit rien du rendu réel.

## File de téléchargement (livrée en M3)

- La file vit dans la GUI (`~/.local/state/gablue-share/queue.json`, écriture
  atomique, sauvegarde à 800 ms au plus), pas dans le démon : fermer la GUI
  arrête les transferts ; le démon, lui, continue de servir (il vit jusqu'à
  l'extinction ou « Quitter le partage »).
- Un élément est identifié par `(peer_id, share_id, chemin relatif)`, **jamais
  par une adresse** : au redémarrage, la découverte retrouve le poste (LAN ou
  Tailscale, l'adresse a pu changer) et la reprise repart seule.
- Reprise : `HEAD` pour l'ETag → même ETag = `GET Range: bytes=<taille .part>-`
  avec `If-Range` ; 200 = le fichier a changé, on repart de zéro ; 416 = il a
  rétréci, idem.
- États : `en attente du poste` (hors ligne : l'élément reste, il n'est jamais
  supprimé et repart quand le poste revient), `en attente du partage` (partage
  retiré), `en attente` (prêt), `en cours`, `terminé`, `échec` (« Relancer »),
  `annulé` (disparaît du fichier). Au chargement, un élément `terminé` est
  oublié (la file ne garde que du travail à faire), un élément `en cours`
  devient `en attente du poste`.
- Le `.part` (`.nom.<etag8>.part`) reste dans le dossier de destination. Pas de
  purge par ancienneté (choix du 15/11/2026) : **retirer un élément de la file
  efface son `.part`** (`Downloads.remove`), c'est le seul cas où un demi-fichier
  ne sert plus à rien.
- **Destination unique, même en lot** : `unique_target` reçoit les noms déjà
  réservés par les transferts en attente. Deux fichiers de même nom venus de
  deux partages étaient sinon envoyés vers **le même chemin** (l'unicité était
  calculée avant que le premier n'existe) : un seul fichier à l'arrivée
  (vécu 15/11/2026, corrigé et vérifié).
- **Reprise automatique** : un élément « en attente » est réessayé toutes les
  5 s par un minuteur interne (et non plus seulement sur un événement de
  découverte, qui peut ne jamais venir si le poste reste annoncé). Attente
  croissante 5 s → 10 → 20 → 40 → 80 → 160 → 300 s, plafonnée ; rien ne martèle
  un poste éteint, et un disque remonté repart seul.
- **Pause / reprise** : `en pause` garde le `.part` et sort le transfert du
  worker ; « Reprendre » le remet en file. Un `.part` d'un autre ETag est
  ignoré (le fichier a changé) et laissé tel quel.
- **File bornée et repliable** : trois lignes entières au plus (somme des
  hauteurs réelles + 8 px, la feuille de style ajoutant des marges que
  `sizeHint()` ignore), le reste au défilement ; le chevron `▾`/`▸` replie la
  liste et rend la hauteur à la navigation. L'en-tête reste dans les deux cas :
  c'est lui qui informe (compte, débit, reste, temps estimé). Vingt transferts
  prenaient sinon toute la fenêtre (vécu 15/11/2026).
- **Nettoyer les terminés** : bouton dans l'en-tête des transferts, visible
  seulement s'il y a au moins un transfert réussi ; il retire les lignes
  « terminé » de la liste **sans toucher aux fichiers** (`clear_finished`). Un
  échec garde sa ligne : c'est celle qu'on veut relancer.
- **Fin de transfert** : notification de bureau (`notify-send`, silencieuse si
  absent), sauf si la fenêtre a le focus. Débit et temps restant agrégés dans
  l'en-tête du panneau.

## Fermeture (ce qui a coûté cher)

- Fermer pendant un transfert ne doit **ni** perdre la progression **ni** faire
  avorter le processus. Trois règles, chacune apprise à ses dépens :
  1. `shutdown()` remet l'élément « en cours » à « en attente » (jamais
     « annulé » : l'ancien code vidait la file et laissait un `.part` orphelin),
     écrit le fichier puis rend la main ;
  2. **jamais `QThread.terminate()`** : tuer un fil Python en pleine lecture
     système fait planter le processus (SIGSEGV constaté le 15/11/2026). On
     ferme la **réponse HTTP** en cours (`DownloadWorker.abort()`) : `read()`
     échoue, le fil voit l'interruption et sort en quelques millisecondes ;
  3. la sortie passe par `app._leave()` (`os._exit`) : Qt ne détruit pas un fil
     encore vivant, ce qui l'**avorterait** (SIGABRT). La file est écrite et
     `fsync`ée avant. Le démon, lui, n'est plus lié à la fenêtre : il survit.
- Attente bornée à **1,5 s** : au-delà (poste muet, connexion non annulable),
  on part quand même — la file reprendra au lancement suivant.

## Permanence — et démarrage au boot

Trois états, choisis par le sélecteur « Démarrage » (`service.MODES`, le même
que dans le dialogue « Partage local » de gablue-manager) :

| État | Ce qui est fait | Effet |
|---|---|---|
| `off` | `systemctl --user disable` (sans `--now`) | pas de relance à la session suivante ; le serveur en cours continue |
| `session` | unité activée ; linger retiré s'il était actif | démarre à l'ouverture de session |
| `boot` | unité activée **et** `loginctl enable-linger <user>` | systemd démarre le gestionnaire de session — donc l'unité — au boot, sans session ouverte |

- **La permanence ne dit que *quand* le serveur redémarre** : `enable`/
  `disable` **sans `--now`**. Le démon en cours n'est ni arrêté ni remplacé ;
  pour l'arrêter, « Quitter le partage » depuis l'icône. L'unité qui tourne est
  adoptée (`Daemon.external`) et jamais tuée par la GUI.
- **`linger` est la seule façon propre de démarrer au boot** : l'unité reste
  `systemd --user` (elle a besoin du `$HOME` et du bus de session de
  l'utilisateur). Une unité système devrait coder l'utilisateur en dur et
  tourner avec les chemins de l'utilisateur — fragile, et inutile ici.
- `loginctl enable-linger` **ne demande pas de mot de passe** :
  `org.freedesktop.login1.set-self-linger` est en `allow_any=yes`
  (vérifié le 15/11/2026 dans `/usr/share/polkit-1/actions/org.freedesktop.login1.policy`).
- `off` **ne touche pas** à lingering : d'autres services de l'utilisateur
  peuvent en dépendre, et sans notre unité il ne partage rien. `session`, lui,
  le retire : sinon le choix ne tiendrait pas au redémarrage suivant.
- `service.current_mode()` lit l'état réel (`is-enabled` + `Linger`), jamais un
  champ du fichier : l'interface se recale dessus après chaque tentative
  (`revert_mode`).
- La vérité est `systemctl --user is-enabled gablue-share.service`, jamais un
  champ du JSON : sinon la case ment dès qu'on touche aux unités à la main.
- L'unité est livrée par l'image dans `/usr/lib/systemd/user/` ;
  `ensure_unit()` ne l'écrit dans `~/.config/systemd/user/` que si elle manque
  (lancement depuis le dépôt, image antérieure à M4). `systemd-analyze --user
  verify` l'accepte.
