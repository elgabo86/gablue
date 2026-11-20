# glauncher

Launcher de jeux pour [Gablue](https://github.com/gablue-os) : il liste les
paquets `.wgp` et `.lgp` de tes dossiers avec leurs visuels (cover, héros, logo,
icône) et les lance en plein écran (`gwine` / `lgplaunch`). Pendant le jeu, la
fenêtre reste noire plein écran pour cacher le bureau ; au retour, on revient à
la liste des jeux.

Clavier et manette (Xbox / PlayStation) : les glyphes du pied de page suivent le
dernier périphérique utilisé. Le launcher ignore clavier et manette quand sa
fenêtre n'est pas active.

## Lancer

```bash
cargo run --release              # plein écran, dossiers de config.toml
cargo run -- --windowed          # en fenêtre
./test.sh                        # tes jeux, plein écran (binaire release)
./test.sh play 12                  # lance vraiment le jeu n° 12
./test.sh list                    # liste numérotée des jeux
./test.sh index cold             # chrono du scan de disque
```

Options : `--config <fichier>`, `--dir <dossier>` (répétable), `--columns <n>`,
`--cell <pixels>`, `--windowed`, `--simulate [secondes]`, `--play <n>`,
`--no-covers`, `--reindex`, `--help`.

## Dossiers scannés

Dans `config.toml` (ou `~/.config/glauncher/config.toml`). **Aucun dossier
n'est scanné par défaut** : la liste doit être écrite (Gablue Manager →
Lanceur de jeux le fait pour toi) ; une liste vide ne montre aucun jeu.

```toml
game_dirs = ["~/Games", "/run/media/gab/t5/home00/gab/Games"]
```

Ils sont **surveillés** : un paquet ajouté ou supprimé met la liste à jour tout
seul, sans relancer le launcher — il n'y a donc pas de bouton « Rafraîchir ».
Le nombre de jeux s'affiche en bas, avec les commandes.

Trois vues, en cycle avec V / Tab ou Y : **grille** (`view = "grid"`),
**liste** avec vignette et grand aperçu (`"list"`) et **mosaïque** façon Switch
Online (`"mosaic"`). Le **tri** se choisit dans son propre menu (`T` ou
Carré/X) : **A→Z**, **Récents** ou **Favoris**. Le **dernier type de vue et le
dernier tri** sont retenus d'une session à l'autre (`<cache>/state.txt`) ;
`view` dans `config.toml` ne sert que de valeur par défaut au premier lancement. La vignette sélectionnée porte un cadre
d'accent et un fond teinté, en grille comme en liste ; la **roulette** de la
souris fait défiler librement (le défilement manuel devient la position de
référence, la liste ne se recale pas sur la sélection). La navigation au
clavier ou à la manette, elle, fait défiler vers la sélection de façon animée.

## Page d'un jeu et favoris

Entrée (ou A) ouvre la page du jeu sélectionné : héros en bande, logo, cover, nom
et temps de jeu, puis trois pastilles — **Jouer**, **☆ Favori**, **Images ▸**
(ni chemin du paquet, ni état des visuels : la page est une affiche). Elle
apparaît en fondu ; sans héros téléchargé, la bande ne réserve rien et le bloc
se centre sur
la page — quand l'image arrive, la bande grandit et se pose en fondu au lieu de
faire sauter le contenu. Échap/B
referme en gardant la sélection **et** la position de défilement. Les quatre visuels se
changent depuis un seul bouton : `Images ▸` propose Grid / Héros / Logo / Icône
(avec `✓` pour ceux déjà présents), puis le menu d'images du type choisi.

L'étoile marque le jeu comme favori (fichier
`~/.local/share/glauncher/favorites.txt`, un slug par ligne). Les vignettes des
favoris portent une étoile ; le tri **Favoris** (menu `T`) ne montre qu'eux.

Le démarrage n'ouvre **aucun paquet** : noms et chemins viennent de l'index,
les visuels du cache local, et le voisinage des paquets (icônes posées à côté,
souvent sur un disque externe qui dort) est lu au second plan, plus tard. La
première image sort en ~0,1 s, même disque froid.

Le parcours est **récursif mais borné** : `scan_depth = 3` par défaut (0 =
illimité) et `scan_skip` ignore les dossiers de données (`Prefixes`, `Content`,
`drive_c`…). L'index SQLite (`~/.local/share/glauncher/index.sqlite`) rend le
démarrage quasi instantané : 688 paquets sur un disque externe en ~40 ms à
froid, ~16 ms ensuite. `--reindex` reconstruit l'index sans fenêtre.

## Covers

