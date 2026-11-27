# Workflows GitHub Actions (.github/) — gablue

> Sous-document du AGENTS.md racine. Couvre .github/ : workflows, composite
> action, dependabot. Contenu de l'ISO (installer/, local-build/, live) →
> installer/AGENTS.md.
> Même règle que la racine : à jour avant tout commit touchant .github/ ou
> les tags de commit.

## gablue-builds.yml

Workflow principal. Déclencheurs : push sur main avec tags (`[main]`, `[nvidia]`, `[dx]`, `[all]`, `[all-iso]`), PRs, **schedule hebdomadaire (vendredi 02:00 UTC)**, workflow_dispatch. Un échec du run planifié est rejoué dans le même run par le job `retry` (voir « Rejeu d'une build planifiée ratée »).

**Cadence : hebdomadaire, pas quotidienne** (modifié le 14/11/2026). Le poste reçoit ses mises à jour par `rpm-ostreed-automatic` **le samedi 04:00** (opt-in, `files/scripts/AGENTS.md`) : une build quotidienne produisait 7 images pour en livrer une, maintenait en permanence une image « en attente » (notification du tray à chaque ouverture de session) et ne laissait que 7 jours d'historique de rollback dans `clean-gablue-images`. La build du vendredi sort donc avant le timer du samedi, et une mise à jour urgente (kernel, Mesa, NVIDIA) passe par un tag de commit ou `workflow_dispatch` — modèle Bazzite (cron hebdo + déclenchement à la demande, cf. leur `build.yml` `cron: "40 4 * * 1"`).

- `paths-ignore` : `.md`/`.txt` ne déclenchent pas de build (les changements `.github/**` déclenchent bien si le tag est présent — jobs filtrés par tag de commit)
- `concurrency` cancel-in-progress : un commit tagué annule la build en cours → pour committer une modif de workflow sans annuler, utiliser `[skip ci]`
- **ISO depuis un push** : plus de `workflow_run` ni de déclencheur `push` dans le workflow ISO — `gablue-builds.yml` l'appelle (`iso` pour `[iso]` seul, `iso-after-images` pour `[all-iso]`, dans le même run). Un déclencheur ne peut pas filtrer sur le message de commit, donc chacun de ces deux déclencheurs créait un run ISO « skipped » à chaque push. Les sous-chaînes ne collisionnent pas : `contains('[all-iso]', '[all]')` et `contains('[all-iso]', '[iso]')` sont tous deux faux

**Jobs** : `build-main`, `build-nvidia` (kernel_type=ogc-lts, nvidia_flavor=nvidia-lts), `build-nvidia-open`, `build-dx` (DX_MODE=true), `build-nvidia-open-dx` (DX_MODE + nvidia-open — restreint à `main`, jamais sur branche `test`), `retry`, `iso`, `iso-after-images`, `update-readme`
- `retry` : rejeu d'une build planifiée ratée — matrice `include` des cinq variantes (mêmes `with:` que les jobs de build, donc **toute nouvelle variante se déclare aux deux endroits**), `needs` sur les cinq builds, `if: failure() && event == 'schedule' && run_attempt == 1`, groupe de concurrence `build-gablue-retry-<image>` **sans** annulation (un rejeu ne doit ni s'annuler lui-même ni tuer la build d'un push plus récent). Cf. « Rejeu d'une build planifiée ratée ».
- `iso` / `iso-after-images` : appellent `build-gablue-live-isos.yml` (donc `reusable-gablue-iso.yml` + `create-release`) dans **ce** run — `[iso]` seul sans `needs`, `[all-iso]` avec `needs` sur les cinq builds et **sans** `always()` (un build en échec saute l'ISO, comme l'ancien `conclusion == 'success'`). D'où les `permissions:` élargies du workflow (`contents: write`, `attestations: write`, `actions: read`) : une chaîne d'appels ne peut pas dépasser les permissions du run de plus haut niveau
- `update-readme` : tableau de versions du README depuis les artifacts `versions-*` (needs sur les 5 builds **et sur `retry`**, ignoré sur PR et branche `test` ; `if: always() && !cancelled() && (retry réussi OU les 5 builds réussis)` — sans `always()`, un run sauvé par le rejeu resterait sans mise à jour du README puisque le `needs` d'origine est en échec). Download via `gh run download` wrappé `nick-fields/retry@v4` (**10×30 s ≈ 4 min 30 s** — tolère le 404 transitoire de l'API artifacts ET les 503 « Egress is over the account limit », incident de stockage GitHub qui dure des minutes : la fenêtre 6×15 s (~90 s) a fait échouer un run dont les 5 builds étaient verts, run 39912731811 du 23/10/2026 ; `on_retry_command: rm -rf versions-data` évite qu'un JSON partiel d'une tentative précédente valide la vérification suivante) ; présence des 5 JSON vérifiée explicitement avec `exit 1` (gh CLI peut retourner 0 malgré un 503 partiel) → le retry se déclenche. Commit `[skip ci]` + push résilient : boucle 5 tentatives avec `git pull --rebase --autostash` entre chaque (absorbe les commits concurrents pendant les ~50 min de build) ; échec du 5e → `exit 1` (pas de succès silencieux)

## Rejeu d'une build planifiée ratée (job `retry`)

Filet de sécurité des builds planifiées : un échec du **run planifié** est rejoué **une fois**, immédiatement, **dans le même run** — job `retry` de `gablue-builds.yml` et de `build-gablue-live-isos.yml`. L'ISO est le cas le plus critique, un build raté laissant un lien BuzzHeavier mort en moins de dix jours.

- **Pourquoi un job, et plus un workflow `workflow_run`** (constaté 15/11/2026) : `workflow_run` crée un run à **chaque** fin de run surveillé — annulé, sauté, manuel compris — et ses filtres ne peuvent vivre que dans le `if:` du job. Chaque completion produisait donc un run fantôme `retry-gablue-builds` (`skipped`, 1 s) : mesuré 5 runs par push sur `main` (images, ISO `push`, chaînage ISO, 3 × retry), 11 runs de rejeu en une journée, tous sautés. Corollaire : une build **annulée** par `cancel-in-progress` déclenchait elle aussi un rejeu potentiel. Supprimer le workflow supprime le bruit **et** le coût d'un run fantôme par completion.
- **Mécanique** : appel direct du workflow réutilisable (`uses: ./.github/workflows/reusable-gablue-image.yml` / `reusable-gablue-iso.yml`) — **NE PAS** remplacer par `gh run rerun` ni par `workflow_dispatch` : un événement émis avec le `GITHUB_TOKEN` d'un workflow n'en déclenche aucun autre (récursion bloquée par GitHub), il faudrait un PAT ou une GitHub App à maintenir. L'input `retry: true` des deux réutilisables distingue cet appel des déclencheurs normaux ; **NE PAS** tester `event_name == 'workflow_run'` pour ça, `build-gablue-live-isos.yml` a déjà un déclencheur `workflow_run` légitime (chaînage `[all-iso]`).
- **Portée volontairement étroite** : seule une exécution **planifiée**, à sa **première tentative**, rejoue (`if: failure() && event == 'schedule' && run_attempt == 1`). Un push tagué raté est vu tout de suite, un « Re-run » manuel est déjà une décision humaine. Un échec déterministe coûte donc une seconde salve (5 images ou 5 ISOs, en parallèle), puis plus rien jusqu'au créneau suivant.
- **`overwrite: true` obligatoire** sur les artifacts que le rejeu republie (`versions-<image>`, `metrics-<image>-iso`, `*-live-link`, `*-live-checksum`) : ils vivent dans le **même run**, et sans écrasement la seconde salve échouerait sur « artifact already exists » alors que l'image est construite.
- **`always()` sur les jobs qui consomment les artifacts** (`update-readme`, `create-release ISO`) : un run sauvé par le rejeu garde le `needs` en échec de la première salve, donc un `if:` ordinaire resterait faux et la release ou le tableau de versions ne serait jamais produit. La condition accepte « rejeu réussi » **ou** « tous les builds d'origine réussis ».
- **Conséquence assumée** : le run reste rouge même quand le rejeu sauve la mise (un job d'origine a échoué, GitHub ne requalifie pas un run). Avant, le rejeu produisait un second run vert séparé ; le rouge existait déjà, il est simplement seul désormais.

## reusable-gablue-image.yml

Workflow réutilisable de build d'une image. **Inputs** : `image_name`/`image_desc`/`image_variant`, `source_image` (kinoite), `fedora_version` (45), `kernel_type` (ogc → KERNEL_FLAVOR), `kernel_version` (défaut hardcodé, surcharge par job), `nvidia_flavor` (optionnel), `containerfile` (optionnel).

**Migration rootless (08/2026, Bazzite `033a18f` + `625bebb`)** : tout sans sudo — buildah/podman/skopeo rootless (storage `~/.local/share/containers`). Plus de loopback BTRFS ici : le rechunk rootless n'a jamais deux images complètes en storage (raw-img supprimée avant le pull oci-archive), pic ~25-30 G << ~110 G libres. Si saturation un jour (variante DX), réintroduire `./.github/actions/mount-btrfs-storage` avec `target-dir: ~/.local/share/containers`.

**Étapes** :
1. Version kernel auto via `skopeo list-tags` si `kernel_version` vide — retry 3×10 s (timeouts ghcr.io)
2. Checkout ; maximisation espace disque
3. Build buildah rootless — retry `nick-fields/retry@v4` (`retry_on: error`, timeout 90 min / 120 min DX — miroirs lents) : le script détecte les erreurs réseau (EOF, TLS handshake, DNS, curl timeout…) **et les dépôts tiers transitoires** (`No match for argument`, `Cannot download`, `Failed to download metadata`, `checksum … mismatch`) → exit 1 (retry) ; erreur de build RUN → exit 2 (échec immédiat)
   - **`No match for argument` est un transitoire, pas un paquet supprimé** : Terra peut servir un repomd partiel pendant une sync — `gablue-main-dx` rouge sur `bpftune-gaming` le 27/11/2026 (métadonnée Terra à 69 Kio au lieu de 1,2 Mio) alors que le job `main` du même commit l'installait 6 min plus tôt et que le paquet était publié. Rejouer coûte 20 s
   - **Ne JAMAIS « corriger » ça avec `--skip-unavailable`** sur les listes de paquets : c'est exactement ce qui a masqué la disparition de `zenergy`/`nct6687d` (bloc akmods) pendant 10 jours. Le retry ne change pas le résultat d'un paquet réellement absent, il ne fait que retarder le rouge de 20 s
   - **`retry_on_exit_code` NE DOIT PAS être utilisé** : désactive le retry sur timeout (bug nick-fields/retry#145)
   - **`set +e -o pipefail` obligatoire** : l'action n'hérite pas du pipefail de GH Actions ; sans lui `$?` capture le code de `tee` à travers le pipe `| tee` → échec de build masqué (l'étape suivante tente `buildah from raw-img` inexistant → pull registres → 404)
   - `buildah rmi raw-img` au début de chaque tentative
4. Labels OCI préparés dans `${RUNNER_TEMP}/labels.txt` (step id `relabel`) — **ne PAS les appliquer via `buildah config`** : le rechunk rootless reconstruit le manifeste de zéro (Bazzite `625bebb`), les labels seraient perdus → passés via `--label` à `build-chunked-oci` à l'étape rechunk
5. SecureBoot check : certificat `/etc/pki/akmods/certs/gablue-secure-boot.der` présent + kmods signés (`modinfo | grep sig_id`) — échec → image ne boote pas en SecureBoot (enroll côté client : `ujust secureboot`)
6. Métriques (durée, disque, taille décompressée raw-img, nb RPMs, kernel, mesa) → JSON `metrics-<image>` (rétention 90 j) + step summary en anglais (commentaires YAML en français)
7. Rechunk rootless rpm-ostree (Bazzite `033a18f`) : nettoyage `/run` + `/tmp` dans un bloc `buildah unshare` (un `buildah mount` rootless exige le user namespace), puis `podman run --privileged` avec `--mount=type=image,src=localhost/raw-img,target=/rpm-ostree`, sortie `--output oci-archive:` dans `${RUNNER_TEMP}`, labels `--label` (1/ligne), `podman rmi -f raw-img` AVANT `podman pull oci-archive:` + tag `localhost/chunked-img`. Retry 3×15 s : abandon immédiat sur erreurs I/O (`no space left`, `read-only`, `disk I/O error`) et rpmdb corrompue (`database disk image is malformed` — présente à chaque tentative, retry inutile) ; retry sur le reste
8. Tag + push GHCR — **retry bash natif** (`for attempt in 1 2 3`, sleep 15) : push tag version via `podman push --digestfile` (préfixe `containers-storage:` retiré, Bazzite `033a18f`)
   - **NE PAS revenir à `skopeo copy` depuis `containers-storage:`** : transport rootless exige un user namespace — bloqué par AppArmor Ubuntu 24.04+ (`Error during unshare(...)`) ; podman a un profil AppArmor dédié, skopeo non
   - Alias tags (`latest`, version, pr-N) : `skopeo copy docker://…@digest → docker://…:tag` APRÈS signature cosign (Bazzite `b97f073`) — chaque alias pointe sur le manifeste signé, sans re-upload (transport remote→remote, pas de user namespace)
   - **Deux logins nécessaires** : `podman login` (push + skopeo, même auth.json) ET `docker login` (cosign lit `~/.docker/config.json` — ne pas retirer)
   - **Pourquoi pas nick-fields/retry@v4 ni wretry.action pour le push** : l'action pipe stdout/stderr via Node.js `spawn()` — 100+ lignes « Copying blob » (image chunkée) saturent le pipe, l'événement exit n'arrive jamais → hang indéfiniment (wretry.action déprécié Node 20). Boucle bash native dans un `run:` hérite du stdio du runner → pas de pipe
9. Signature Cosign (v2.6.1 — bump v3 + `--use-signing-config=false` = chantier séparé, à faire sur ce workflow ET l'ISO en même temps)
10. Métriques post-push : taille compressée réelle via `skopeo inspect --raw` + jq (somme layers + config blob) → JSON final + summary ; upload des métriques APRÈS cette étape

**Version du kernel — cascade à 3 niveaux** (chaque niveau validé contre les tags akmods via `check_akmods`, sinon niveau suivant) :
1. Kernel pinné dans la matrice de la branche main de Bazzite — fetch `raw.githubusercontent.com/ublue-os/bazzite/main/build.py` (retry curl 3×10 s). **Depuis 10-11/2026 la matrice n'est plus dans `build.yml`** : Bazzite l'a déplacée dans `build.py` (table `IMAGES` + dict `DEFAULTS`) ; l'ancien parsing awk ne trouvait plus rien (3 tentatives, ~30 s perdues à chaque build, puis repli sur le niveau 2). Le parseur (Python inline, écrit dans `/tmp`) lit `DEFAULTS` (fedora_version/kernel_flavor/kernel_version), prend le 1er bloc `IMAGES` du flavor voulu (`ogc`/`ogc-lts`), **résout les constantes module-level** (`LTS_KERNEL_VERSION = "6.18.60-ogc1.1.fc45"`), et sort en erreur si `DEFAULTS.fedora_version` ne correspond pas à celle qu'on construit — sinon on adopterait un kernel fc45 pour une image fc44. Suit les kernels préparés en avance par Bazzite
2. Filet de sécurité : kernel réellement publié sur l'image stable Bazzite — label OCI `ostree.linux` via `skopeo inspect` (retry 3×10 s), suffixe `.x86_64` retiré ; source `ghcr.io/ublue-os/bazzite:stable`, ou `bazzite-nvidia:stable` pour nvidia-lts (Bazzite paire nvidia-lts ↔ ogc-lts, commit `3eb7b09` — akmods-nvidia-lts ne publie plus que des tags `ogc-lts-*`). Utilisé si le pin main n'a pas ses akmods (kernel RC trop frais) ou fetch/parsing en échec
3. Fallback : dernière version commune via `skopeo list-tags` (retry 3×10 s par repo) — intersection tags `akmods` et `akmods-{NVIDIA_FLAVOR}` → filtre `{KERNEL_FLAVOR}-{FEDORA_VERSION}-*`, exclusion alias non versionnés (`test("^[0-9]")`), normalisation suffixe `.x86_64` des alias arch, tri `sort -V` (warning dans les logs)
- `check_akmods` : valide `akmods:{FLAVOR}-fc{FEDORA}-{VERSION}` et, si NVIDIA, `akmods-{NVIDIA_FLAVOR}:{…}` (ublue peut arrêter de builder nvidia-lts contre un kernel RC, cf. Bazzite `a5897ab`)
- Manuel : `kernel_version` dans un job → détection skippée

## build-gablue-live-isos.yml

Build des **ISOs live** Plasma complet (tous les 5 jours, 06:00 UTC). **La cadence vient de la rétention BuzzHeavier, pas de la fraîcheur du payload** : la release GitHub `latest-live-iso` ne porte que les `-CHECKSUM`, les ISO ne vivent que sur BuzzHeavier, dont les fichiers libres expirent après 8 jours de base (+2 jours par téléchargement, 60 j max — page tarifs ; la page d'aide annonce 15/+3/45 : les deux sources officielles se contredisent, constaté le 14/11/2026). En hebdomadaire il ne resterait qu'un jour de marge avant que le lien de la release ne meure, et un build raté le tuerait — d'où le `*/5` **conservé** alors que la build d'images est passée en hebdo. Conséquence assumée : une ISO peut embarquer une image vieille de 5 jours (c'est un support live/installation), et `[iso]` ou `workflow_dispatch` en reconstruit une fraîche à la demande. Déclencheurs : schedule, workflow_dispatch et **`workflow_call`** (depuis `gablue-builds.yml`, seul chemin pour un push `[iso]`/`[all-iso]` — l'appelant passe ses `permissions:` et son `github.event` reste `push`, ce qui laisse le job `retry` gardé sur `event == 'schedule'`). Le job `build-live-iso` n'a donc plus de `if:` : on ne l'appelle que quand on veut le construire. Un échec du run planifié est rejoué par le job `retry` (même run).

- **Concurrency ISO** : groupe `build-gablue-live-isos-${{ github.run_id }}-iso` avec `github.run_id` (pas `github.ref`) — chaque run ISO unique, donc une build planifiée, une ISO appelée par un push et un dispatch ne s'annulent jamais entre eux
- **Titanoboa** : installateur bootc générant un squashfs live ; `build_iso.sh` de l'image Titanoboa (`quay.io/fedora/fedora:latest`) patché via `installer/titanoboa_build_iso.sh` (bind-mounté, sans `-all-root` — préserve l'UID 1000 du préfixe Wine)
- 5 variantes : main, main-dx, nvidia, nvidia-open, nvidia-open-dx ; `timeout-minutes: 180`
- **Processus en 2 étapes** :
  1. Image payload via `installer/Containerfile` (FROM image Gablue, flatpaks pré-cachés, swap kernel OGC→vanilla pour Secure Boot). Stockage podman sur le loopback BTRFS (action `mount-btrfs-storage`) **avec `image_copy_tmp_dir` redirigé dans le loopback** (drop-in `/etc/containers/containers.conf.d/`) : par défaut podman copie le layer diff du commit (~30 G non compressés) dans `/var/tmp` **sur l'hôte** → saturation, loopback sparse affamé, BTRFS read-only (les « corruptions » historiques du loopback n'étaient que ce mécanisme). Chemin explicite `/var/lib/containers/image-copy-tmp` plutôt que `"storage"` (résolution buggy, podman#28211). Boucle de build 3 tentatives : **pas de retry** si log contient `no space left on device`/`read-only file system`/`disk I/O error` (non récupérable — tentative identique, loopback read-only mort) → exit immédiat. Optimisations espace (Bazzite `7ecb26a1`) : `extra-squeeze: "true"` sur remove-unwanted-software (~6 G), `TMPDIR=/var/lib/containers/image-copy-tmp` sur le podman build, step `if: always()` affichant `df -h` + taille loopback réelle
  2. Génération ISO via `podman run` direct (remplace l'action `Zeglius/titanoboa`) : script patché bind-mounté sur `/src/build_iso.sh`, payload `localhost/payload:latest` monté `--mount type=image`, sortie bind-mountée `/output`. Image Titanoboa **pré-pullée avec retry** (5×15 s — timeouts quay.io transitoires)
- Signature Cosign + attestation de provenance ; upload BuzzHeavier ; release GitHub `latest-live-iso`
- **Job `create-release`** : `needs: [build-live-iso, retry]` et `if: always() && (build réussi OU rejeu réussi)`. Artifacts liens/checksums via `gh run download` wrappé `nick-fields/retry@v4` (**10×30 s** ≈ 4 min 30 s, `timeout-minutes: 30` sur le job — même durcissement que `update-readme` face aux 503 d'egress GitHub) plutôt qu'`actions/download-artifact` — l'API artifacts peut renvoyer un 404 « workflow run not found » transitoire juste après la fin des jobs ISO (race de propagation interne) ; `on_retry_command` vide `links`/`checksums` entre deux tentatives ; `actions: read` déclaré ; structure de dossiers identique (`merge-multiple: false`)

## reusable-gablue-iso.yml

Build ISO lui-même (matrice des 5 variantes, 20 étapes), extrait de `build-gablue-live-isos.yml` pour être **appelable deux fois dans le même run** : build normal puis rejeu. Il ne porte **aucune condition de déclenchement** — le filtrage appartient à l'appelant (`if:` sur le job `build-live-iso`), sinon un job sauté à l'intérieur d'un workflow appelé ne remonte pas de façon fiable comme `skipped` au job appelant, et `create-release` prendrait un push sans `[iso]` pour un build réussi.

- **Pourquoi l'extraction** : un build ne peut être rejoué dans son propre run que s'il est appelable ; sinon il faut un second run, et c'est ce second run qui imposait le workflow `workflow_run` supprimé (voir « Rejeu d'une build planifiée ratée »).
- **`env` d'un workflow n'est PAS hérité par un workflow appelé** : `REPO_OWNER` (nom en minuscules du propriétaire pour les étiquettes ghcr.io) est déclaré **dans le réutilisable**, pas dans l'appelant — l'oublier casse l'étape de build payload (variable vide).
- **`permissions:`** : copiées dans le réutilisable (`contents: write`, `id-token: write`, `attestations: write`, `packages: write`, `actions: read`) ; un workflow appelé ne peut pas élever au-delà des permissions de l'appelant.
- **`secrets: inherit`** dans l'appelant : le build signe avec `secrets.SIGNING_SECRET`.
- **`overwrite: true`** sur les trois artifacts (`metrics-<image>-iso`, `*-live-link`, `*-live-checksum`) : le rejeu republie les mêmes noms dans le même run. Un rejeu ré-uploadé sur BuzzHeavier laisse un fichier orphelin côté service — même comportement qu'avant, où le rejeu était un run séparé.

## clean-gablue-images.yml

Nettoyage hebdomadaire (dimanche) : images > 90 jours supprimées ; conservation des 7 dernières taggées + 7 dernières non-taggées (avec la build hebdomadaire, ces 7 tags couvrent ~7 semaines de rollback). Packages : gablue-main, gablue-nvidia, gablue-nvidia-open, gablue-main-dx, gablue-nvidia-open-dx, gablue-main-test, gablue-nvidia-open-test

## bump-image-digests.yml

Bump hebdo des images pinnées par digest — mini-ubot maison, équivalent du bot Bazzite ubot-7274 (~1 PR/semaine), mais ouvre une PR au lieu de committer direct.

- **Déclencheurs** : schedule lundi 04:00 UTC + `workflow_dispatch` ; concurrency cancel-in-progress
- **Deps suivies** : tableau `DEPS` dans le step « Vérifier et bumper » — format `"image|fichier"`, une ligne par image pinnée `@sha256:` dans un Containerfile. Ajouter une dep = ajouter une ligne (le pin doit exister dans le fichier, sinon warning + skip)
- **Mécanique** : `skopeo inspect` (retry 3×10 s, timeouts ghcr.io transitoires) → **normalisation du préfixe `sha256:`** (`{{.Digest}}` l'inclut selon la version de skopeo — sans strip, faux positif hebdo et sed corrompu `@sha256:sha256:…`) → comparaison au pin extrait par grep → `sed` si différent. Tout à jour → job sans effet
- **PR** : branche `chore/bump-digests-<date>` (ou réutilise la branche d'une PR bump ouverte, détectée par préfixe) ; la branche est TOUJOURS main + 1 commit — force-push assumé qui supersede le contenu précédent, jamais de conflit de rebase. Force-push sûr : namespace `chore/bump-digests-*` réservé au bot, unique écrivain (concurrency)
- **Validation** : l'ouverture de la PR déclenche les builds des 5 variantes (event `pull_request`) → merge sur CI verte
- **Merge sans build** : commit/PR sans tag de build → rien au merge ; la build planifiée du **vendredi** ingère le nouveau digest — merger la PR en fin de semaine pour qu'elle parte dans la build du soir (sinon jusqu'à une semaine de dérive). Push avec `GITHUB_TOKEN` ne déclenche de toute façon jamais de workflow (récursion bloquée par GitHub)
- **Identité** : `github-actions[bot]` (comme update-readme) ; permissions `contents: write` + `pull-requests: write` ; checkout pinné par SHA (convention repo)
- **Prérequis dépôt** : Settings → Actions → General → Workflow permissions → **« Allow GitHub Actions to create and approve pull requests »** DOIT être coché (réglage `can_approve_pull_request_reviews`). Sinon `gh pr create` échoue : `GraphQL: GitHub Actions is not permitted to create or approve pull requests` (réglage indépendant des `permissions:` du workflow). Réglable par API : `gh api -X PUT repos/elgabo86/gablue/actions/permissions/workflow -F can_approve_pull_request_reviews=true`
- **Branche orpheline** : un run interrompu entre le `git push` et le `gh pr create` laisse une branche `chore/bump-digests-*` sans PR ; le step tombe alors en fallback sur `git ls-remote` pour la réutiliser (sinon nouvelle branche datée à chaque run, anciennes qui traînent)

## Composite action `mount-btrfs-storage`

`.github/actions/mount-btrfs-storage/action.yml` — remplace `ublue-os/container-storage-action` : loopback BTRFS zstd:2 sur `/` + storage podman dessus. `losetup --direct-io=on` + `mount` (évite le double cache page-cache hôte+loop ; remplace `systemd-mount` qui n'expose pas l'option — Bazzite `7ecb26a1`) ; `mkfs.btrfs -m single -d single` (dispositif unique, pas de métadonnées dupliquées).

**Runners `ubuntu-26.04`** depuis 08/2026 (Bazzite `dc41cfb`) — motivé par des corruptions rpmdb récurrentes (`database disk image is malformed`) sur ubuntu-24.04. Si ça persiste → cause ailleurs (rpm-6.0/dnf5 de l'image de base).

**Pourquoi** : les runners ubuntu-24.04 ne montent plus de disque temporaire sur `/mnt` — l'action amont sautait le montage SILENCIEUSEMENT (simple `notice`) → storage sur ext4 sans compression → `no space left on device` sur les gros payload (ISO flatpaks, rechunk DX).

| Input | Défaut | Description |
|-------|--------|-------------|
| `target-dir` | `/var/lib/containers` | Répertoire placé sur le loopback BTRFS |
| `loopback-free` | `0.9` | Fraction de l'espace libre de `/` (fichier sparse, occupation = contenu compressé) |
| `mount-opts` | `compress-force=zstd:2,discard=async,noatime` | Options de montage |

**Contrainte** : action locale `uses: ./.github/actions/mount-btrfs-storage` — dépôt checkouté avant l'appel.

**Utilisation** :
- ~~`reusable-gablue-image.yml`~~ plus utilisée depuis la migration rootless — conservée comme garde-fou si une variante sature un jour (pointer sur `~/.local/share/containers`)
- `build-gablue-live-isos.yml` : libérer espace → checkout → mount → drop-in `image_copy_tmp_dir` → build payload → podman run Titanoboa (script patché)

## Épinglage des actions

- **Actions GitHub épinglées par SHA** : toutes les actions tierces sont pinnées par SHA (avec commentaire `# vX.Y.Z`) pour la sécurité supply-chain, mises à jour par Dependabot (daily)
