# Gablue Share — paquet `gablue_share` (src/gablue-share)

Partage de dossiers entre postes Gablue, sur le réseau local **et** le tailnet :
chaque poste sert en lecture seule ce qu'il déclare, les autres voient les
postes apparaître seuls et y prennent ce qu'ils veulent. Aucune
authentification, aucun compte : la seule barrière est l'adresse.

Deux processus, séparés pour la RAM :

| Processus | Rôle | Qt | RSS mesuré (15/11/2026) |
|---|---|---|---|
| `gablue-shared` (`daemon/`, Rust) | sert les dossiers en HTTP, annonce le poste | non | **3,2 Mo** au repos, 3,7 Mo après 50 requêtes puis stable |
| `gablue-share` (GUI PySide6) | découverte, navigation, file de téléchargement | oui | ~70-120 Mo, seulement fenêtre ouverte |

## État

- **M1 (livré)** : démon HTTP, `/api/info`, `/api/list`, `/f/` (Range/ETag),
  `/a/` (tar), filtre d'adresses, rechargement à chaud, `--parent-pid`,
  annonce mDNS. `cargo test` : 32 tests (8 unitaires + 24 d'intégration, vrai
  serveur, vrai système de fichiers, vrais sockets).
- **M2 (livré)** : GUI PySide6 (liste des dossiers partagés persistante,
  découverte LAN/tailnet, navigation, téléchargement fichier + dossier).
  Vérifié en headless : capture réelle de la fenêtre, reprise d'un `.part`
  (500 Kio → 2 Mio, hash identique), extraction tar, collision `(1)`.
- **M3 (livré)** : file persistante (`~/.local/state/gablue-share/queue.json`),
  rattachement automatique au poste retrouvé, reprise, mode permanent
  (`systemd --user`) avec bascule depuis la GUI — et entrée « Réseau ▸ Partage
  local… » dans gablue-manager (permanence + ouverture de l'app).
- **M4** : image (build Rust en étape `share-build` jetée après coup, unité
  `systemd --user`, ouverture firewalld, `.desktop`, icône), installée par
  `files/scripts/build-share`. **Vérifié dans l'image publiée** le 15/11/2026
  (run `43655829429`, les cinq variantes vertes, `gablue-main:latest`) :
  `/usr/bin/gablue-share`, `/usr/libexec/gablue-shared` (861 904 octets,
  `ldd` : `libc`/`libgcc_s` seuls, `--help` répond), unité `ExecStart`
  correcte, `.desktop`, icône, service firewalld **et** son ouverture dans la
  zone `FedoraWorkstation`. Première panne du build : un cache monté sous
  `/root/.cargo` — dans une image ostree `/root` est un lien vers
  `var/roothome`, absent (corrigé par `CARGO_HOME=/cargo`, voir
  `files/scripts/AGENTS.md`).
- **M5 (15/11/2026)** : listing à jour sans clic (ETag + `HEAD` conditionnel,
  cadence adaptative), pagination en défilant, tri serveur, recherche récursive
  bornée, taille des dossiers à la demande, téléchargement d'un partage entier,
  noms de partages, identifiant caché dérivé du chemin, disques externes
  (partage indisponible → absent du réseau, réapparition automatique), file :
  pause/reprise, retrait qui efface le `.part`, reprise auto à attente
  croissante, notifications de fin, débit et temps restant.
- **M5.1 (15/11/2026, après essai en vrai)** : l'accueil liste les **partages**
  (avec leur poste) au lieu des postes ; recherche **globale** en parallèle sur
  tous les partages ; tri par défaut sur la date de modification ; bouton
  « Nettoyer les terminés » ; plus de bouton « Tout le partage » (le clic droit
  propose « Tout sélectionner ») ; plus de port ni d'IP affichés ; visibilité
  `both|lan|tailscale` appliquée par le démon.

## Paquet (GUI)

Sources = ce dossier, installées par `files/scripts/build-share` (paquet dans
`/usr/lib/gablue-share/`, lanceur `/usr/bin/gablue-share`).

| Fichier | Rôle |
|---|---|
| `gablue-share` | lanceur `/usr/bin` (sys.path puis `gablue_share.app.main`) |
| `gablue_share/app.py` | `main()`, `--check` (construit la fenêtre et sort, sans démon) |
| `gablue_share/config.py` | **les dossiers partagés** : `share.json`, écriture atomique, fichier corrompu mis de côté |
| `gablue_share/daemon.py` | démarrage/arrêt du démon, adoption d'un démon permanent |
| `gablue_share/discovery.py` | registre des postes, mDNS (avahi-browse) + sonde Tailscale |
| `gablue_share/api.py` | client HTTP du protocole (threads) |
| `gablue_share/downloads.py` | transfert fichier (reprise) et dossier (tar), `.part`, pause, reprise auto |
| `gablue_share/window.py` | trois panneaux : mon partage, postes/navigation (recherche, tri, défilement infini), transferts |
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
- **L'écran d'accueil liste les partages, pas les postes** : une ligne par
  partage, sous-titre « poste · Réseau local|Tailscale », clic = ouverture
  directe de ce partage. Un écran intermédiaire « postes à proximité » obligeait
  à deux clics pour ce qu'on cherche (un dossier), et le nom du poste reste
  visible sur chaque ligne.
- **Adresse** : le LAN est essayé d'abord (un aller-retour sur le réseau local
  coûte moins qu'un détour par le tailnet), et la sonde retient l'adresse qui a
  **réellement répondu** (`active_addr`). Sans ça, un LAN mort mais un tailnet
  vivant faisait échouer la navigation : la sonde basculait, pas la GUI.
- **Recherche globale** : la barre de l'écran d'accueil interroge **tous** les
  partages trouvés en parallèle (6 fils, `limit` 60 par partage, 300 ms après la
  dernière frappe), et chaque résultat porte son partage et son poste. Double
  clic = on ouvre le dossier qui le contient. Une recherche en cours est rejouée
  quand la liste des partages change.

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
- Le filtre d'adresses reste la seule barrière (pas d'authentification) :
  `--allow-nonlocal` demeure l'échappatoire de test/dépannage.

