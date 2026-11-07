# gablue-dualsense — firmware des manettes DualSense / Edge

Deux pièces, aucune ne demande de privilège :

| Élément | Rôle |
|---|---|
| `dualsense-ctl.c` → `/usr/bin/gablue-dualsense-ctl` | C + hidapi : lit le rapport `0x20`, la batterie, et flashe (rapports `0xF4`/`0xF5`). Compilé par `files/scripts/build-c` (`hidapi-devel`). |
| `files/system/all/usr/bin/gablue-dualsense` | Bash : commandes `status`/`check`/`update`, téléchargement Sony, détection de famille, confirmation. Stocké 644, `+x` posé par `post-install`. |

L'accès à `/dev/hidraw*` vient des règles udev déjà présentes
(`ublue-os-udev-rules` → `71-sony-controllers.rules`, `steam-devices` →
`60-steam-input.rules`, `MODE=0660 TAG+="uaccess"` sur `054c:0ce6`/`0df2`).
Ne jamais passer par sudo/pkexec : il n'y a rien à élever.

## Faits vérifiés sur matériel (07/11/2026)

Manette DualSense `054c:0ce6`, firmware `0x0630`, `hardware_info=0x0513`,
build `Jul  4 2025 10:10:32`, batterie 65 %.

- **La manette accepte l'en-tête START d'une autre famille** quand la version
  est plus récente : un en-tête 000B patché en `0x0640` et un 000E `0x0641`
  ont été acceptés sur cette manette 0004. **Ne jamais déduire la famille de
  l'acceptation du START** : une écriture avec la mauvaise image est un vrai
  risque de brick.
- START renvoie **`0x02`** quand la version proposée n'est **pas strictement plus
  récente** : 0004 `0x0630` (identique) refusé, 0004 `0x0520` (antérieure,
  test réel 07/11/2026) refusé aussi. Un `0x02` ne dit donc **rien** de la
  famille. Le statut F5 reste à `0x02` après un refus jusqu'au rebranchement
  (inoffensif, `info` répond toujours) ; rebrancher avant de réessayer.
- START sur en-tête accepté renvoie **`0x01` en boucle** (jamais `0x00`) : c'est
  « prêt à recevoir », pas une erreur. `dualsensectl` 0.7.0 attend `0x00` et
  **bloque** (`Checking firmware header…` sans fin) sur ce matériel — d'où notre
  propre implémentation qui considère `0x01` comme accepté après 5 s.
- **Écriture/vérification/finalisation non validées sur matériel** au 07/11/2026 :
  la seule manette disponible est à la dernière version et refuse tout ce qui
  n'est pas plus récent, donc aucun chemin d'écriture n'a pu être exercé. La
  boucle reprend celle de `dualsensectl` à l'octet près ; à revalider au premier
  firmware plus récent (surveiller `info.json`).
- La **date de compilation** reportée par la manette vient de l'image en cours
  d'exécution : identité à la seconde avec l'en-tête de la bonne famille
  (0004 = `10:10:32`, 000B = `10:38:40` pour la même version `0x0630`). C'est
  le seul discriminant fiable et il ne fonctionne que si la manette est déjà à
  la dernière version (seules les dernières images sont sur le CDN Sony).
- Le CDN Sony ne sert que la **dernière** version de chaque famille
  (`info.json` : `FwUpdate0004LatestVersion` etc.) ; les archives vivent dans
  le dépôt Paliverse (hachés, utiles pour un test de downgrade).
- URL : `fwupdate<FAMILLE>/<0xVERSION>/FWUPDATE<FAMILLE>.bin` — **hexadécimal en
  majuscules** dans le chemin (`fwupdate000B`), `Range: bytes=0-255` supporté
  (récupération d'en-tête sans télécharger 950272 octets).

## Détection de famille (ordre du script)

1. `--target` fourni → imposé.
2. Comparaison de la **date + heure de compilation** reportées par la manette
   avec l'en-tête de chaque famille candidate : d'abord la version courante
   sur le CDN Sony si c'est la dernière de la famille, sinon l'archive
   Paliverse (Range `0-255`, 256 octets). Une correspondance à la seconde,
   avec la même version et la même cible, identifie la famille de façon
   certaine.
3. Table `variants_of()` — repli quand aucun en-tête ne peut être récupéré
   (hors ligne, dépôt d'archives indisponible) : les versions communes à
   0004/000B (`0520`, `0630`) y restent indéterminées.
4. Sinon : message explicite, `--target` obligatoire (le numéro BDM-0xx sous la
   manette ou l'historique des versions tranche).

**Maintenance** : à chaque nouvelle version publiée, ajouter l'ancienne
dernière version à `variants_of()` (table à jour depuis
`Paliverse/DualSense-List-of-Firmwares` → `firmwares.json`). Une version
absente de la table et plus récente que toutes les images connues = `--target`.

## Invariants du flash

- USB obligatoire (protocole refusé en Bluetooth), batterie ≥ 10 % (le C
  refuse).
- Taille d'image exacte 950272 octets ; PID à `0x62` = PID de la manette ;
  cible à `0x74` et version à `0x78` lues dans l'en-tête et affichées.
- Écriture par blocs de `0x8000`, paquets de 57 octets (rapport de 64),
  attente du statut phase `0x01` après chaque paquet ; vérification phase
  `0x02` (longue, jusqu'à 180 s) ; finalisation `0x03`.
- La manette se réénumère après finalisation : le script attend qu'elle
  réponde et confirme la nouvelle version.

## Tester

```bash
# état + famille (sans flash)
gablue-dualsense status
# simulation complète hors matériel : binaire local
GABLUE_DUALSENSE_CTL=/chemin/gablue-dualsense-ctl gablue-dualsense status
```

Test de bout en bout du flash : impossible tant que la manette est à la
dernière version — le downgrade est refusé (`0x02`, vérifié avec 0004 `0x0520`
sur une manette `0x0630`). Le premier vrai test sera une version plus récente
publiée par Sony : `gablue-dualsense check`, puis `update` (la détection de
famille par date de compilation ne sert plus dans ce cas ; c'est la table
`variants_of()` qui tranche, d'où l'importance de la tenir à jour).
