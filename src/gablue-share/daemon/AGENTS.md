# Gablue Share — démon Rust (`daemon/`)

Serveur HTTP en lecture seule : sert les dossiers déclarés dans `share.json`
(réseau local et tailnet), annonce le poste en mDNS, et **porte l'icône de zone
de notification** (StatusNotifierItem par DBus, crate `ksni`). Un seul binaire,
`gablue-shared`, ~3,2 Mo au repos, aucune dépendance graphique.

Modules : `config.rs` (lecture/validation de `share.json`), `routes.rs`
(protocole), `files.rs` (confinement, listage, ETag), `tarstream.rs` (archive
en flux), `net.rs` (filtre d'adresses), `announce.rs` (avahi), `tray.rs`
(icône). Le cycle de vie (détaché, survit à la fenêtre) est décrit dans
`../AGENTS.md`.

## Protocole (version 1)

| Route | Rôle |
|---|---|
| `GET /` | page texte d'aide (dépannage `curl`) |
| `GET /api/info` | `{protocol, peer_id, name, port, uid, shares:[{id,name,locked,salt,rounds,revision}]}` — **partages disponibles seulement** ; `uid` = compte système qui sert (deux comptes d'un poste partagent `peer_id` et port) ; `salt`/`rounds`/`revision` n'apparaissent que pour un partage protégé (l'empreinte attendue, jamais) |
| `GET /api/auth/<share>` | vérifie le mot de passe (`X-Gablue-Key`) : `200` ou `401`. Ne lit rien — la GUI valide une saisie avant d'ouvrir |
| `HEAD|GET /api/list/<share>?path=&offset=&limit=&sort=&desc=` | `{entries, parent, truncated}` + `ETag` ; `HEAD` = empreinte seule, `304` si `If-None-Match` correspond |
| `GET /api/find/<share>?q=&path=&limit=` | recherche récursive par nom : `{entries:[{name,path,type,size,mtime}], truncated, scanned}` |
| `GET /api/size/<share>?path=` | taille d'un dossier : `{files, bytes, complete}` |
| `HEAD|GET /f/<share>/<chemin>` | fichier, `Range`/`If-Range`, `ETag`, `Content-Disposition` |
| `GET /a/<share>/<chemin>?as=<nom>` | dossier en flux tar (ustar, chunked) ; `as` impose le nom du dossier dans l'archive |

Invariants :

- **Lecture seule.** Aucun verbe d'écriture, aucun état côté serveur — même
  pour les partages protégés : l'empreinte du mot de passe accompagne chaque
  requête (`X-Gablue-Key`), le démon n'ouvre aucune session.
- **Toujours un `Content-Length` quand la taille est connue** :
  `Response::with_chunked_threshold(usize::MAX)` est obligatoire, sinon
  `tiny_http` bascule en chunked au-delà de 32 Kio — le `HEAD` d'un gros
  fichier ne dit plus sa taille et la reprise perd son point de départ
  (bug trouvé à l'intégration du 14/11/2026, test de régression dans
  `tests/protocol.rs`). Le tar, de longueur inconnue, reste chunked.
- **ETag = mtime (ns) + taille.** Gratuit et stable entre deux démarrages :
  c'est ce qui rend la reprise d'un téléchargement possible après fermeture de
  l'app. Si le fichier a changé, l'ETag change et la reprise repart de zéro.
- **Confinement** : `canonicalize` puis `starts_with(root canonique)` ; `..`
  refusé (y compris `%2e%2e`), symlink sortant invisible au listing et 404 en
  GET. Le partage ne peut pas fuiter hors de son dossier.
- **Filtre d'adresses** : boucle locale, RFC1918, lien-local, ULA IPv6,
  `100.64.0.0/10` (Tailscale). Le reste → 403 journalisé. `--allow-nonlocal`
  existe pour les tests seulement.
- `tiny_http` ne pose pas de timeout de lecture : acceptable sur LAN/tailnet
  filtré, à revoir si le partage devient exposé.
- Le port par défaut (47823) est **fixe** : la sonde Tailscale ne peut pas
  découvrir un port dynamique.
- **Encodage des paramètres** : dans une valeur de requête, `+` vaut une espace
  (le client encode ainsi), un `+` littéral arrive en `%2B`. Le chemin d'URL,
  lui, garde ses `+`. Sans cette distinction, un partage nommé « Photos de
  l'été » s'extrayait en `Photos+de+l'été` (vécu 15/11/2026).

## Fraîcheur, recherche, tailles (coût maîtrisé)

- **Listing à jour sans clic.** Chaque page de listing porte un `ETag` (FNV-1a
  sur nom/type/taille/mtime des entrées). La GUI fait un `HEAD` conditionnel :
  `304` = rien à faire, quelques centaines d'octets au lieu du dossier entier.
  Cadence **3 s** quand la fenêtre a le focus, **10 s** quand elle est visible
  mais au second plan, **rien** quand elle est cachée ou réduite. Le coût réel
  est un `readdir`+`stat` côté démon à chaque sonde : mesuré à ~15 ms pour
  20 000 entrées, soit 0,5 % d'un cœur au pire — et zéro au repos.
  Mesures du 15/11/2026 sur 3000 fichiers : sonde `304` **0,2 ms**, listing
  de 10 entrées 1 ms, 10 recherches 4 ms chacune, RSS **3,2 Mo** au repos et
  3,7 Mo après 50 requêtes (stable ensuite) — la charge des nouvelles routes
  ne laisse aucune empreinte.
- **Tri par défaut : date de modification, la plus récente en tête**
  (`sort=mtime&desc=1`). C'est ce qu'on cherche presque toujours ; le nom, la
  taille et la date restent à un clic sur l'en-tête (sens inversé au second).
- **Pagination** : `PAGE = 500` côté GUI, la suite se charge en défilant
  (`offset`). Le serveur borne à 5000 par requête. Le tri (`sort=name|size|mtime`,
  `desc=1`) est fait **côté serveur** : trier la page affichée donnerait un
  ordre faux dès qu'il y a plusieurs pages.
- **Recherche** (`/api/find`) : parcours récursif borné à **400 ms** et à
  `limit` résultats ; `truncated` prévient que c'est partiel. Un partage de
  200 000 fichiers ne peut donc pas immobiliser un fil du serveur. La GUI
  attend 300 ms après la frappe avant d'interroger.
- **Taille d'un dossier** (`/api/size`) : calcul borné à **600 ms**, jamais
  fait pour tout un listing — seulement pour le dossier sélectionné (et mis en
  cache côté GUI). `complete: false` = minorant assumé.
- **Télécharger un dossier entier** : `/a/<share>` sans chemin (bouton « Tout le
  partage ») ; `as=<nom>` fait porter au dossier extrait le nom affiché dans
  l'app, pas le nom physique côté source. Un `as` contenant `/`, `..` ou
  commençant par `.` est ignoré (retour au nom physique) : jamais de traversée.

## Partages protégés par un mot de passe

Champ `password` d'un partage dans `share.json`, écrit par la GUI
(`gablue_share/config.py`), appliqué **ici** sur toutes les routes qui livrent
quelque chose :

```json
"password": {"salt":"…32 hex…", "rounds":200000, "hash":"…64 hex…", "revision":"…16 hex…"}
```

- **Le mot de passe ne circule jamais et n'est jamais dérivé ici.** La GUI
  calcule `pbkdf2-hmac-sha256(mot de passe, sel, rounds)` ; le client refait le
  même calcul avec le sel publié par `/api/info` et envoie le résultat dans
  `X-Gablue-Key`. Le démon **compare deux chaînes** : aucune dépendance
  cryptographique, aucune clé à protéger dans le composant exposé au réseau.
  Un fichier malformé reste verrouillé (`hash` impossible) : un verrou cassé
  n'ouvre pas le partage.
- **Comparaison à temps constant** (`Password::accepts`) : une réponse plus
  rapide sur le premier caractère faux dirait combien sont justes.
- **Tout est verrouillé sauf `/api/info`, `/api/auth` et `/`** : `/api/list`
  (GET **et** `HEAD`), `/api/find`, `/api/size`, `/f/`, `/a/` répondent `401`
  avec `WWW-Authenticate` sans l'empreinte. Une seule route oubliée serait une
  porte : le garde est un seul helper (`share_or_denied`).
- `/api/info` annonce `locked: true` **et** le jeton de version (`revision`,
  régénéré à chaque changement de mot de passe) : le client sait qu'il devra
  demander, écarte le partage de la recherche globale tant qu'il ne l'est pas,
  et reconnaît un mot de passe changé au jeton — pas besoin de garder l'ancien.
- **Coût : une comparaison de chaînes par requête**, aucune entrée de cache, donc
  un mot de passe changé prend effet au rechargement à chaud (≤ 3 s), sans
  redémarrer ni couper un transfert. Vérifié par
  `locked_share_requires_its_password_everywhere` (vrai serveur, vrai fichier).
- **Ce que le verrou ne protège pas** : un réseau écouté. Sans TLS (choix de
  M1), l'empreinte d'une requête est rejouable jusqu'au changement de mot de
  passe. C'est une barrière contre l'accès opportuniste ; Tailscale chiffre le
  tailnet, pas le LAN.

## Visibilité : réseau local, tailnet, ou les deux

- Choix **par machine** (`visibility` dans `share.json`), sélecteur dans « Mes
  partages » : `both` (défaut), `lan`, `tailscale`.
- **C'est le démon qui applique la règle**, à chaque requête (relue à chaud, sans
  redémarrage ni coupure de transfert) : `lan` refuse les adresses du tailnet
  (`100.64.0.0/10`, `fd7a:115c:a1e0::/48`), `tailscale` refuse le réseau local.
  La boucle locale reste toujours autorisée : c'est par là que la GUI du poste
  interroge son propre démon.
- **Annonce mDNS seulement pour `both` et `lan`** : mDNS ne parle que sur le
  réseau local, donc en mode tailnet on n'annonce rien du tout (annonce retirée
  si elle existait) — les autres postes trouvent le nôtre par leur sonde
  `tailscale status`. Annoncer puis refuser serait une invitation trompeuse.
- Le filtre d'adresses et le mot de passe (facultatif, par partage) sont les
  deux barrières : `--allow-nonlocal` demeure l'échappatoire de test/dépannage.

## Partages : identité, noms, disques externes

Format écrit par la GUI (`gablue_share/config.py`), lu par le démon ici.

- **Identifiant caché et stable** : `s` + 12 hex de `sha256(chemin réel)`. Dérivé
  du chemin, jamais du nom : renommer ne casse rien, et retirer puis re-ajouter
  le même dossier redonne le même identifiant — c'est ce qui permet à la file de
  se rattacher. Les identifiants d'une version antérieure sont réalignés une
  fois au chargement (`config.load`), ce qui réécrit `share.json` à ce moment-là.
- **Nom** : optionnel, saisi à l'ajout (pré-rempli avec le nom du dossier) et
  modifiable par clic droit ▸ « Renommer… ». Vide = nom du dossier, côté démon
  comme côté client.
- **Disque externe** : un partage dont le dossier n'existe plus reste **déclaré**
  mais `available: false` — ni annoncé, ni listé dans `/api/info`, ni servi
  (`404 « partage indisponible »`). Le démon re-vérifie la présence de chaque
  dossier toutes les 3 s (un `stat`), donc un disque remonté réapparaît sans
  intervention. Règle de disponibilité : `is_dir` +, sous `/mnt`, `/media`,
  `/run/media`, un périphérique différent de celui du parent (point de montage
  laissé vide). La GUI reflète l'état (« Disque débranché ») sans décider.
- **Un seul partage par dossier** : deux écritures du même chemin sont refusées ;
  l'identifiant étant dérivé du chemin, deux entrées identiques seraient de
  toute façon le même partage.

### Options de vue d'un partage (réglées par celui qui partage)

Deux champs par partage dans `share.json`, appliqués **par le démon** (le client
ne peut pas les contourner) et relus à chaud :

| Champ | Défaut | Effet |
|---|---|---|
| `hidden` | `false` | les noms commençant par `.` n'apparaissent ni au listing ni à la recherche |
| `recursive` | `true` | `false` = seuls les **fichiers de la racine** du partage sont listés, servis et trouvables |

- **Télécharger ≠ lister.** Une archive (`/a/`) est toujours complète : un dossier
  téléchargé emporte ses sous-dossiers **et** ses fichiers cachés, même masqués
  au listing. C'est le comportement demandé (15/11/2026) : on masque pour ne pas
  encombrer la vue, pas pour amputer un téléchargement.
- **La taille annoncée suit le téléchargement** (`/api/size`), donc elle compte
  tout, cachés et sous-dossiers compris. Sinon « 2 fichiers · 6 Kio » à côté
  d'une archive de 40 fichiers serait un piège.
- `recursive: false` refuse les sous-chemins partout (`/api/list?path=`,
  `/api/find?path=`, `/api/size?path=`, `/f/<share>/sous/fichier`, `/a/<share>/sous`
  → `404 « ce partage ne contient que sa racine »`), et la recherche ne descend
  pas. L'archive du partage entier reste permise, évidemment.
- L'interface expose les deux options au clic droit sur un partage (« Afficher
  les fichiers cachés », « Inclure les sous-dossiers ») et les résume sous le
  chemin (« cachés masqués · sous-dossiers inclus »).