## Cycle de vie

| Situation | Démon | Visible des autres |
|---|---|---|
| GUI ouverte, partage(s) déclaré(s) | enfant de la GUI (`--parent-pid`) | oui |
| GUI fermée, pas de permanence | tué (et auto-terminé si la GUI meurt) | non, port fermé |
| GUI fermée, permanence | unité `systemd --user` | oui |
| Aucun partage | tourne, n'annonce rien | **non** |

Pas de sortie sur inactivité : l'annonce mDNS meurt avec le processus, donc
« vivant » et « visible » sont le même état. Un démon endormi = un poste qui
disparaît, ce qui se lit comme une panne.

## Protocole (version 1)

| Route | Rôle |
|---|---|
| `GET /` | page texte d'aide (dépannage `curl`) |
| `GET /api/info` | `{protocol, peer_id, name, port, shares:[{id,name}]}` — **partages disponibles seulement** |
| `HEAD|GET /api/list/<share>?path=&offset=&limit=&sort=&desc=` | `{entries, parent, truncated}` + `ETag` ; `HEAD` = empreinte seule, `304` si `If-None-Match` correspond |
| `GET /api/find/<share>?q=&path=&limit=` | recherche récursive par nom : `{entries:[{name,path,type,size,mtime}], truncated, scanned}` |
| `GET /api/size/<share>?path=` | taille d'un dossier : `{files, bytes, complete}` |
| `HEAD|GET /f/<share>/<chemin>` | fichier, `Range`/`If-Range`, `ETag`, `Content-Disposition` |
| `GET /a/<share>/<chemin>?as=<nom>` | dossier en flux tar (ustar, chunked) ; `as` impose le nom du dossier dans l'archive |

Invariants :

- **Lecture seule.** Aucun verbe d'écriture, aucun état côté serveur.
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

## Partages : identité, noms, disques externes

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

## File de téléchargement (livrée en M3)

- La file vit dans la GUI (`~/.local/state/gablue-share/queue.json`, écriture
  atomique, sauvegarde à 800 ms au plus), pas dans le démon : fermer la GUI
  arrête les transferts, le démon continue de servir si la permanence est
  active.
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
- **Reprise automatique** : un élément « en attente » est réessayé toutes les
  5 s par un minuteur interne (et non plus seulement sur un événement de
  découverte, qui peut ne jamais venir si le poste reste annoncé). Attente
  croissante 5 s → 10 → 20 → 40 → 80 → 160 → 300 s, plafonnée ; rien ne martèle
  un poste éteint, et un disque remonté repart seul.
- **Pause / reprise** : `en pause` garde le `.part` et sort le transfert du
  worker ; « Reprendre » le remet en file. Un `.part` d'un autre ETag est
  ignoré (le fichier a changé) et laissé tel quel.
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
     `fsync`ée avant, et le démon est lié à la vie de la fenêtre.
