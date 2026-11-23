"""Enregistreur et replay : les réglages de `gpad-rec`.

L'overlay manette les règle déjà (page RÉGLAGES) ; ce dialogue écrit **le même
fichier**, `~/.config/gablue/recorder.conf`, avec les mêmes défauts
(`local.RECORDER_DEFAULTS` reflète `config_defaut` du démon). Aucune second
source : le démon relit le fichier dès qu'il change (il en surveille la date),
donc un réglage appliqué ici vaut pour le prochain enregistrement sans
redémarrer quoi que ce soit.

Pourquoi ces défauts : H.264 1080p, pas HEVC natif. Un PC Windows sans
l'extension « HEVC Video Extensions » affiche un écran noir avec du son, et le
lecteur de Discord prévisualise mal le HEVC. Le HEVC reste proposé pour qui
archive chez lui (~28 % plus léger) — jamais par défaut.
"""

import os

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QCheckBox, QComboBox, QDialog, QFileDialog, QFrame, QHBoxLayout, QLabel,
    QLineEdit, QPushButton, QVBoxLayout,
)

from ..constants import _which
from ..local import (
    parse_recorder_conf, recorder_conf_path, recorder_estimate,
    write_recorder_conf,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

CODECS = (("H.264 — s'ouvre partout (défaut)", "h264"), ("H.265 / HEVC — plus léger, moins compatible", "h265"))
QUALITES = (
    ("Économique", "1"),
    ("Normale", "2"),
    ("Haute (défaut)", "3"),
    ("Très haute", "4"),
    ("Maximale (qualité constante)", "5"),
)
RESOLUTIONS = (
    ("1280x720 — léger, pour un téléphone", "1280x720"),
    ("1920x1080 — le format de partage courant", "1920x1080"),
    ("2560x1440 — 1440p", "2560x1440"),
    ("Source — résolution de l'écran (défaut)", "source"),
)
FPS = (
    ("60 — plafond (défaut)", "60"),
    ("120", "120"),
    ("144", "144"),
    ("Automatique — suit le jeu, plus lourd", "flips"),
)
DUREES = (("15 s", "15"), ("30 s (défaut)", "30"), ("60 s", "60"), ("2 min", "120"), ("5 min", "300"))
CURSEURS = (("Masqué (défaut)", "masque"), ("Affiché (v2, non fait)", "affiche"))
# Captures d'écran : lues par `gpad-shot` dans le même fichier.
SHOT_FORMATS = (
    ("JPEG — quelques centaines de Ko (défaut)", "jpeg"),
    ("PNG — sans perte, plusieurs Mo", "png"),
)
SHOT_QUALITES = (
    ("Économique", "basse"),
    ("Normale", "moyenne"),
    ("Haute (défaut)", "haute"),
)


class CaptureDialog(QDialog):
    """Codec, qualité, résolution, fps, curseur, replay et dossier de sortie."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Enregistreur et replay")
        self.setModal(True)
        self.setMinimumWidth(700)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel("Enregistreur et replay", self)
        heading.setObjectName("title")
        root.addWidget(heading)

        note = QLabel(
            "Réglages de l'enregistreur maison (capture KMS + encodage matériel). "
            "Ce sont les mêmes que dans l'overlay manette : un seul fichier, "
            "~/.config/gablue/recorder.conf. Ils s'appliquent au prochain "
            "enregistrement.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root.addWidget(note)

        # -- vidéo ----------------------------------------------------------
        video = self._card(root, "Vidéo")
        self.codec = self._combo(video, "Codec", CODECS)
        self.qualite = self._combo(video, "Qualité", QUALITES)
        self.reso = self._combo(video, "Résolution", RESOLUTIONS)
        self.fps = self._combo(video, "Images par seconde", FPS)
        self.curseur = self._combo(video, "Curseur", CURSEURS)
        for combo in (self.codec, self.qualite, self.reso, self.fps):
            combo.currentIndexChanged.connect(self._update_estimate)

        self.estimation = QLabel("", self)
        self.estimation.setObjectName("hostDetail")
        self.estimation.setWordWrap(True)
        video.addWidget(self.estimation)

        # -- replay ---------------------------------------------------------
        replay = self._card(root, "Replay")
        self.duree = self._combo(replay, "Durée du buffer", DUREES)
        self.auto_replay = QCheckBox(
            "Démarrer le replay automatiquement quand un jeu LGP ou WGP se lance",
            self,
        )
        self.auto_replay.setCursor(Qt.PointingHandCursor)
        replay.addWidget(self.auto_replay)
        auto_detail = QLabel(
            "Désactivé par défaut. Le buffer tourne alors sans bip ni "
            "notification dès qu'un .lgp ou .wgp est ouvert, et s'arrête avec "
            "le jeu. Un arrêt manuel pendant la partie n'est pas annulé.",
            self,
        )
        auto_detail.setObjectName("hostDetail")
        auto_detail.setWordWrap(True)
        replay.addWidget(auto_detail)

        # -- captures d'écran ------------------------------------------------
        photos = self._card(root, "Captures d'écran")
        self.shot_format = self._combo(photos, "Format", SHOT_FORMATS)
        self.shot_qualite = self._combo(photos, "Qualité", SHOT_QUALITES)
        photos_detail = QLabel(
            "JPEG divise le poids par 4 à 10 (une capture 1440p passe de "
            "plusieurs Mo à quelques centaines de Ko) ; PNG reste sans perte, "
            "avec une qualité qui règle la compression, pas l'image. "
            "S'applique à la capture suivante, prise par Home+droite ou par "
            "la ligne CAPTURE ÉCRAN de l'overlay.",
            self,
        )
        photos_detail.setObjectName("hostDetail")
        photos_detail.setWordWrap(True)
        photos.addWidget(photos_detail)

        # -- sortie ---------------------------------------------------------
        sortie = self._card(root, "Sortie")
        self.dossier, _ = self._folder_row(sortie, "Enregistrements", "~/Vidéos/Captures")
        self.shots, _ = self._folder_row(
            sortie, "Captures d'écran", "~/Images/Screenshots"
        )
        sortie_detail = QLabel(
            "Enregistrements (Video_<date>.mp4) et replays sauvegardés "
            "(Replay_<date>.mp4). Le buffer de replay vit en RAM, rien n'est "
            "écrit tant qu'on ne sauvegarde pas. Les captures d'écran "
            "(screenshot_<date>.jpg ou .png selon le format) suivent le même "
            "fichier : c'est aussi ce "
            "que règlent les Réglages du launcher, hors jeu. Un champ vide "
            "revient au défaut affiché.",
            self,
        )
        sortie_detail.setObjectName("hostDetail")
        sortie_detail.setWordWrap(True)
        sortie.addWidget(sortie_detail)

        # -- boutons --------------------------------------------------------
        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint = QLabel(self)
        self.hint.setObjectName("hostStatus")
        buttons.addWidget(self.hint)
        buttons.addStretch(1)
        fermer = QPushButton("Fermer", self)
        fermer.setObjectName("ghost")
        fermer.setCursor(Qt.PointingHandCursor)
        fermer.clicked.connect(self.accept)
        buttons.addWidget(fermer)
        self.appliquer = QPushButton("Appliquer", self)
        self.appliquer.setObjectName("accent")
        self.appliquer.setCursor(Qt.PointingHandCursor)
        self.appliquer.clicked.connect(self._apply)
        buttons.addWidget(self.appliquer)
        root.addLayout(buttons)

        self.texte = ""
        self.initial: dict[str, str] = {}
        self._load()
        fit_dialog_height(self, 700)

    # -- construction -------------------------------------------------------
    @staticmethod
    def _card(root: QVBoxLayout, titre: str) -> QVBoxLayout:
        frame = QFrame()
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)
        titre_label = QLabel(titre, frame)
        titre_label.setObjectName("title")
        layout.addWidget(titre_label)
        root.addWidget(frame)
        return layout

    def _folder_row(self, layout: QVBoxLayout, label: str, defaut: str):
        """Une ligne « libellé + chemin + Parcourir… » (dossier de sortie)."""
        row = QHBoxLayout()
        row.setSpacing(8)
        row.addWidget(QLabel(label, self))
        champ = QLineEdit(self)
        champ.setPlaceholderText(defaut)
        row.addWidget(champ, 1)
        parcourir = QPushButton("Parcourir…", self)
        parcourir.setObjectName("ghost")
        parcourir.setCursor(Qt.PointingHandCursor)
        parcourir.clicked.connect(lambda: self._choose_dir(champ, defaut))
        row.addWidget(parcourir)
        layout.addLayout(row)
        return champ, row

    @staticmethod
    def _combo(layout: QVBoxLayout, label: str, options) -> QComboBox:
        combo = QComboBox()
        for text, value in options:
            combo.addItem(text, value)
        row = QHBoxLayout()
        row.setSpacing(8)
        row.addWidget(QLabel(label))
        row.addWidget(combo, 1)
        layout.addLayout(row)
        return combo

    # -- état ---------------------------------------------------------------
    def _load(self) -> None:
        try:
            self.texte = recorder_conf_path().read_text(encoding="utf-8")
        except OSError:
            self.texte = ""
        values = parse_recorder_conf(self.texte)
        self.initial = dict(values)
        for combo, cle in (
            (self.codec, "codec"), (self.qualite, "qualite"),
            (self.reso, "resolution"), (self.fps, "fps"),
            (self.curseur, "curseur"),
            (self.shot_format, "shot_format"), (self.shot_qualite, "shot_qualite"),
        ):
            index = combo.findData(values.get(cle))
            combo.setCurrentIndex(index if index >= 0 else 0)
        index = self.duree.findData(values.get("replay_sec", "30"))
        self.duree.setCurrentIndex(index if index >= 0 else 1)
        self.auto_replay.setChecked(values.get("auto_replay") == "1")
        self.dossier.setText(values.get("dossier", ""))
        self.shots.setText(values.get("dossier_screenshot", ""))
        self.auto_replay.toggled.connect(lambda _v: self._update_apply())
        self.dossier.textChanged.connect(lambda _t: self._update_apply())
        self.shots.textChanged.connect(lambda _t: self._update_apply())
        self._update_estimate()
        self._update_apply()

    def desired(self) -> dict[str, str]:
        return {
            "codec": self.codec.currentData(),
            "qualite": self.qualite.currentData(),
            "resolution": self.reso.currentData(),
            "fps": self.fps.currentData(),
            "curseur": self.curseur.currentData(),
            "shot_format": self.shot_format.currentData(),
            "shot_qualite": self.shot_qualite.currentData(),
            "replay_sec": self.duree.currentData(),
            "auto_replay": "1" if self.auto_replay.isChecked() else "0",
            "dossier": self.dossier.text().strip(),
            "dossier_screenshot": self.shots.text().strip(),
        }

    def changed(self) -> bool:
        wanted = self.desired()
        return any(str(self.initial.get(k, "")) != v for k, v in wanted.items())

    def _update_estimate(self) -> None:
        go, mo = recorder_estimate(self.desired())
        self.estimation.setText(
            f"Ordre de grandeur : ≈ {go:.1f} Go par heure, "
            f"≈ {mo:.0f} Mo pour 30 s de replay "
            "(contenu en mouvement très détaillé ; un menu ou une pause pèse "
            "beaucoup moins)."
        )

    def _update_apply(self) -> None:
        dirty = self.changed()
        self.appliquer.setEnabled(dirty)
        self.hint.setText("Réglages à appliquer" if dirty else "Aucun changement.")

    def _choose_dir(self, champ: QLineEdit, defaut: str) -> None:
        start = champ.text().strip() or os.path.expanduser(defaut)
        chosen = QFileDialog.getExistingDirectory(self, "Dossier de sortie", start)
        if chosen:
            champ.setText(chosen)

    # -- application --------------------------------------------------------
    def _apply(self) -> None:
        if not self.changed():
            return
        wanted = self.desired()
        texte = write_recorder_conf(self.texte, wanted)
        chemin = str(recorder_conf_path())
        dossiers = " ".join(
            f'"{valeur}"'
            for valeur in (wanted["dossier"], wanted["dossier_screenshot"])
            if valeur
        )
        script = (
            "exec < /dev/null\n"
            f'mkdir -p "$(dirname "{chemin}")" {dossiers}\n'
            f"cat > \"{chemin}.gablue.tmp\" <<'GABLUE_EOF'\n{texte}GABLUE_EOF\n"
            f'mv "{chemin}.gablue.tmp" "{chemin}"\n'
        )
        dialog = LocalTaskDialog(
            self, title="Enregistreur",
            subtitle="Effet au prochain enregistrement.",
            argv=[_which("bash"), "-c", script],
            progress_mode="busy",
            progress_text="Enregistrement des réglages…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()
