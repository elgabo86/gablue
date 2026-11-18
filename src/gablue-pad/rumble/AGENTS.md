# rumble — retour haptique (seule confirmation qui ne se filme pas)

`rumble [node] [ms] [force]` : impulsion `FF_RUMBLE`, 140 ms / 60 % par défaut,
détection automatique de la manette quand le node n'est pas donné.

La vibration est le **seul retour qui ne passe ni par l'écran ni par l'audio**,
donc le seul qui ne peut pas se retrouver dans le buffer (tout ce qui est
affiché ou joué est capturé). Sur succès de `save`, `replay.sh` lance
`rumble/rumble` ; en cas d'échec, **aucune vibration** — le silence haptique
signifie « raté », ce qui serait ambigu si on vibrait aussi pour échouer.

`PAD_NODE` désigne la manette qui a déclenché, et les **trois** chemins le
transmettent — sinon c'est la première manette trouvée qui vibre :

| Chemin | Transmission |
|---|---|
| panneau ouvert (Home+gauche) | `lancer_script_env("REPLAY", …)` ; le node est mémorisé **avant** `release_pad()` |
| `gpad-trigger` (panneau fermé) | `PAD_NODE='<home_node>' replay.sh save` |
| ligne de commande | `PAD_NODE=/dev/input/eventN ./replay.sh save` |

Mesuré : `RUMBLE /dev/input/event14 140ms 60%` puis fichier écrit ; panneau →
`STUB arg=save PAD_NODE=/dev/input/event13` ; `gpad-trigger` → `STUB_PADWATCH
arg=save PAD_NODE=/dev/input/event13`.

Deux pièges de détection FF (DS4 en BT, kernel 7.3.4-ogc4.1.fc45, 16/11/2026 —
revérifier avant d'appliquer) :

- **`EVIOCGBIT(EV_FF, 8)` renvoie zéro alors que la manette vibre** : les codes
  d'effets commencent à `0x50` (bit 80), donc un bitmap de 8 octets (64 bits)
  ne peut rien retourner. Il faut au moins 12 octets — `rumble.c` demande
  `FF_MAX/8+1`. Sur ce pad : `FF_RUMBLE`, `FF_PERIODIC`, `FF_SQUARE`,
  `FF_TRIANGLE`, `FF_SINE`, `FF_GAIN`.
- **`EVIOCGEFFECTS` renvoie 0 et pourtant `EVIOCSFF` réussit** : ne pas s'en
  servir pour décider si la vibration est possible, tester l'upload.
