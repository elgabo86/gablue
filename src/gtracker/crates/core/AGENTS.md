# gtracker-core — moteur de GTracker

Contrat du **moteur audio multitrack** (pistes, clips, tempo maître, tonalité).
Le reste du crate (catalogue, décodage, boucle, export d'une piste) est décrit
dans `../../AGENTS.md` : ce document ne le répète pas et n'autorise pas à le
contredire.

## Modules

| Fichier | Rôle |
|---|---|
| `mixer.rs` | horloge de transport, plan de voix, mix temps réel (`rodio::Source`) et rendu hors temps réel |
| `stretch.rs` | WSOLA (durée) + resampling (hauteur) |
| `key.rs` | chroma et tonalité (Krumhansl-Schmuckler) |
| `project.rs` | modèle sérialisable, validation, dossiers, nommage |
| `peaks.rs` | pyramide de formes d'onde multi-résolution |
| `decode.rs`, `tempo.rs`, `convert.rs` | **réutilisés tels quels** : aucune deuxième analyse du rythme, aucun deuxième décodeur |

## Invariants du moteur

- **Une seule horloge** : la position vit en trames de sortie ; les beats ne sont
  qu'une projection via `tempo_bpm`. Deux horloges (GUI + audio) = dérive visible
  et clips qui « glissent ».
- **Tout le projet est en beats** (`at_beat`, `length_beats`, fades, boucle).
  Seul le contenu d'un clip est en trames de sa source (`src_start`, `src_end`) :
  c'est du matériau, pas du temps.
- **Le ratio d'un clip se déduit du clip**, jamais du BPM estimé de la source :
  `ratio = (src_end - src_start) / (length_beats × beat_frames)`. C'est ce qui
  empêche une erreur de BPM de 0,1 % de décaler la fin d'un clip de plusieurs
  mesures. Le BPM de la source ne sert qu'à **créer** le clip (longueur par
  défaut) et à afficher.
- **Démarrage à la trame exacte** : une voix démarre à la trame arrondie de son
  `at_beat`, calculée une fois, pas à la frontière de bloc — sinon flam de
  jusqu'à 5 ms entre deux clips censés être ensemble.
- **Aucun trou, jamais** : le mixer avance toujours ; une voix qui finit libère sa
  place et ne rend pas `None` pour tout le monde. Même règle que `LoopSource`.
- **Chemin audio propre** : pas d'allocation, pas d'I/O, pas de décodage. Seul le
  **remplacement du plan** (reconstruit côté GUI) se fait sous verrou court :
  l'audio clone un `Arc<Plan>` et repart. Le verrou ne doit jamais entourer un
  calcul.
- **Rendu hors temps réel = même code** : `render()` pousse les mêmes trames que
  le temps réel, avec le même plan. Un test compare les deux sorties
  échantillon par échantillon ; sans ça, l'export ne prouve rien sur la lecture.
- **Sortie stéréo f32** à la fréquence du périphérique : mono dupliqué, plus de
  2 canaux moyennés (jamais un canal perdu en silence).
- **Gain général** (`Project.master_gain`) et **métronome** sont appliqués dans
  `Plan::build` : le métronome est une voix dont le PCM est un bip, donc il passe
  par le même mixage que la musique (aucun étage audio en plus). L'export part
  d'une copie du projet avec `metronome = false` et `loop_beats = None` : on
  exporte le projet, pas la séance d'écoute.
- **Latence compensée** : la position exposée à la GUI est retardée de la latence
  de sortie, sinon on juge un clip 20–40 ms après sa position réelle.
- **Sources épinglées** : le PCM d'une source utilisée par le projet ouvert ne
  peut pas être évincé du cache RAM (`decode::pin`/`unpin_all`) ; une éviction
  silencieuse couperait la lecture en plein milieu. Plafond par défaut 1024 Mo
  (`GTRACKER_RAM_MB`), l'éviction saute les sources épinglées et s'arrête si
  tout est épinglé (mieux vaut dépasser que couper).

## BPM : qui suit qui

- `Project.tempo_bpm` est la **seule** vérité du transport.
- Choisir une piste maîtresse **pose** `tempo_bpm = bpm effectif de sa source`,
  une fois. Ensuite le badge BPM du transport édite `tempo_bpm`. Re-switcher de
  maîtresse repose depuis la nouvelle : c'est le comportement demandé
  (« on choisit une principale et on peut switcher quand on veut »).
- Une piste `Suit` ne modifie jamais le tempo ; son ratio est recalculé à chaque
  changement de `tempo_bpm` ou de longueur de clip.