- Attente bornée à **1,5 s** : au-delà (poste muet, connexion non annulable),
  on part quand même — la file reprendra au lancement suivant.

## Permanence (M3)

- Vérifié : `systemd-analyze --user verify` accepte l'unité (celle de l'image,
  `files/system/all/usr/lib/systemd/user/gablue-share.service`).
- La vérité est `systemctl --user is-enabled gablue-share.service`, jamais un
  champ du JSON : sinon la case ment dès qu'on touche aux unités à la main.
- Cocher : la GUI arrête son démon éphémère puis `enable --now` ; décocher :
  `disable --now` puis la GUI reprend un démon enfant. Si un service tourne
  déjà, la GUI l'adopte (`Daemon.external`) et ne le tue jamais.
- L'unité est livrée par l'image dans `/usr/lib/systemd/user/` ; `ensure_unit()`
  ne l'écrit dans `~/.config/systemd/user/` que si elle manque (lancement depuis
  le dépôt, image antérieure à M4).
- L'unité est un service **utilisateur** : elle démarre avec la session, pas au
  boot. Pour un poste sans session graphique, `loginctl enable-linger` serait
  nécessaire — hors périmètre.

## Build et développement

```bash
cd src/gablue-share/daemon
cargo build --release      # binaire : target/release/gablue-shared
cargo test                 # 32 tests, aucun réseau externe
```

Développement local :

```bash
mkdir -p /tmp/opencode/partage && echo hello > /tmp/opencode/partage/a.txt
cat > /tmp/opencode/share.json <<EOF
{"peer_id":"dev","name":"dev","port":47823,"shares":[{"id":"s1","name":"Essai","path":"/tmp/opencode/partage"}]}
EOF
./target/release/gablue-shared --config /tmp/opencode/share.json --host 127.0.0.1
curl -s localhost:47823/api/info
curl -s -r 1-3 localhost:47823/f/s1/a.txt      # 206
curl -s localhost:47823/a/s1 | tar -tf -        # archive du dossier
```

Mesures de référence (14/11/2026, loopback) : 2 Gio servis à 3,7 Go/s, RSS
3,0 Mo, 10 threads. À revérifier après un changement de version de `tiny_http`.

GUI sans affichage (validation après une modification) :

```bash
QT_QPA_PLATFORM=offscreen XDG_CONFIG_HOME=/tmp/opencode/cfg \
  ./gablue-share --check     # construit la fenêtre, ne démarre rien
```

Capture réelle sans session graphique : `window.grab().save(...)` sous
`QT_QPA_PLATFORM=offscreen` (voir l'historique de la session du 14/11/2026) —
c'est ce qui a montré le chemin de partage qui débordait sur deux lignes, pas
les tests.

## Décisions et alternatives écartées

- **Rust + `tiny_http`** plutôt que Python : 3 Mo contre 22-30 Mo permanents.
  Mesuré aussi : C sockets nus 1,6 Mo (mais le parsing HTTP et la résolution
  de chemins écrits à la main, sur le seul composant exposé au réseau),
  `axum`+`tokio` 3,9 Mo et 17 threads. Le C ne gagne que 1,4 Mo pour la
  responsabilité de sécurité la plus lourde du projet.
- **HTTP/1.1** plutôt que QUIC/TLS : reprise (`Range`), testable avec `curl`,
  zéro certificat à distribuer ; Tailscale chiffre déjà le trafic du tailnet.
- **Pas de SMB/WebDAV/Syncthing** : authentification ou synchronisation, alors
  qu'on veut « tirer à la demande » sans compte.
- **Pas de HEAD pour les dossiers** : `/a/` est un flux, sa taille n'existe pas ;
  d'où `/api/size`, calculé à la demande et borné.
- **Un champ ignoré fait un réglage mort** : la visibilité vit dans
  `share.json`, mais c'est le démon qui l'applique. Lancée depuis les sources,
  la GUI prend donc le binaire de `daemon/target/release/` avant celui de
  l'image — sinon on teste une interface neuve contre un démon qui ignore le
  champ (vécu 15/11/2026 : l'ancien ordre préférait `/usr/libexec`).
- **Deux comptes sur un même poste** : impossible pour l'instant — le port est
  fixe et le `peer_id` est le `machine-id`, donc le second démon ne peut pas
  écouter et la GUI affiche « port occupé ». Le rendre multi-compte demanderait
  un port dérivé du compte (comme KRdp) et un `peer_id` par utilisateur :
  à faire un jour, documenté ici pour ne pas le redécouvrir.