## Cycle du serveur

- **Partage arrêté depuis la GUI** (`serving: false` au niveau du document) :
  `run()` sort en **0** avant d'écouter, et le rechargeur fait de même quand la
  valeur passe à `false`. C'est la **même vérité** pour la fenêtre, le service
  permanent et un démon détaché d'une session précédente : un partage arrêté ne
  revient pas parce qu'on rouvre Gablue Share (demandé le 15/11/2026).
- **Plus aucun dossier déclaré** : `run()` sort en **0** avant même d'écouter,
  et le rechargeur (3 s) fait de même quand le dernier dossier est retiré. Un
  serveur qui ne sert rien n'a ni port, ni annonce, ni icône. Filet pour un
  lancement direct ou une unité systemd restée activée — la GUI ne le lance
  déjà pas dans ce cas.
- **Rechargement à chaud** : `share.json` est relu sur changement de mtime ou
  d'empreinte de disponibilité ; visibilité et options suivent sans redémarrage.
- **Détaché** : la GUI le lance avec `start_new_session` et l'adopte au
  lancement suivant par `/api/info`. `--parent-pid` est encore accepté (GUI plus
  ancienne) mais **sans effet** : le serveur survit à la fenêtre.
- **`--launcher CHEMIN`** : le lanceur de la GUI, utilisé par « Ouvrir Gablue
  Share » depuis l'icône (dépôt et image n'ont pas le même chemin).