- **Une seule maîtresse**, pas de graphe de dépendances, pas de cycle. Une chaîne
  A→B→C n'apporte rien qu'un ratio manuel ne fasse déjà.
- Tempo constant dans un projet (musique de jeu) : pas d'automation en v1.

## Repitch vs Stretch

- **`Repitch` (défaut)** : on change la vitesse de lecture, la hauteur suit de
  `12·log2(ratio)` demi-tons. Gratuit, zéro artéfact, comportement tracker/MPC —
  c'est le mode juste pour du chiptune.
- **`Stretch`** : durée et hauteur indépendantes. WSOLA **hors temps réel**, par
  clip, mis en cache (clé = source + plage + ratio + demi-tons) et borné ; la voix
  lit ensuite un buffer ordinaire, donc le mixage ne connaît pas le stretch. Un
  WSOLA temps réel par voix serait plus élégant et beaucoup plus risqué.
- Le pitch **induit** par le repitch est affiché sur le clip et **compensé** par
  la case « même gamme ». On ne fait jamais comme s'il n'existait pas.

## Tonalité

- Analyse **par tranche**, pas par piste : une intro de 4 mesures n'est pas dans
  la tonalité du refrain. `key::analyse(pcm, ch, rate, range)` ; le résultat est
  stocké dans `Clip.key` à la création du clip et prime sur `Source.key`.
  `Source.key` n'est qu'un repli (clip importé d'un ancien projet).
- Chroma (STFT maison, 4096/2048) → corrélation aux profils majeur/mineur de
  Krumhansl → `Key { tonic, mode, confidence }`.
- Transposition = plus court chemin entre toniques (±6 demi-tons), option
  « aligner le mode » (relatif majeur/mineur).
- **Limite assumée** : on transpose un clip en bloc, on ne replacera jamais
  chaque note dans la gamme. Une phrase reste juste, un accord interne reste un
  accord. La réharmonisation note à note demanderait une séparation de sources et
  sonnerait pire sur un rendu de jeu.
- Case décochée : badge de compatibilité par distance au cycle des quintes
  (0/±1 compatible, ±2 tendu, au-delà conflit). L'analyse se trompe sur du modal :
  la tonique est réglable à la main (même philosophie que `bpm_manual`).

## Projet (persistance)

- Emplacement : `util::music_dir()/GTracker/projets/<nom>.gtrack`, JSON + `serde`,
  champ `version`. **Jamais** `~/.cache` : un projet est un document utilisateur,
  il survit au nettoyage du cache et au re-téléchargement.
- Une source est référencée (`site`, `entry_id`, `file`, `subsong`, BPM, tonalité),
  jamais copiée. Source absente à l'ouverture = clip en erreur visible, pas un
  crash et pas une suppression silencieuse.
- **Un clip déposé est gelé** : re-découper la source ou changer sa grille ne
  déplace plus la composition. C'est l'invariant qui protège le travail.
- Écriture atomique (`.part` puis renommage) et un seul écrivain par fichier.

## Tests et outils headless

- `cargo test -p gtracker-core` doit couvrir : ratio/longueur exacts, mixage
  d'une somme connue, rendu offline déterministe et égal au temps réel,
  aller-retour JSON (et rejet de version inconnue), tonalité sur signaux
  synthétiques (La mineur, Do majeur, gamme chromatique = confiance basse),
  WSOLA (durée exacte, pas de clic aux jointures, pitch préservé à ±X cents).
- Vérification sans interface : un exemple `mixcheck` rend un projet de test en
  WAV et imprime durée, crêtes et position des attaques ; `exportcheck` va
  jusqu'au FLAC écrit sur disque et relu par `ffprobe` (durée, canaux, clip au
  temps exact, métronome exclu). **Aucune validation ne
  se fait à l'œil sur une capture d'écran.**

## Pièges à ne pas réintroduire

- Un clip dont `src_end ≤ src_start` après rognage doit être supprimé, pas laissé
  à longueur nulle (voix silencieuse invisible à l'écran).
- Un clip peut dépasser son voisin sur la même piste : c'est **autorisé et mixé**,
  sans fondu croisé automatique (l'utilisateur pose ses fades). Interdire le
  chevauchement oblige à écraser des données au glisser — inacceptable.
- Ne pas réutiliser `Decoded.peaks` (2400 tranches fixes, échelle du morceau
  entier) pour dessiner un clip : à 3 s zoomées, on n'a que du bruit. D'où
  `peaks::Wave` (pyramide) et des textures par clip.
- Ne pas « corriger » le BPM d'une source pour faire tomber la fin du clip juste :
  la longueur du clip est la vérité, le tempo de la source une indication.
