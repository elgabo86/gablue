# kwin — scripts KWin de l'overlay (info de fenêtre, actions jetables)

Les scripts de `kwin/` contiennent le jeton `__BUS__` : le nom de service propre
au processus est substitué au chargement, dans
`/tmp/opencode/<script>-<pid>.js`. Rôle de chaque script : table des composants,
`AGENTS.md` racine. Page « fenêtres », filtre et icônes côté panneau :
`gpad/AGENTS.md`.

## Comment l'info arrive

KWin **n'expose pas** la fenêtre active sur D-Bus (`getWindowInfo` demande un
UUID, `queryWindowInfo` est interactif). On charge donc un script KWin qui, sur
`workspace.windowActivated` et toutes les 300 ms, appelle `callDBus` vers
`org.gablue.Win.p<pid> /win` — un petit service zbus que l'overlay héberge. Le
script est chargé au démarrage de l'overlay et **déchargé à la sortie** (son
`QTimer` part avec lui ; sur SIGTERM, `ARRET_DEMANDE` déclenche `request_close`).

Le **nom de service est propre au processus** (`org.gablue.Win.p<pid>`) et les
scripts sont écrits dans `/tmp/opencode/<script>-<pid>.js` : deux overlays
peuvent tourner en même temps (celui de `gpad-trigger` et un de banc) sans se voler
le service ni les scripts. Les fichiers livrés dans `kwin/` contiennent le jeton
`__BUS__`, remplacé au chargement ; **l'interface D-Bus reste `org.gablue.Win`**
(la remplacer aussi casse l'appel).

Ces copies **s'accumulaient** : une instance tuée au `SIGKILL` (banc, test)
n'efface jamais son fichier — 40+ `wininfo-*.js` / `lister-*.js` retrouvés dans
`/tmp/opencode` (17/11/2026). `nettoyer_scripts_temp()` passe au démarrage et
supprime celles dont le **pid n'existe plus** (`/proc/<pid>` absent) : jamais
celles d'une instance vivante, puisque deux overlays coexistent. Il reste le
fichier de la dernière instance, enlevé au lancement suivant.

Actions de fenêtre : KWin seul peut modifier les propriétés de fenêtre sur
Wayland → scripts jetables chargés/exécutés/déchargés (`loadScript(path, nom)`,
`Scripting.start()`, `unloadScript(nom)`).

**Cyclage L2/R2 : script généré, pas de fichier dans `kwin/`.** L'ancien couple
`cycle-prev.js`/`cycle-next.js` (supprimé) s'ancrait sur
`workspace.activeWindow` dans `stackingOrder` : le panneau peut être la fenêtre
active (`indexOf` → -1, donc toujours la même cible) et surtout **activer une
fenêtre la remonte dans `stackingOrder`** — au coup suivant, « précédente »
rebondissait sur les deux dernières fenêtres au lieu de parcourir la pile
(constaté à l'usage). Le script est donc écrit à chaque appui
(`/tmp/opencode/kwin-cycle-<pid>.js`, `CYCLE_JS` dans `gpad/src/main.rs`)
avec deux paramètres :

- `cible` = **UUID de la fenêtre active**, que l'overlay connaît par `wininfo`
  (`w.internalId` poussé par `wininfo.js` — c'est pourquoi la signature D-Bus
  `Info(...)` a gagné un 6e argument : JS et Rust doivent bouger ensemble) ;
- ordre de parcours **stable** : `frameGeometry` (gauche→droite, puis
  haut→bas), départage par `internalId`. Jamais `stackingOrder`.

Repli si l'UUID est inconnu : `workspace.activeWindow`, puis bord de liste.
Mesuré : L2 ×4 puis R2 ×4 parcourent 4 fenêtres et reviennent exactement sur
leurs pas (`CYCLE -1`/`CYCLE 1`, 0 échec).

## Pièges des scripts KWin (mesurés le 16/11/2026)

- **`Script{id}.run()` est un no-op silencieux** quand le chemin D-Bus
  `/Scripting/Script<id>` désigne encore un script précédent : KWin réutilise
  les ids (`const int id = scripts.size()`, un `unloadScript` détruit en
  différé) et `Script::run()` sort sans rien faire si `running()` ou
  `m_starting` est vrai — aucune erreur D-Bus. Symptôme : `loadScript` rend un
  id, `isScriptLoaded` est vrai, la réponse de `run` est OK, et **le script ne
  s'exécute jamais** (aucun `console.info`). Utiliser **`Scripting.start()`**,
  qui parcourt la vraie liste et ignore ceux qui tournent déjà.
- **Un nom de plugin KWin n'est pas libre avant la destruction différée** :
  recharger sous le même `pluginName` juste après `unloadScript` rend
  l'**ancien** script (ou `-1`). Générer un nom unique par chargement
  (`gablue-<rôle>-<pid>-<n>`).
- **Le premier `loadScript` au tout début du processus** n'exécute pas toujours
  le script ; l'overlay vérifie donc qu'un `Info` arrive et réessaie sous un nom
  neuf (la preuve est l'état, pas le code de retour D-Bus).
- Un appel D-Bus raté depuis un script peut arrêter le script **avant** la
  création de son `QTimer` : `gabluePush` est sous `try/catch`.
- **`/tmp/opencode` est en tmpfs : le dossier n'existe pas au démarrage.**
  `script_instance()` le crée avant d'écrire (`create_dir_all`) et journalise
  l'échec ; sans ça l'écriture échouait en silence, le service D-Bus
  `org.gablue.Win.p<pid>` tournait sans jamais recevoir d'`Info`, et le
  panneau affichait ACTIONS ~600 ms puis APPLICATIONS (grâce de `page_initiale`)
  avec un en-tête toujours vide — vécu gen1 20/11/2026, `/tmp/opencode`
  n'apparaissait qu'au premier `lister.js`.
- Le service doit posséder son nom **avant** le chargement du script
  (`request_name` dans le thread appelant, pas dans le thread de service).