- **`--no-tray`** : coupe l'icône (tests : sinon chaque serveur de test affichait
  une icône sur le bureau de l'utilisateur, une vingtaine d'un coup — vécu
  15/11/2026). Côté Python, `Daemon.tray = False` pour les GUI de test.

## Icône de zone de notification (`src/tray.rs`)

**Hébergée par le démon** (crate `ksni`, features `blocking` + `async-io` :
StatusNotifierItem en DBus, aucun toolkit). Pas de processus en plus, pas de
Qt : une icône Qt coûtait ~50 Mo là où le démon en pèse 3,2 — l'icône doit
coûter quelques kilo-octets, pas une boîte à outils graphique (choix du
15/11/2026, `gablue-manager --tray` mesuré à 114 Mo comme contre-exemple).

- **Elle existe si et seulement si le démon existe** : `run()` ne démarre pas
  sans dossier déclaré, et n'appelle `tray::spawn` que dans ce cas. Plus de
  verrou, plus d'autostart, plus de sonde : un seul propriétaire.
- **Un serveur permanent peut naître avant le bureau** :
  `assume_sni_available(true)` + `watcher_offline` par défaut gardent le
  service vivant ; l'icône s'enregistre quand le `StatusNotifierWatcher` de
  Plasma arrive.
- **`icon_name() = "gablue-share"`** : le SVG installé, rien à décoder ni à
  embarquer. `title()` et l'infobulle disent « Gablue Share » et le nombre de
  dossiers **servis** (dossier présent), pas seulement déclarés.
