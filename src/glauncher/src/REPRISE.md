# Reprise de session — ambiance du fond de glauncher

> **Fichier de passage** : à supprimer dès que tout est en tête (ou à la fin du
> chantier). Les idées à plus long terme sont dans `IDEES.md`, à côté.

État au 20/11/2026, soir. Le travail a été fait dans `src/glauncher/` (et
`src/gablue-manager/` pour le volume de la musique).

## Où on en est

- **Commité (local, pas poussé)** : `f0e1c2ee feat(glauncher): sparkling dust
  over the interface` — la poussière d'étoiles v1 + doc.
- **Non commité** (arbre de travail), après une salve de retours d'écran :
  - `src/sparkles.rs` : hyperespace au lancement, respiration **cadre compris**,
    tranche du boîtier, **deux géométries** (tracés avant images dans iced : la
    cover mangeait la tranche pendant le zoom) ; **gerbe retirée** ;
  - `src/app.rs` : `slot_rect`, `App::canvas_frame`, tranche + ombre
    dans `card`, `CARD_INSET` (boîtes plus petites), `SCROLLBAR_GUTTER` (marge
    de droite), fondu **enchaîné** de la page (bascule au sommet), apparition
    animée des visuels (héros, logo, cover), **plongée** de la page au
    lancement (`float`), **retour des vignettes depuis les bords** (`comeback`),
    pied de page (statistiques retirées, aides en Lato, pastille de tri, nom du
    jeu à droite) ; `App::breathing` devient le **seul peintre** de la cover
    sélectionnée (`Breath::owns_art`) ;
  - `src/music.rs` + `src/config.rs` : `music_volume` (0 à 100, défaut 60) ;
  - `src/gablue-manager/…` : curseur « Volume » dans le dialogue Lanceur de jeux
    (`dialogs/glauncher.py`, `local.py`, `tests/test_pure.py`) ;
  - `AGENTS.md`, `README.md`, `src/IDEES.md`, `src/REPRISE.md`.
- **Vérifié** : `cargo build --release` sans warning, `cargo test --release`
  (36 tests), manager 225 tests + dialogue instancié hors écran. **Rien n'a été
  regardé à l'écran** : c'est le seul vrai juge, et le prochain point.

## Ce qui est en place

1. **Poussière d'étoiles** — 3 familles, dérive + scintillement `sin³`,
   parallaxe 10-18 %, ~132 particules en 1920×1080, minuteur 20 Hz coupé dès
   que la scène n'est pas visible.
2. **Hyperespace au lancement** — pendant le voile noir (0,9 s) : poussée
   radiale + traînées, pilotée par `fade`. Une couche `warp` le redessine
   **au-dessus de la page** (les jeux se lancent depuis leur page, opaque).
3. **Respiration de la sélection** — la vignette entière (cadre, cover,
   tranche, étoile) grossit de 0 à 2,8 % en 4 s autour de son centre ; le canvas
   peint aussi le cadre de sélection (`App::canvas_frame`).
4. **Tranche + ombre** sur toutes les vignettes de grille/favoris.
5. **Boîtes plus petites** : `CARD_INSET` 9 px entre la case et la boîte, donc
   plus d'air à nombre de colonnes constant ; marge de droite
   (`SCROLLBAR_GUTTER` 16 px) pour ne plus passer sous la barre de défilement.
6. **Transitions de la page** : fondu **enchaîné par le fond** (montée 0,12 s,
   bascule au sommet, descente 0,20 s — plus de saut au noir), et apparition en
   fondu des visuels qui arrivent après l'ouverture (héros + bande, logo,
   cover ; 0,30 s).
7. **Effets de lancement / retour** : la page **grossit** de 6 % au lancement
   (`PAGE_DIVE` via `Pose`, un agrandissement ne découvre aucun bord,
   contrairement au recul essayé avant), et au retour **chaque vignette revient
   de son côté de l'écran en dézoomant, titre compris** (`App::comeback` :
   `RETURN_SPREAD` 55 %, `RETURN_ZOOM` 10 %, `RETURN_STAGGER` 45 %).
