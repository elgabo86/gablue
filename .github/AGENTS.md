# Workflows GitHub Actions (.github/) — gablue

> Sous-document du AGENTS.md racine. Couvre .github/ : workflows, composite
> action, dependabot. Contenu de l'ISO (installer/, local-build/, live) →
> installer/AGENTS.md.
> Même règle que la racine : à jour avant tout commit touchant .github/ ou
> les tags de commit.

## gablue-builds.yml

Workflow principal. Déclencheurs : push sur main avec tags (`[main]`, `[nvidia]`, `[dx]`, `[all]`, `[all-iso]`), PRs, schedule quotidien (02:00 UTC), workflow_dispatch.

- `paths-ignore` : `.md`/`.txt` ne déclenchent pas de build (les changements `.github/**` déclenchent bien si le tag est présent — jobs filtrés par tag de commit)
- `concurrency` cancel-in-progress : un commit tagué annule la build en cours → pour committer une modif de workflow sans annuler, utiliser `[skip ci]`
- **Chaînage ISO (`[all-iso]`)** : déclenche les 5 variantes (comme `[all]`), puis le workflow ISO via `workflow_run`. Les sous-chaînes ne collisionnent pas : `contains('[all-iso]', '[all]')` et `contains('[all-iso]', '[iso]')` sont tous deux faux → pas de build ISO immédiat au push

