# Idées d'ambiance pour l'interface

Notes de travail : ce qui est fait, ce qui reste, et **pourquoi** certains
choix ont été écartés. Toutes ces idées vivent dans `src/sparkles.rs` (le
`canvas` transparent posé au-dessus de la liste, sous la page) sauf mention.

## La contrainte qui décide de tout : le CPU

Dans iced, **chaque message provoque une reconstruction de la vue et une image
complète** (aucun redessin partiel). Le coût d'une animation est donc
directement sa fréquence, et deux règles en découlent :

1. **20 Hz maximum** (`FRAME_MS`, `GLAUNCHER_FX_MS` pour juger sur place) ;
   ne jamais brancher une animation d'ambiance sur `window::frames()` (60 Hz),
   réservé aux fondus et au défilement.
2. **Aucune mise en page par image** : rien qui change une taille, un texte ou
   une position dans l'arbre de widgets (ça relance la mise en page de la
   grille entière). Tout ce qui bouge est dessiné dans le `canvas`.

Le minuteur est **coupé** (`sparkles::set_running`, recalculé à chaque
`subscription()`) dès que la scène ne peut pas être vue : fenêtre sans focus,
page de jeu, menu, prompt, écran noir. `sparkles = false` supprime le fil.

## Fait

- [x] **Poussière d'étoiles** — 3 familles (poussière fine, étoiles moyennes,
      grands halos), dérive + scintillement `sin³`, parallaxe au défilement.
- [x] **Hyperespace au lancement** — pendant le voile noir, chaque particule
      est poussée vers l'extérieur depuis le centre (`fade²`) et doublée de
      deux traînées : elles filent vers les bords puis s'éteignent sous le
      voile. Au retour du jeu, le même calcul joué à l'envers les fait revenir.
- [x] **Zoom « respiration » de la vignette sélectionnée** — le canvas redessine
      **la vignette entière** 0 à 2,8 % plus grande (période 4 s) : cadre
      d'accent, cover, tranche et étoile grossissent ensemble autour du même
      centre. Aucune mise en page refaite (tout est dans le canvas). Grille et
      favoris seulement.
- [x] **Tranche et ombre du boîtier** sur toutes les vignettes de grille :
      bande sombre au bord gauche de la cover + arête claire, et ombre portée
      sous la vignette. La cover se lit comme un boîtier posé.

## À faire, dans l'ordre où je les prendrais

1. **Halo qui respire** derrière la vignette sélectionnée : un disque flou
   (le sprite `sparkle.png` très agrandi, opacité 0,05) qui pulse sur 3 s,
   centré sur la vignette. Même canvas, même minuteur → coût nul. À dessiner
   *sous* la respiration de la cover.
2. **Aurore lente** : deux ou trois grands dégradés radiaux très pâles qui
   dérivent sur 30 s derrière la grille. Redessinés une fois toutes les 2 s
   (un compteur, pas une image sur deux) → quasi gratuit, et ça donne de la
   profondeur au fond plat `BG`.
3. **Parallaxe accentuée** : la poussière suit le défilement à 10-18 % ;
   monter à 35 % pour un vrai effet de profondeur. Une constante par famille
   dans `fleck()`.
4. **Particules teintées** : deux sprites pré-teintés au démarrage (chaud
   22 h-7 h, froid sinon) à partir du PNG embarqué (`image` est déjà une
   dépendance). Coût nul, aucune image par image.
5. **Météores rares** : une particule sur ~1200 prend une trajectoire filante
   pendant 0,8 s et retombe. Purement une branche de `fleck()`.
6. **Case « Particules » dans Gablue Manager** → Lanceur de jeux, comme la
   musique (`sparkles = false`). ~10 lignes dans
   `gablue_manager/dialogs/glauncher.py` + `local.py`.
7. **Reflet qui passe sur le titre / le pied de page** toutes les 20 s :
   joli, mais c'est du **texte** à redessiner (et la mise en page suit) —
   à ne faire que si le coût mesuré le permet.

## Écarté (et pourquoi)

- **Gerbe d'étincelles au changement de jeu** : implémentée puis retirée le
  20/11 sur retour d'usage (« l'effet d'explosion quand je vais sur un jeu
  c'est nul »). Trop démonstratif pour un déplacement qui se fait sans arrêt ;
  ne pas la réintroduire sans demande explicite.
- **Zoom de la cover seule, découpée au rectangle de l'image** : c'était la v1.
  Le cadre restait figé et, comme la copie grossie était découpée à l'ancien
  rectangle, la cover non zoomée restait visible aux bords — deux images l'une
  derrière l'autre, et le zoom semblait déborder de son cadre. Corrigé en
  grossissant la vignette entière.

- **Zoom de la cover par la taille de l'image** (`width/height` × 1,03) :
  iced n'a ni transformation ni découpe de widget, l'image déborderait sur le
  texte et les voisines, et chaque image relancerait la mise en page.
- **Respirer dans la mosaïque** : les covers se touchent et le voile de
  sélection (22 % d'accent) est posé **au-dessus** de la cover dans la
  vignette ; le canvas le recouvrirait. Il faudrait redessiner ce voile aussi.
  Possible plus tard, pas prioritaire.
- **Respirer en vue liste** : vignette de 40-90 px, l'effet ne se voit pas.
- **Animation à 60 Hz** (`window::frames()`) : trois fois le coût pour une
  dérive lente qui n'en a pas besoin.
- **« Covers qui respirent » toutes ensemble** : relance la mise en page de
  chaque vignette à chaque image, exactement ce qui fait un launcher qui
  chauffe.