8. **Pied de page** : aides des touches centrées en **Lato embarquée**, pastille
   de tri à gauche (bordure d'accent, « Récents » avec majuscule), total de la
   bibliothèque en pastille à droite, ni nom du jeu ni « covers 12/540 ».
   `T` (ou Carré) ouvre un **menu « Tri »** (A→Z, Récents, Favoris), comme le
   menu des vues ; un changement de tri remet en début de liste. Une touche
   **maintenue** ne rejoue plus son action (auto-repeat ignoré).
9. **Page d'un jeu** : plus de chemin de paquet ni d'état des visuels
   (`Grid ✓ · Héros …`), seulement logo, nom, temps de jeu et les trois
   pastilles — la page est une affiche.
10. **Vues et favoris** : les favoris ne sont plus une 4ᵉ vue mais un **tri**
   (menu ci-dessus) ; la respiration grossit la vignette entière (fond bleu,
   cover, tranche, étoile, titre) via `Pose`, le canvas ne peint que le cadre
   d'accent autour de la cover ; les covers ont des coins légèrement arrondis.
11. **Musique** : volume réglable (défaut 60, contre 45 avant), curseur dans
   Gablue Manager → Lanceur de jeux ; 0 = pas de `mpv` du tout.
12. **Titre trop long** : la sélection défile (pause / descente / pause /
   retour) dans une bande recadrée (`Marquee`, largeur naturelle mesurée au
   rendu) ; les autres vignettes restent tronquées.
13. **Année de sortie** : « Sorti en 1998 » sous le titre (le temps de jeu
   reste en dessous), apprise par `Query::Year` et gardée dans
   `assets/years.txt` (partagé par Syncthing, `slug=` = « pas de date »,
   jamais redemandée, `--update` la remplit pour tout le monde, `--date` ne
   fait que ça).
   **Filtre d'année** dans le menu Tri (une année par ligne, avec son nombre de
   jeux), retenu dans `state.txt`.
14. **Roue de chargement** : coin haut droit, visible pendant les
   téléchargements (`covers::busy`). Deux corrections après l'essai raté : un
   emplacement **permanent** dans la pile racine (sinon la liste remonte en
   haut) et un **réveil** au début de chaque requête (`covers::wake_with`),
   sinon elle n'apparaissait qu'une fois le téléchargement fini — donc jamais.
   Elle ne tourne plus que pour ce qu'on regarde : la file de fond est en pause
   tant qu'une page de jeu est ouverte.

## La contrainte de coût (à ne pas casser)

Dans iced, **chaque message reconstruit la vue et redessine toute la fenêtre**
(pas de redessin partiel). Donc : minuteur à 20 Hz (`FRAME_MS`,
`GLAUNCHER_FX_MS`) pour l'ambiance, `window::frames()` (60 Hz) réservé aux
transitions courtes (fondu, défilement, page), et **aucune mise en page par
image** hors des transitions de page assumées (fondu de page, arrivée du héros).
`GLAUNCHER_FX_DEBUG=1` journalise les ms par image de canvas.

## À vérifier à l'écran (dans cet ordre)