**Jobs** : `build-main`, `build-nvidia` (kernel_type=ogc-lts, nvidia_flavor=nvidia-lts), `build-nvidia-open`, `build-dx` (DX_MODE=true), `build-nvidia-open-dx` (DX_MODE + nvidia-open — restreint à `main`, jamais sur branche `test`), `update-readme`
- `update-readme` : tableau de versions du README depuis les artifacts `versions-*` (needs sur les 5 builds, ignoré sur PR et branche `test`). Download via `gh run download` wrappé `nick-fields/retry@v4` (6×15 s — tolère le 404 transitoire de l'API artifacts) ; présence des 5 JSON vérifiée explicitement avec `exit 1` (gh CLI peut retourner 0 malgré un 503 partiel) → le retry se déclenche. Commit `[skip ci]` + push résilient : boucle 5 tentatives avec `git pull --rebase --autostash` entre chaque (absorbe les commits concurrents pendant les ~50 min de build) ; échec du 5e → `exit 1` (pas de succès silencieux)

## reusable-gablue-image.yml

Workflow réutilisable de build d'une image. **Inputs** : `image_name`/`image_desc`/`image_variant`, `source_image` (kinoite), `fedora_version` (44), `kernel_type` (ogc → KERNEL_FLAVOR), `kernel_version` (défaut hardcodé, surcharge par job), `nvidia_flavor` (optionnel), `containerfile` (optionnel).

**Migration rootless (08/2026, Bazzite `033a18f` + `625bebb`)** : tout sans sudo — buildah/podman/skopeo rootless (storage `~/.local/share/containers`). Plus de loopback BTRFS ici : le rechunk rootless n'a jamais deux images complètes en storage (raw-img supprimée avant le pull oci-archive), pic ~25-30 G << ~110 G libres. Si saturation un jour (variante DX), réintroduire `./.github/actions/mount-btrfs-storage` avec `target-dir: ~/.local/share/containers`.

**Étapes** :
1. Version kernel auto via `skopeo list-tags` si `kernel_version` vide — retry 3×10 s (timeouts ghcr.io)
2. Checkout ; maximisation espace disque
3. Build buildah rootless — retry `nick-fields/retry@v4` (`retry_on: error`, timeout 90 min / 120 min DX — miroirs lents) : le script détecte les erreurs réseau (EOF, TLS handshake, DNS, curl timeout…) → exit 1 (retry) ; erreur de build RUN → exit 2 (échec immédiat)
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
1. Kernel pinné dans le `build.yml` de la branche main de Bazzite — fetch `raw.githubusercontent.com/ublue-os/bazzite/main/.github/workflows/build.yml` (retry curl 3×10 s) + extraction awk du `kernel_version` du 1er bloc matrix dont `kernel_flavor` correspond exactement (`ogc`/`ogc-lts` ancré en fin de clé) et `fedora_version` correspond ; état du bloc réinitialisé à chaque `- item`. Suit les kernels préparés en avance par Bazzite
2. Filet de sécurité : kernel réellement publié sur l'image stable Bazzite — label OCI `ostree.linux` via `skopeo inspect` (retry 3×10 s), suffixe `.x86_64` retiré ; source `ghcr.io/ublue-os/bazzite:stable`, ou `bazzite-nvidia:stable` pour nvidia-lts (Bazzite paire nvidia-lts ↔ ogc-lts, commit `3eb7b09` — akmods-nvidia-lts ne publie plus que des tags `ogc-lts-*`). Utilisé si le pin main n'a pas ses akmods (kernel RC trop frais) ou fetch/parsing en échec
3. Fallback : dernière version commune via `skopeo list-tags` (retry 3×10 s par repo) — intersection tags `akmods` et `akmods-{NVIDIA_FLAVOR}` → filtre `{KERNEL_FLAVOR}-{FEDORA_VERSION}-*`, exclusion alias non versionnés (`test("^[0-9]")`), normalisation suffixe `.x86_64` des alias arch, tri `sort -V` (warning dans les logs)
- `check_akmods` : valide `akmods:{FLAVOR}-fc{FEDORA}-{VERSION}` et, si NVIDIA, `akmods-{NVIDIA_FLAVOR}:{…}` (ublue peut arrêter de builder nvidia-lts contre un kernel RC, cf. Bazzite `a5897ab`)
- Manuel : `kernel_version` dans un job → détection skippée

## build-gablue-live-isos.yml

Build des **ISOs live** Plasma complet (tous les 5 jours). Déclencheurs : schedule, workflow_dispatch, push `[iso]`, **`workflow_run`** en fin du workflow d'images — ne construit l'ISO que si l'amont a été déclenché par un push (`workflow_run.event == 'push'`), réussi (`conclusion == 'success'`) et le message contient `[all-iso]` → images `:latest` publiées avant les ISOs ; checkout sur `workflow_run.head_sha`.

- **Concurrency ISO** : groupe `build-gablue-live-isos-${{ github.run_id }}-iso` avec `github.run_id` (pas `github.ref`) — chaque run ISO unique ; sinon un `workflow_run` déclenché par un échec annulerait un `workflow_dispatch` ISO en cours (même groupe, ses jobs skipped de toute façon)
- **Titanoboa** : installateur bootc générant un squashfs live ; `build_iso.sh` de l'image Titanoboa (`quay.io/fedora/fedora:latest`) patché via `installer/titanoboa_build_iso.sh` (bind-mounté, sans `-all-root` — préserve l'UID 1000 du préfixe Wine)
- 5 variantes : main, main-dx, nvidia, nvidia-open, nvidia-open-dx ; `timeout-minutes: 180`
- **Processus en 2 étapes** :
  1. Image payload via `installer/Containerfile` (FROM image Gablue, flatpaks pré-cachés, swap kernel OGC→vanilla pour Secure Boot). Stockage podman sur le loopback BTRFS (action `mount-btrfs-storage`) **avec `image_copy_tmp_dir` redirigé dans le loopback** (drop-in `/etc/containers/containers.conf.d/`) : par défaut podman copie le layer diff du commit (~30 G non compressés) dans `/var/tmp` **sur l'hôte** → saturation, loopback sparse affamé, BTRFS read-only (les « corruptions » historiques du loopback n'étaient que ce mécanisme). Chemin explicite `/var/lib/containers/image-copy-tmp` plutôt que `"storage"` (résolution buggy, podman#28211). Boucle de build 3 tentatives : **pas de retry** si log contient `no space left on device`/`read-only file system`/`disk I/O error` (non récupérable — tentative identique, loopback read-only mort) → exit immédiat. Optimisations espace (Bazzite `7ecb26a1`) : `extra-squeeze: "true"` sur remove-unwanted-software (~6 G), `TMPDIR=/var/lib/containers/image-copy-tmp` sur le podman build, step `if: always()` affichant `df -h` + taille loopback réelle
  2. Génération ISO via `podman run` direct (remplace l'action `Zeglius/titanoboa`) : script patché bind-mounté sur `/src/build_iso.sh`, payload `localhost/payload:latest` monté `--mount type=image`, sortie bind-mountée `/output`. Image Titanoboa **pré-pullée avec retry** (5×15 s — timeouts quay.io transitoires)
- Signature Cosign + attestation de provenance ; upload BuzzHeavier ; release GitHub `latest-live-iso`
- **Job `create-release`** : artifacts liens/checksums via `gh run download` wrappé `nick-fields/retry@v4` (6×15 s) plutôt qu'`actions/download-artifact` — l'API artifacts peut renvoyer un 404 « workflow run not found » transitoire juste après la fin des jobs ISO (race de propagation interne) ; `actions: read` déclaré ; structure de dossiers identique (`merge-multiple: false`)

## clean-gablue-images.yml

Nettoyage hebdomadaire (dimanche) : images > 90 jours supprimées ; conservation des 7 dernières taggées + 7 dernières non-taggées. Packages : gablue-main, gablue-nvidia, gablue-nvidia-open, gablue-main-dx, gablue-nvidia-open-dx, gablue-main-test, gablue-nvidia-open-test

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
