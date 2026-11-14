# Gablue Share — paquet `gablue_share` (src/gablue-share)

Partage de dossiers entre postes Gablue, sur le réseau local **et** le tailnet :
chaque poste sert en lecture seule ce qu'il déclare, les autres voient les
postes apparaître seuls et y prennent ce qu'ils veulent. Aucune
authentification, aucun compte : la seule barrière est l'adresse.

Deux processus, séparés pour la RAM :

| Processus | Rôle | Qt | RSS mesuré (14/11/2026) |
|---|---|---|---|
| `gablue-shared` (`daemon/`, Rust) | sert les dossiers en HTTP, annonce le poste | non | **3,0 Mo** au repos et pendant un transfert (pic 3,0 Mo) |
| `gablue-share` (GUI PySide6, à venir) | découverte, navigation, file de téléchargement | oui | ~70-120 Mo, seulement fenêtre ouverte |

## État

- **M1 (livré)** : démon HTTP, `/api/info`, `/api/list`, `/f/` (Range/ETag),
  `/a/` (tar), filtre d'adresses, rechargement à chaud, `--parent-pid`,
  annonce mDNS. `cargo test` : 24 tests (8 unitaires + 16 d'intégration, vrai
  serveur, vrai système de fichiers, vrais sockets).
- **M2 (livré)** : GUI PySide6 (liste des dossiers partagés persistante,
  découverte LAN/tailnet, navigation, téléchargement fichier + dossier).
  Vérifié en headless : capture réelle de la fenêtre, reprise d'un `.part`
  (500 Kio → 2 Mio, hash identique), extraction tar, collision `(1)`.
- **M3** : file persistante (reprise après fermeture, rattachement par
  `peer_id`), bascule permanente, gablue-manager.
- **M4** : image (build Rust en étape dédiée, unités systemd, firewalld, icône).

## Paquet (GUI)

Sources = ce dossier, installées par `files/scripts/build-share` (M4).

| Fichier | Rôle |
|---|---|
| `gablue-share` | lanceur `/usr/bin` (sys.path puis `gablue_share.app.main`) |
| `gablue_share/app.py` | `main()`, `--check` (construit la fenêtre et sort, sans démon) |
| `gablue_share/config.py` | **les dossiers partagés** : `share.json`, écriture atomique, fichier corrompu mis de côté |
| `gablue_share/daemon.py` | démarrage/arrêt du démon, adoption d'un démon permanent |
| `gablue_share/discovery.py` | registre des postes, mDNS (avahi-browse) + sonde Tailscale |
| `gablue_share/api.py` | client HTTP du protocole (threads) |
| `gablue_share/downloads.py` | transfert fichier (reprise) et dossier (tar), `.part` |
| `gablue_share/window.py` | trois panneaux : mon partage, postes/navigation, transferts |
| `gablue_share/style.py`, `util.py`, `constants.py` | QSS dérivé de la palette, formatage, chemins |

## Découverte — règles non négociables

- **Le user ne saisit jamais un nom de machine.** Aucun champ « ajouter un
  poste », aucune adresse à taper : la liste se remplit et se vide seule.
- **Un poste n'apparaît que s'il a au moins un partage.** Le démon ne publie
  son annonce mDNS que si `shares` est non vide ; à la suppression du dernier
  partage, l'annonce est retirée (le poste disparaît des listes). Côté
  Tailscale, la sonde lit `/api/info` et écarte les `shares: []`.
- Identité = `peer_id` (machine-id), **pas** le nom d'hôte ni l'adresse : un
  poste joignable en LAN et en Tailscale est une seule entrée, adresse LAN
  préférée, Tailscale en secours.

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
| `GET /api/info` | `{protocol, peer_id, name, port, shares:[{id,name}]}` |
| `GET /api/list/<share>?path=&offset=&limit=` | `{entries, parent, truncated}` |
| `HEAD|GET /f/<share>/<chemin>` | fichier, `Range`/`If-Range`, `ETag`, `Content-Disposition` |
| `GET /a/<share>/<chemin>` | dossier en flux tar (ustar, chunked) |

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

## File de téléchargement (M2 partiel, M3 pour la persistance)

Déjà en place (M2) : un transfert à la fois, `.part` dans le dossier de
destination, reprise `HEAD` → ETag → `Range` + `If-Range` (200 = fichier
changé, on repart de zéro), annulation coopérative, dossier en tar.

Reste à faire (M3) : la file survit à la fermeture de l'app et se rattache
seule au poste retrouvé.

- La file vivra dans la GUI (`~/.local/state/gablue-share/queue.json`,
  écriture atomique), pas dans le démon : fermer la GUI arrête les transferts,
  le démon continue de servir si la permanence est active.
- Un élément est identifié par `(peer_id, share_id, chemin relatif)`, **jamais
  par une adresse** : au redémarrage, la découverte retrouve le poste (LAN ou
  Tailscale, l'adresse a pu changer) et la reprise repart seule.
- Reprise : `HEAD` pour l'ETag → même ETag = `GET Range: bytes=<taille .part>-`
  avec `If-Range` ; 200 = le fichier a changé, on repart de zéro ; 416 = il a
  rétréci, idem.
- États : `en attente du poste` (hors ligne : on garde, on ne supprime jamais),
  `en attente du partage` (partage retiré), `en pause` (choix user), `en
  cours`, `terminé`, `échec` (3 essais, 2/5/15 s).
- Le `.part` (`.nom.<etag8>.part`) reste dans le dossier de destination ; les
  orphelins de plus de 7 jours sont nettoyés au démarrage.

## Build et développement

```bash
cd src/gablue-share/daemon
cargo build --release      # binaire : target/release/gablue-shared
cargo test                 # 23 tests, aucun réseau externe
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
- **Pas de HEAD pour les dossiers** : `/a/` est un flux, sa taille n'existe pas.
