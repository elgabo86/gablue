# Installateur APK Gablue — paquet `gablue_apk_installer` (src/gablue-apk-installer)

Installateur d'APK Android par glisser-déposer (PySide6, Wayland natif) :
`.apk`, `.apkm`, `.xapk`, `.apks` ; appareils en USB ou WiFi (adb over TCP) ;
installation avec remplacement/downgrade. Nom affiché « Installateur APK
Gablue » ; binaire et `.desktop` = `gablue-apk-installer` /
`gablue-apk-installer.desktop`.

## Paquet

Sources = ce dossier (`src/gablue-apk-installer/`), installées par
`files/scripts/build-apk-installer` :

| Fichier | Rôle |
|---|---|
| `gablue-apk-installer` | lanceur `/usr/bin/gablue-apk-installer` : ajoute `src/gablue-apk-installer` (développement) ou `/usr/lib/gablue-apk-installer` au `sys.path` puis `gablue_apk_installer.app.main()` |
| `gablue_apk_installer/app.py` | `main()` : QApplication, nom d'application, fenêtre |
| `gablue_apk_installer/constants.py` | extensions APK/bundles, port ADB par défaut (5555) |
| `gablue_apk_installer/device.py` | `DeviceScanner` (adb devices -l) et `NetworkConnectThread` (tcpip + connect, appairage sans fil) |
| `gablue_apk_installer/install.py` | `InstallThread` : `adb install -r -d` ou extraction de bundle + `install-multiple` |
| `gablue_apk_installer/dialogs/pair.py` | `PairDialog` : appairage sans fil Android 11+ (adresse + code) |
| `gablue_apk_installer/window.py` | `ApkInstallerWindow` : appareils, glisser-déposer, liste, progression |

**Règle** : le paquet est la source de vérité ; ne pas réintroduire de fichier
unique. Un nouveau dialogue → un fichier dans `dialogs/`. Les imports vont dans
un seul sens (`constants` ← `device`/`install` ← `dialogs` ← `window` ← `app`).
Les objets Qt (`QThread`, `QDialog`, `QMainWindow`) restent dans leur module :
les signaux sont déclarés sur la classe qui les émet.

## Build

- `Containerfile-gablue` : `COPY src/gablue-apk-installer /src/gablue-apk-installer`,
  puis `RUN /ctx/build-apk-installer` juste après `build-tvqt`.
- `files/scripts/build-apk-installer` : copie `gablue_apk_installer/` dans
  `/usr/lib/gablue-apk-installer/`, installe le lanceur en
  `/usr/bin/gablue-apk-installer` (755) et retire les `__pycache__` (le Python
  du build n'est pas celui du poste, `/usr` est en lecture seule sur composefs).
- Développement : `src/gablue-apk-installer/gablue-apk-installer` marche depuis
  le dépôt.
- L'image doit être reconstruite pour valider l'installation (pas l'ISO).

## Comportement

- **Appareils** : `adb devices -l` toutes les 5 s dans un `QThread` ; analyse
  des propriétés `model:`/`product:` ; série contenant `:` = appareil réseau.
  Trois états affichés : autorisé, `unauthorized` (« débogage non autorisé »),
  `offline` (« hors ligne ») — le bouton Installer reste désactivé tant qu'il
  n'y a pas d'appareil autorisé.
- **WiFi** : appareil USB sélectionné → `adb tcpip 5555` puis détection de l'IP
  du téléphone en trois tentatives (`ip -4 addr show wlan0`, `ip route`,
  `getprop dhcp.wlan0.ipaddress`) et `adb connect ip:5555`. Appareil réseau
  déjà listé → simple reconnexion d'après la série.
- **Appairage sans fil Android 11+** (aucun USB) : `PairDialog` demande
  l'adresse d'appairage + le code, puis l'adresse de connexion (affichée sur le
  téléphone après appairage) ; exécution `adb pair` puis `adb connect`. Aucun
  QR code : trois champs saisis à la main.
- **Installation** : `.apk` → `adb install -r -d` (remplacement + downgrade).
  Bundle (`.apkm`/`.xapk`/`.apks`) → extraction dans un dossier temporaire
  `gablue-apk-*` (supprimé dans `finally`), puis `adb install-multiple -r -d`
  sur les APK trouvés récursivement ; aucun APK → erreur explicite.
- **Fichiers** : glisser-déposer (seules les extensions reconnues sont
  acceptées), ajout manuel par `QFileDialog`, doublons ignorés (comparaison sur
  le chemin), `Suppr`/« Retirer »/« Tout retirer ». Bundle affiché 📦, APK 📱,
  taille en Mo.
- **Threads** : le scan et l'installation sont annulables — fermer la fenêtre
  annule l'installation (`cancel()` + `wait(3000)`). Le scan est relancé au
  tick suivant s'il tourne déjà (pas d'empilement).
- **Privilèges** : aucun — tout passe par `adb` en utilisateur.

## Tests

Sonde hors écran dans un `HOME` de bac à sable :

```bash
cd <dépôt>
HOME=$(mktemp -d) QT_QPA_PLATFORM=offscreen PYTHONPATH=src/gablue-apk-installer \
python3 - <<'EOF'
from PySide6.QtWidgets import QApplication
from PySide6.QtCore import QTimer
app = QApplication([])
from gablue_apk_installer.window import ApkInstallerWindow
w = ApkInstallerWindow(); w.show()
assert w.file_list.count() == 0 and not w.install_btn.isEnabled()
w._add_file("/tmp/fake.apk"); w._add_file("/tmp/fake.apk")
assert w.file_list.count() == 1          # doublon ignoré
def quit_when_idle():                     # ⚠ attendre la fin du scan ADB
    if w._scanner.isRunning():
        QTimer.singleShot(100, quit_when_idle); return
    w._refresh_timer.stop(); w.close(); app.quit()
QTimer.singleShot(100, quit_when_idle)
app.exec()
EOF
```

**Piège** : quitter l'application pendant que `DeviceScanner` tourne fait
avorter l'interpréteur à la destruction du `QThread` (`QThread: Destroyed while
thread is still running`) — attendre la fin du scan avant `app.quit()`.
