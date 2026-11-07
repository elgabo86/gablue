"""Nettoyer le système : choisir les domaines, puis confirmer.

Équivalent graphique de `ujust clean-system`, avec une case par domaine :
Podman (`system prune -a --volumes`), Flatpaks inutilisés (compte + système),
rpm-ostree (`cleanup -bm`) et Homebrew (autoremove + cleanup). Un domaine dont
l'outil manque (Podman hors variante DX, Homebrew absent) est grisé, jamais
coché en silence.
"""

from PySide6.QtCore import Qt, QThread, Signal
from PySide6.QtWidgets import (
    QCheckBox, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QVBoxLayout,
)

from .. import local
from ..local import (
    CLEAN_DOMAINS, clean_domain_available, clean_system_script,
    parse_gablue_progress,
)
from ..tasks import LocalTaskDialog, start_task
from ..uiutil import fit_dialog_height

CHOICE_LABELS = {
    "podman": (
        "Podman",
        "Images, conteneurs arrêtés, réseaux, cache de build et volumes non "
        "utilisés. Le contenu des volumes supprimés est perdu.",
    ),
    "flatpak": (
        "Flatpaks",
        "Applications et runtimes inutilisés, pour le compte et pour le "
        "système.",
    ),
    "ostree": (
        "rpm-ostree",
        "Anciens déploiements de l'image et métadonnées obsolètes. Le "
        "déploiement en cours n'est pas touché.",
    ),
    "brew": (
        "Homebrew",
        "Formules devenues inutiles (autoremove) et cache de téléchargement "
        "(cleanup).",
    ),
    "gwine_temp": (
        "Cache gwine — temp",
        "Fichiers temporaires des installations et mises à jour gwine. "
        "Sans effet sur les préfixes existants.",
    ),
    "gwine_shaders": (
        "Cache gwine — shaders",
        "Shaders compilés par Wine/DXVK. Recompilés au fil des lancements.",
    ),
    "gwine": (
        "Cache gwine — complet",
        "Composants Wine/DXVK téléchargés, shaders et temporaires. Récupérés "
        "au prochain lancement de gwine.",
    ),
    "wgp": (
        "Cache WGP",
        "Configuration amorcée des jeux WGP. Réamorcée au prochain "
        "lancement ; les réglages modifiés dans le cache sont perdus.",
    ),
    "lgp": (
        "Cache LGP",
        "Configuration amorcée des jeux LGP. Réamorcée au prochain "
        "lancement ; les réglages modifiés dans le cache sont perdus.",
    ),
    "opencode": (
        "Cache OpenCode",
        "Paquets npm des plugins et MCP d'OpenCode. Réinstallés au besoin ; "
        "les artefacts de build de ~/.cache/opencode sont conservés.",
    ),
}


# Les mesures tournent dans un thread : `du` sur plusieurs gigaoctets prend
# quelques secondes. Le worker vit sans parent pour survivre à la fermeture du
# dialogue (sinon « QThread destroyed while running »).
_ACTIVE_WORKERS: set = set()


class _SizeWorker(QThread):
    """Estime l'espace récupérable par domaine, sans bloquer l'interface."""

    done = Signal(object)          # dict[domaine, octets | None]

    def __init__(self, domains: list[str], parent=None):
        super().__init__(parent)
        self._domains = list(domains)

    def run(self) -> None:
        sizes: dict[str, int | None] = {}
        for domain in self._domains:
            if domain == "podman":
                sizes[domain] = local.podman_reclaimable()
            else:
                sizes[domain] = local.cache_size(domain)
        self.done.emit(sizes)


