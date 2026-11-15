# Gablue Share — paquet `gablue_share` (src/gablue-share)

Partage de dossiers entre postes Gablue, sur le réseau local **et** le tailnet :
chaque poste sert en lecture seule ce qu'il déclare, les autres voient les
postes apparaître seuls et y prennent ce qu'ils veulent. Aucune
authentification, aucun compte : la seule barrière est l'adresse.

Deux processus, séparés pour la RAM :

| Processus | Rôle | Qt | RSS mesuré (15/11/2026) |
|---|---|---|---|
| `gablue-shared` (`daemon/`, Rust) | sert les dossiers en HTTP, annonce le poste, porte l'icône de notification | non | **3,2 Mo** au repos, 3,7 Mo après 50 requêtes puis stable |
| `gablue-share` (GUI PySide6) | découverte, navigation, file de téléchargement | oui | ~70-120 Mo, seulement fenêtre ouverte |

## Documents

| Domaine | Document | À lire AVANT de… |
|---|---|---|
| Démon Rust : protocole HTTP, invariants, fraîcheur/recherche/tailles, visibilité, partages et options côté serveur, cycle du serveur, icône de notification | `daemon/AGENTS.md` | modifier `daemon/src/` ou `daemon/tests/protocol.rs` |
| GUI Python : découverte et navigation, démon vu de la GUI, second compte, style, file de transfert, fermeture, permanence | `gablue_share/AGENTS.md` | modifier `gablue_share/` |
| Vue d'ensemble, cycle de vie, build, décisions | ce fichier | — |

## État

- **M1 (livré)** : démon HTTP, `/api/info`, `/api/list`, `/f/` (Range/ETag),
  `/a/` (tar), filtre d'adresses, rechargement à chaud, annonce mDNS.
  `--parent-pid` est encore accepté mais sans effet depuis M5.3 (démon détaché).
  `cargo test` : 38 tests (11 unitaires + 27 d'intégration, vrai serveur, vrai
  système de fichiers, vrais sockets).
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
  `/usr/bin/gablue-share`, `/usr/libexec/gablue-shared` (861 904 octets à
  l'époque, `ldd` : `libc`/`libgcc_s` seuls, `--help` répond), unité
  `ExecStart` correcte, `.desktop`, icône, service firewalld **et** son
  ouverture dans la zone `FedoraWorkstation`. Première panne du build : un
  cache monté sous `/root/.cargo` — dans une image ostree `/root` est un lien
  vers `var/roothome`, absent (corrigé par `CARGO_HOME=/cargo`, voir
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
- **M5.2 (15/11/2026)** : deux options **par partage**, appliquées côté démon —
  `hidden` (cachés masqués par défaut ; une archive les emporte quand même) et
  `recursive` (faux = seuls les fichiers de la racine sont proposés). 27 tests
  d'intégration + 11 unitaires.
- **M5.3 (15/11/2026)** : **icône et serveur ne font qu'un**. Le tray Python
  (`--tray`, `QLockFile`, sonde, autostart) est supprimé : l'icône est portée
  par le démon (`ksni`), donc elle coûte quelques kilo-octets au lieu de ~50 Mo
  et n'a plus de vie propre. Le démon est lancé **détaché** et survit à la
  fenêtre jusqu'à l'extinction ; le clic sur l'icône bascule la fenêtre. Sans
  dossier déclaré, ni serveur ni icône (arrêt en 0 au démarrage comme au
  rechargement). Deuxième compte du poste : averti et empêché (`uid` dans
  `/api/info`, `Daemon.probe`). Écran vide : arc animé et rescan toutes les 5 s.
- **M5.4 (15/11/2026) : la découverte mDNS fonctionne enfin.** `avahi-browse`
  était lancé **sans `-r`** : il n'émettait que les événements `+` (service vu),
  jamais les lignes `=` résolues, donc `Discovery` n'apprenait aucune adresse et
  **tous** les postes n'existaient que par la sonde Tailscale — étiquette
  « Tailscale » et priorité LAN morte, même sur le même switch. Deuxième cause :
  une sonde partie avec une liste d'adresses périmée supprimait le poste et
  effaçait l'adresse LAN apprise entre-temps (`_drop_if_unchanged`). Vérifié sur
  le vrai réseau : `gen1` (même switch) passe de la seule adresse
  `100.64.0.29` à `{192.168.1.110: lan, 100.64.0.29: tailscale}`,
  étiquette « Réseau local », `active_addr` = 192.168.1.110. Au passage les
  adresses sont classées avec `ipaddress` (plages du tailnet, privé/lien-local/
  ULA) : l'IPv6 publique qu'Avahi annonce n'est plus sondée pour un 403.

## Cycle de vie

| Situation | Démon | Visible des autres |
|---|---|---|
| GUI ouverte, partage(s) déclaré(s) | détaché (`start_new_session`), adopté par la GUI | oui |
| GUI fermée, pas de permanence | **tourne toujours**, jusqu'à l'extinction | oui |
| GUI fermée, « Avec la session » | pareil ; l'unité `systemd --user` prend le relais à la session suivante | oui |
| GUI fermée, « Au démarrage du poste » | pareil ; unité + `loginctl enable-linger` au boot suivant | oui, dès le boot |
| Dernier partage retiré | **s'arrête de lui-même** (exit 0), icône comprise | non, port fermé |
| Aucun partage déclaré au démarrage | **ne démarre pas** (ni démon, ni icône) | non |

Pas de sortie sur inactivité : l'annonce mDNS meurt avec le processus, donc
« vivant » et « visible » sont le même état. Un démon endormi = un poste qui
disparaît, ce qui se lit comme une panne.

**Fermer la fenêtre ne coupe plus le partage** : le démon est lancé détaché et
la GUI le retrouve au lancement suivant par `/api/info` (adoption,
`Daemon.external`). Il ne s'arrête qu'à l'extinction du poste, à la
déconnexion, ou quand le dernier partage disparaît (demandé le 15/11/2026).
Détail des deux côtés : `daemon/AGENTS.md` (« Cycle du serveur ») et
`gablue_share/AGENTS.md` (« Démon vu de la GUI »).

## Build et développement

```bash
cd src/gablue-share/daemon
cargo build --release      # binaire : target/release/gablue-shared
cargo test                 # 38 tests, aucun réseau externe
```

Développement local du démon :

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
- **Deux comptes sur un même poste** : un seul partage à la fois, par
  construction — port fixe, `peer_id` = machine-id. Le second compte est averti
  et empêché (voir `gablue_share/AGENTS.md`), pas laissé dans le faux. Le
  rendre simultané demanderait un port dérivé du compte (comme KRdp), un
  `peer_id` par utilisateur, une **plage** de ports ouverte dans firewalld
  (donc une image reconstruite) et une sonde de ports sur le tailnet,
  `tailscale status` ne donnant que des adresses : écarté le 15/11/2026, le cas
  ne se présente pas et aucune authentification ne séparerait de toute façon les
  deux comptes.
- **L'icône vit dans le démon, pas dans la GUI ni dans un tray séparé** : zéro
  processus en plus, ~3 Ko de code, et l'icône suit exactement la vie du
  serveur. Un tray Python coûtait ~50 Mo (`gablue-manager --tray` : 114 Mo) pour
  une pastille ; un binaire séparé aurait ajouté un propriétaire, un verrou et
  une sonde (choix du 15/11/2026).