1. **Respiration** : cadre + cover grossissent ensemble, pas de double image
   derrière, pas de cadre fantôme ; la **tranche ne disparaît plus** pendant le
   zoom (c'était le bug du 20/11) ; rien ne touche la vignette au-dessus.
2. **Espacement** : taille des boîtes et air entre elles (`CARD_INSET`, `GAP`) ;
   la colonne de droite ne touche plus la barre de défilement.
3. **Page** : fondu enchaîné (pas de saut au noir), et surtout **arrivée des
   visuels** sur une page dont les images n'étaient pas encore téléchargées
   (vider `<cache>/heroes/<slug>.png` pour l'obtenir à coup sûr) : bande qui
   grandit, logo et cover en fondu.
4. **Lancement / retour** : lancer un jeu depuis sa page — la page doit
   **plonger** (grossir) sous le voile pendant l'hyperespace, sans jamais
   montrer de bord — puis quitter le jeu : **chaque vignette revient de son côté
   de l'écran en dézoomant, titre compris**, bords en dernier. Le démarrage du
   launcher ne doit PAS faire l'effet de retour.
5. **Pied de page** : aides centrées (Lato), pastille de tri lisible, nom du
   jeu à droite, rien qui se chevauche sur une fenêtre étroite ; `T` renvoie en
   haut de liste.
6. **Musique** : volume par défaut (60) et curseur du manager (0 = muet).
7. **CPU** : `top -p $(pgrep -x glaucher)`, puis `sparkles = false` pour l'écart.

## Pièges et décisions déjà prises

- **Deux géométries dans le canvas** : iced dessine **tous les tracés avant
  toutes les images** dans une même géométrie. La cover (image) passait donc
  par-dessus la tranche et le cadre (tracés) : le boîtier perdait son arête
  pendant tout le zoom. `draw` rend `vec![images, tracés + particules]`.
- **Le cadre ET la cover de la sélection sont peints par le canvas** en
  grille/favoris quand `sparkles = true` : la carte garde l'épaisseur
  `FOCUS_BORDER` mais la peint en transparent, et garde **la place** de l'image
  sans la dessiner (`Breath::owns_art`). Le canvas peint l'image à **tous** les
  facteurs — deux copies qui se remplacent faisaient réapparaître la cover d'un
  coup au minimum de la respiration (constaté 20/11). Pendant le retour d'un
  jeu, le canvas s'efface (`breathing()` = `None`) : les vignettes glissent
  (`comeback`), il ne peut pas les suivre.
- **Ne pas redécouper le zoom au rectangle de l'image** : c'était la v1, elle
  laissait voir la cover non zoomée aux bords.
- **Le canvas n'est pas découpé par la zone défilante** : il découpe lui-même
  (`PAGE_PADDING_Y` → `FOOTER_BAND`, `intersect`).
- **Tranche** : `sparkles::SPINE_FILL` / `SPINE_EDGE` / `spine_width` partagés
  par la vignette et le canvas — mêmes valeurs des deux côtés.
- **Hauteur de carte imposée** (`row_height()`) : la hauteur réelle (texte de
  titre) divergeait de quelques pixels et le cadre du canvas dérivait de rangée
  en rangée. Même logique pour `slot_height()` avec `CARD_INSET`.
- **Page** : voile unique `max(fade, page_fade)`, bascule **au sommet** du
  fondu (`pending_page`) ; l'arrivée des visuels est la seule mise en page par
  image assumée. Au lancement, `Pose` agrandit la page de 6 % (`PAGE_DIVE`) :
  une transformation de rendu, donc aucune mise en page ni bord visible ; au
  retour, `Pose` replace chaque vignette (décalage latéral + dézoom).
- **Wayland/décorations** : ne rien toucher (`decorations: true` fixe,
  `application_id = "glauncher"`).
- **Mosaïque et liste exclues** de la respiration, de la tranche et de l'ombre.
- **Ordre des couches** : `stack![liste, poussière, page, hyperespace, voile,
  menus]` — nombre d'enfants **constant**.

## Reste à faire

1. Valider à l'œil et ajuster les constantes (`GAP` 20, `CARD_INSET` 9,
   `SCROLLBAR_GUTTER` 16, `PAGE_FADE_RISE` 0,12 / `PAGE_FADE_FALL` 0,20,
   `PAGE_ASSET_SECONDS` 0,30, `PAGE_DIVE` 0,06, `RETURN_SPREAD` 0,55,
   `RETURN_ZOOM` 0,10, `RETURN_STAGGER` 0,45, `RETURN_END` 0,15,
   `music_volume` 60) — le seul vrai juge.
2. **Case « Particules » dans Gablue Manager** : `sparkles = false`, sur le
   modèle de la case musique — `gablue_manager/dialogs/glauncher.py`,
   `local.py` (`GLAUNCHER_DEFAULTS["sparkles"] = True`), `tests/test_pure.py`.
3. **Transition de page plus ambitieuse** : fondu enchaîné pour l'instant ; la
   piste proposée (zoom héros : la cover rejoint sa place dans la page,
   tranche qui s'épaissit, souffle de poussière) attend le feu vert.
4. Puis les idées d'`IDEES.md` (halo, aurore, parallaxe accentuée, teintes,
   météores).
5. Commit + push quand c'est validé : message en **anglais**, pas de tag de
   build.

## Commandes

```bash
cd /var/home/gab/Syncthing/Data/gits/perso/ublue/gablue/src/glauncher
cargo build --release            # binaire → ~/.cache/opencode/cargo-target/release/glauncher
cargo test --release             # 36 tests, dont 5 sur les particules
~/.cache/opencode/cargo-target/release/glauncher --config /tmp/opencode/gui/config.toml --windowed
```

Config de test : `/tmp/opencode/gui/config.toml` (recréée le 20/11 avec
`demo-games/`, `music = false`, `covers = false`, `sparkles = true`).

## Vérifications de fond, déjà faites

- Audio (musique) et fenêtre : voir les commits `cf76a524` / `1410d376`.
- Le build `[all]` poussé le 20/11 (`3cd29a33`) : statut des workflows GitHub
  non revérifié depuis.