class CleanDialog(QDialog):
    """Une case par domaine de nettoyage, un seul « Nettoyer »."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Nettoyer le système")
        self.setModal(True)
        self.setMinimumWidth(700)

        root_layout = QVBoxLayout(self)
        root_layout.setContentsMargins(24, 22, 24, 20)
        root_layout.setSpacing(10)

        heading = QLabel("Nettoyer le système", self)
        heading.setObjectName("title")
        root_layout.addWidget(heading)

        note = QLabel(
            "Supprime les restes inutilisés des outils choisis. Vos fichiers "
            "personnels, vos jeux et la configuration des applications ne "
            "sont pas touchés.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        root_layout.addLayout(self.rows_layout, 1)

        self.checks: dict[str, QCheckBox] = {}
        self.size_labels: dict[str, QLabel] = {}
        self.sizes: dict[str, int | None] = {}
        self.available: dict[str, bool] = {}
        for domain in CLEAN_DOMAINS:
            self._add_row(domain)
        self._start_sizes()

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        buttons.addWidget(self.hint_label)
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        self.clean_button = QPushButton("Nettoyer", self)
        self.clean_button.setObjectName("accent")
        self.clean_button.setCursor(Qt.PointingHandCursor)
        self.clean_button.clicked.connect(self.clean)
        buttons.addWidget(self.clean_button)
        root_layout.addLayout(buttons)

        self._update_apply()
        fit_dialog_height(self, 700)

    # -- construction -------------------------------------------------------
    def _add_row(self, domain: str) -> None:
        label, description = CHOICE_LABELS[domain]
        available = clean_domain_available(domain)
        self.available[domain] = available

        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        top = QHBoxLayout()
        top.setSpacing(8)
        check = QCheckBox(label, frame)
        check.setCursor(Qt.PointingHandCursor)
        check.setChecked(available)          # tout ce qui peut l'être, par défaut
        check.setEnabled(available)
        check.toggled.connect(self._update_apply)
        top.addWidget(check)
        top.addStretch(1)
        size_label = QLabel("", frame)
        size_label.setObjectName("hint")
        self.size_labels[domain] = size_label
        top.addWidget(size_label)
        layout.addLayout(top)

        detail = QLabel(
            description if available else self._missing_text(domain), frame
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        self.checks[domain] = check
        self.rows_layout.addWidget(frame)

    @staticmethod
    def _missing_text(domain: str) -> str:
        if domain == "podman":
            return "Podman n'est pas installé sur cette variante."
        if domain == "brew":
            return "Homebrew n'est pas installé sur ce poste."
        if domain == "ostree":
            return "Helper absent de cette image — reconstruction requise."
        return "Rien à nettoyer : ce cache est absent sur ce poste."

    # -- état ---------------------------------------------------------------
    def _start_sizes(self) -> None:
        """Mesure en fond les domaines qui ont un volume identifiable."""
        domains = [
            domain for domain in CLEAN_DOMAINS
            if self.available.get(domain)
            and (domain == "podman" or local.clean_cache_path(domain) is not None)
        ]
        if not domains:
            return
        for domain in domains:
            self.size_labels[domain].setText("…")
        worker = _SizeWorker(domains)
        worker.done.connect(self._sizes_done)
        worker.finished.connect(lambda: _ACTIVE_WORKERS.discard(worker))
        worker.finished.connect(worker.deleteLater)
        _ACTIVE_WORKERS.add(worker)
        self._size_worker = worker
        worker.start()

    def _sizes_done(self, sizes: dict) -> None:
        self.sizes = dict(sizes or {})
        for domain, label in self.size_labels.items():
            label.setText(local.format_size(self.sizes.get(domain)))
        self._update_apply()

    def selected(self) -> list[str]:
        return [
            domain for domain in CLEAN_DOMAINS
            if self.available.get(domain) and self.checks[domain].isChecked()
        ]

    def _update_apply(self) -> None:
        chosen = self.selected()
        self.clean_button.setEnabled(bool(chosen))
        if chosen:
            total = sum(self.sizes.get(domain) or 0 for domain in chosen)
            gain = f" · {local.format_size(total)} à récupérer" if total else ""
            self.hint_label.setText(
                f"{len(chosen)} domaine{'s' if len(chosen) > 1 else ''} "
                f"à nettoyer{gain}"
            )
        elif any(self.available.values()):
            self.hint_label.setText("Aucun domaine sélectionné.")
        else:
            self.hint_label.setText("Aucun outil de nettoyage disponible.")

    # -- application --------------------------------------------------------
    def clean(self) -> None:
        chosen = self.selected()
        if not chosen:
            return
        details = "\n".join(
            f"• {CHOICE_LABELS[domain][0]}" for domain in chosen
        )
        answer = QMessageBox.question(
            self, "Nettoyer le système",
            f"Nettoyer ces domaines ?\n\n{details}\n\n"
            "Cette action est immédiate et ne peut pas être annulée.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        dialog = LocalTaskDialog(
            self, title="Nettoyer le système",
            subtitle="Suppression des restes inutilisés.",
            argv=["/usr/bin/bash", "-c", clean_system_script(chosen)],
            progress_parser=parse_gablue_progress,
            progress_text="Nettoyage…",
            reveal_log_on_error=True,
        )
        start_task(self, dialog)
        self.accept()