- **Clic** : bascule la fenêtre — socket `CONTROL_SOCKET` entendue → ordre
  `quit` (la fenêtre se ferme, le serveur reste) ; sinon lance le lanceur reçu
  par `--launcher`, détaché (`process_group(0)`), et **ramasse l'enfant**
  (un fil fait `wait()`, sinon chaque ouverture/fermeture laissait un zombie
  sous le démon — vécu 15/11/2026).
- **Le lancement récupère l'environnement de session du bureau**
  (`session_environment`) : l'unité `systemd --user` peut démarrer avant la
  session, et son environnement ne porte alors que `XDG_RUNTIME_DIR` — un Qt
  lancé sans `WAYLAND_DISPLAY` avorte (« no Qt platform plugin could be
  initialized », quatre plantages d'affilée en cliquant l'icône, vécu
  15/11/2026). On relit `WAYLAND_DISPLAY`, `DISPLAY`, `XAUTHORITY` et
  `ICEAUTHORITY` (ouvrir une fenêtre), puis l'identité du bureau
  (`XDG_CURRENT_DESKTOP`, `KDE_FULL_SESSION`, `KDE_SESSION_VERSION`,
  `DESKTOP_SESSION`, `XDG_SESSION_DESKTOP`, `XDG_SESSION_TYPE`), plus
  `XDG_CONFIG_DIRS` (qui porte `kdedefaults`) et `QT_WAYLAND_RECONNECT`, dans
  `systemctl --user show-environment` au moment du clic — et seulement ceux qui
  nous manquent. Les trois variables d'affichage ne suffisaient pas : sans
  l'identité du bureau, Qt ne charge pas `KDEPlasmaPlatformTheme` et retombe sur
  Fusion avec sa palette claire — la feuille de style de la GUI étant dérivée
  de la palette, **la fenêtre ouverte depuis l'icône était blanche** (vécu
  15/11/2026, mesuré : Fusion `#efefef`/`#ffffff` contre Breeze
  `#202326`/`#141618` pour les mêmes 12 variables). Vérifié : `gablue-share
  --check` avec le seul `XDG_RUNTIME_DIR` → core dump ; avec le jeu complet →
  fenêtre construite, style `breeze`. Pour un démon déjà lancé,
  `systemctl --user restart gablue-share.service` suffit à lui donner
  l'environnement courant — mais le binaire de l'image, lui, ne change qu'avec
  une image reconstruite.
- **Menu (clic droit)** : état (« Partage actif — N dossier(s) partagé(s) »),
  « Ouvrir »/« Fermer la fenêtre » selon la socket, « Démarrage » (les trois
  mêmes états que le tiroir, coché sur l'état réel de systemd), « Quitter le
  partage ».
- **« Quitter le partage »** : ordre `quit` à la fenêtre puis sortie du démon
  (`exit 0`) — plus de serveur, donc plus d'icône. L'unité **reste activée** :
  le partage revient à la prochaine session (ou au prochain boot). Comme
  l'unité est en `Restart=on-failure`, sortir en 0 n'est pas relancé.
- **La permanence ne dit que *quand* le serveur redémarre** : `enable`/
  `disable` sans `--now`, le démon en cours n'est ni arrêté ni remplacé.
- **Testable en headless** : `/tmp/opencode/gshare-tray.py` interroge le vrai
  `StatusNotifierWatcher` par `busctl`/`gdbus` — nom
  `org.kde.StatusNotifierItem-<pid>-1`, infobulle, plan de menu, `Activate`
  (ordre `quit` ou lanceur), et `Event` sur « Quitter le partage ».