Les visuels manquants sont téléchargés **pour les jeux affichés seulement**
(SteamGridDB, API publique sans compte ; `steamgriddb_key` ou `$SGDB_API_KEY`
pour l'API v2 officielle). Tout est rangé sous
`~/.local/share/glauncher/assets/` : `grids/`, `heroes/`, `logos/`, `icons/` —
et **rien d'autre**, ce dossier ne contient que des images du jeu : tu peux le
partager tel quel entre machines (Syncthing, par exemple). Miniatures (`thumbs/`),
icônes extraites des paquets (`pack-icons/`) et marqueurs (`.miss`, `.choice`,
sous `marks/`) vivent à côté du dossier, pas dedans, et les fichiers sont
écrits en `.part` + rename (jamais d'image à moitié copiée en transit).
L'ancienne disposition (`covers/`, `icons/`) est reprise automatiquement au
démarrage.

Le téléchargement suit ce que tu regardes : la file des covers de fond est
remplacée à chaque déplacement, et le menu d'image (ci-dessous) la suspend
entièrement le temps du choix. En vue grille, liste ou mosaïque, seules la
cover et l'icône sont demandées ; **ouvrir la page d'un jeu** demande ses
quatre visuels en priorité absolue (d'où le héros et le logo qui arrivent en
quelques secondes). Un jeu traversé trop vite n'est jamais « oublié » : ses
demandes abandonnées redeviennent demandables dès qu'on revient dessus.

Les vignettes affichent une **miniature** de 320 px (`assets/thumbs/`),
générée une seule fois en tâche de fond : `iced` décode les images sur un seul
thread, et une cover pleine (600×900) coûtait ~3,2 ms contre ~0,55 ms pour sa
miniature — une vue très dézoomée se remplit donc d'un coup au lieu de
s'étaler. Changer d'image refait la miniature, et les réglages (largeur,
qualité) sont versionnés pour purger un cache devenu obsolète.

Le jeu est mal reconnu par SteamGridDB ? Dans le menu d'images, **Changer le
nom** ouvre un champ pré-rempli (et présélectionné) : corrige, Entrée, la
recherche repart avec ce nom pour le type courant — et le nom est retenu pour
les autres visuels de la session.

Une image ne te plaît pas ? Sur la page, `Images ▸` (ou `I`) choisit le type,
et le menu propose les vignettes SteamGridDB **dans l'ordre du top** ; Entrée/A
applique. Le choix est écrit dans le cache (`<slug>.<ext>` + `<slug>.choice`)
et n'est **jamais** remplacé automatiquement. Pour les icônes, celle du paquet
est proposée en tête du menu. Dans le menu **Grid**, seules les images au
format portrait sont proposées (SteamGridDB range aussi des carrés et des
bannières dans ce type).

## Icônes

L'icône d'un jeu est cherchée dans cet ordre :

1. l'icône choisie à la main (SteamGridDB ou paquet) ;
2. `<nom du paquet>.png` à côté du paquet ;
3. `icons/<slug>.png` à côté du paquet (`Skyward Odyssey.wgp` → `skyward-odyssey.png`) ;
4. `<slug>.png` dans les `icon_dirs` (`~/.local/share/icons` par défaut) ;
5. `.icon.png` **extrait du paquet** (`unsquashfs -cat`, ~15 ms, mis en cache
   dans `<cache>/pack-icons/`, hors du dossier partagé) ;
6. à défaut, l'icône SteamGridDB.

Sans aucune icône, une vignette colorée avec l'initiale s'affiche.

Chaque icône trouvée est copiée dans le cache (`pack-icons/` pour celles
extraites d'un paquet) et n'en est jamais supprimée : elle survit à la
disparition du jeu ou du dossier. Le dossier `assets/`, lui, ne garde que les
images SteamGridDB.

## Jeux et disques qui disparaissent

Un paquet supprimé sort de la liste tout seul. Un dossier injoignable (disque
externe débranché) est signalé « dossier introuvable » mais reste dans la
config : ses jeux réapparaissent dès que le disque est remonté.

## Musique de fond

Le launcher joue une musique en boucle : une piste **embarquée** dans le
binaire (`assets/music/menu.opus`, Opus 96 kb/s, 1,6 Mo), recopiée une fois
dans le cache puis lue par `mpv` (`--no-config --no-video --ab-loop-a=0.133
--ab-loop-b=115.289 --gapless-audio=yes`). Elle monte en fondu au démarrage, **s'efface pendant le
voile noir** du lancement (0,9 s) et revient en fondu au retour du jeu — donc
jamais par-dessus une partie, et `mpv` est arrêté, pas mis en pause.

La boucle est **musicale, pas un raccord** : la piste est calée sur 100 BPM
(mesure de 2,4 s) et fait exactement 48 mesures ; `mpv` boucle de la première
note (0,133 s) à la fin de la 48ᵉ mesure (115,289 s). L'intro est donc jouée en
entier au démarrage, puis chaque tour repart sur la première note — rien n'est
mélangé ni décalé, et le morceau n'est pas coupé au milieu d'une phrase. Seule
retouche du fichier : la queue morte de l'export Suno (51 ms à −67 dB) est
retirée, avec 8 ms de fondu de garde anti-clic. `--gapless-audio=yes` garde la
sortie audio ouverte entre deux tours (sans lui : 31 ms de blanc par
bouclage) ; vérifié sur la sortie réelle, aucune plage silencieuse aux
jonctions. Coût : ~2 à 3 % d'un cœur et ~85 Mo résidents, mesurés.

Coupée par `music = false` dans `config.toml` (case « Jouer une musique de
fond » de Gablue Manager → Lanceur de jeux), et son volume se règle au curseur
« Volume » à côté (`music_volume`, 0 à 100, 60 par défaut ; 0 = muet et aucun
`mpv` lancé). Sans `mpv`, le launcher reste simplement muet.

## Poussière d'étoiles

Le fond est vivant : une poussière scintillante dérive devant les vignettes
(façon fond de PS3), 130 particules environ, trois familles (poussière fine,
étoiles moyennes, grands halos pâles). Elle **ne coûte rien au repos** : le
minuteur (20 images par seconde) est coupé dès que la scène n'est pas visible
(fenêtre sans focus, page de jeu, menu, prompt) ; chaque image est un
`canvas` transparent dont les particules sont des fonctions pures du temps —
aucune position stockée, aucune allocation par image.

Le titre de la vignette sélectionnée défile quand il ne tient pas dans la
vignette (pause, descente jusqu'au dernier caractère, pause, retour) ; les
autres restent tronqués. Une petite **roue de chargement** apparaît dans le coin
haut droit pendant les téléchargements de visuels, et seulement là.

La vignette sélectionnée **respire** : la vignette entière — fond de sélection,
cover, tranche, étoile et titre — grossit de 0 à 2,8 % (période de 4 s), sans
rien déplacer autour ; le cadre d'accent, lui, entoure la cover seule et suit
le même mouvement. Chaque cover a les coins légèrement cassés, porte une fine
tranche sombre et une ombre portée : la vignette se lit comme un boîtier posé.

Au lancement d'un jeu, l'hyperespace (les particules filent vers l'extérieur)
s'accompagne d'une **plongée de la page** : elle grossit de 6 % comme si on
entrait dans l'écran, pendant que le voile monte. Au retour, chaque vignette
revient **de son côté de l'écran** en dézoomant (**titre compris**), les
vignettes du bord partant de plus loin et se posant les dernières. Les pages de
jeu, elles, s'enchaînent par un **fondu par le fond** (montée courte, bascule,
descente) et les visuels qui arrivent après coup (héros, logo, cover)
apparaissent en fondu.

Le pied de page porte l'aide des touches en **Lato** (police embarquée, plus
ronde que la sans-serif système), une pastille de tri lisible à gauche et le
total de la bibliothèque en pastille à droite (le nombre de favoris en tri
Favoris). `T` (ou Carré/X) ouvre le menu du tri — A→Z, Récents, Favoris — et un
changement de tri **ramène en début de liste**.

Réglages : `sparkles = false` dans `config.toml` coupe tout, et
`GLAUNCHER_FX_MS=80` (16 à 200) ralentit le minuteur sans recompiler
(`GLAUNCHER_FX_DEBUG=1` journalise les ms par image).

## Contrôles

| Action      | Clavier         | Manette              |
| ----------- | --------------- | -------------------- |
| naviguer    | flèches / WASD  | croix + stick gauche |
| changer de page | Page↑ / Page↓ | L1 / R1          |
| ouvrir le jeu | Entrée / Espace  | A (Xbox) / ✕ (PS)       |
| grille/liste/mosaïque | V / Tab | Y (Xbox) / Triangle (PS) |
| choisir l'image | C           | Select / Share          |
| menu du tri | T                | X (Xbox) / Carré (PS)   |
| plein écran | F / F11            | —                       |
| quitter     | Échap / Q          | retour ×2 (1,5 s)        |

Sur la page d'un jeu :

| Action      | Clavier         | Manette              |
| ----------- | --------------- | -------------------- |
| choisir une action | ← →      | croix gauche/droite |
| valider     | Entrée / Espace    | A                    |
| favori      | F                  | Select / Share       |
| visuels     | I (direct : G/H/L/C) | Y                  |
| retour      | Échap              | B                    |

## Plein écran des jeux

Beaucoup de jeux `gwine` / `lgplaunch` s'ouvrent dans une simple fenêtre. Au
lancement, le launcher charge un petit script KWin (D-Bus) qui force l'attribut
« plein écran » de KWin sur les fenêtres du jeu, puis le décharge quand le jeu
se termine. Les dialogues et le launcher ne sont pas concernés.

Wine reparente ses processus à systemd : la fenêtre du jeu n'est pas un
descendant du lanceur. Le launcher vise donc aussi les processus qui partagent
la session du jeu (celle que `bwrap --new-session` ouvre pour gwine) — jamais la
session du bureau, pour ne pas mettre toutes les fenêtres en plein écran.

Le plein écran n'est imposé qu'à l'apparition de la fenêtre : retiré ensuite
(`F`, overlay manette), il reste retiré. Pendant la fraction de seconde où le
jeu n'est pas encore plein écran, l'écran noir du launcher reste la fenêtre
active — Plasma ne cache ses panneaux que dans ce cas, et sans ça la barre du
bas réapparaissait entre l'écran noir et le jeu.

## État

Grille, liste, mosaïque, tri (dont favoris), page de jeu (héros / logo / grid / icône),
lancement plein écran, manettes, surveillance des dossiers. Suite prévue :
recherche, dernier jeu lancé, pointeur masqué pendant un jeu.
