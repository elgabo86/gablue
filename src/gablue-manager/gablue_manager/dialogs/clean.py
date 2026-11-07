"""Nettoyer le système : un bouton « Vider » par outil, avec confirmation.

Équivalent graphique de `ujust clean-system`, mais sans sélection multiple :
chaque domaine se vide séparément — Podman (`system prune -a --volumes`),
Flatpaks inutilisés (compte + système), Homebrew (autoremove + cleanup) et les
caches régénérables (gwine temp/shaders, WGP, LGP, npm d'OpenCode). Une carte
par domaine : nom, gain estimé, bouton « Vider », puis une confirmation qui
rappelle ce que le geste supprime.

Les tailles se mesurent en fond (`podman system df`, `du -sb`) ; celles que
l'outil ne sait pas donner (Flatpaks, Homebrew) sont affichées « ? ».
"""

from PySide6.QtCore import Qt, QThread, Signal
from PySide6.QtGui import QGuiApplication
from PySide6.QtWidgets import (
    QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox, QPushButton,
    QScrollArea, QVBoxLayout, QWidget,
)

from .. import local
from ..local import (
    CLEAN_DOMAINS, clean_domain_available, clean_system_script,
    parse_gablue_progress,
)
from ..tasks import LocalTaskDialog, start_task
from .stversions import StVersionsSection

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
    """Une carte par domaine de nettoyage, un bouton « Vider » par carte."""

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
            "Chaque outil se vide séparément, après confirmation. Vos fichiers "
            "personnels, vos jeux et la configuration des applications ne sont "
            "pas touchés.",
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        # La liste grandit avec les domaines et les dossiers Syncthing : un
        # ascenseur évite un dialogue plus haut que l'écran sur un 1080p.
        content = QWidget(self)
        content_layout = QVBoxLayout(content)
        content_layout.setContentsMargins(0, 0, 0, 0)
        content_layout.setSpacing(8)
        self.rows_layout = QVBoxLayout()
        self.rows_layout.setSpacing(8)
        content_layout.addLayout(self.rows_layout)
        self.scroll = QScrollArea(self)
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setWidget(content)
        root_layout.addWidget(self.scroll, 1)

        self.buttons: dict[str, QPushButton] = {}
        self.size_labels: dict[str, QLabel] = {}
        self.sizes: dict[str, int | None] = {}
        self.available: dict[str, bool] = {}
        for domain in CLEAN_DOMAINS:
            self._add_row(domain)

        # Anciennes versions Syncthing : même dialogue, section à part. Elle
        # ne supprime rien (le clic ouvre Dolphin), donc pas de bouton
        # « Vider » — mais c'est bien un nettoyage que l'utilisateur fait ici.
        self.versions = StVersionsSection(self)
        content_layout.addWidget(self.versions)
        if not self.versions.refresh():
            self.versions.setVisible(False)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        self.hint_label.setWordWrap(True)
        buttons.addWidget(self.hint_label, 1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.setDefault(True)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        root_layout.addLayout(buttons)

        self._update_hint()
        self._start_sizes()
        self._fit_height()

    # -- construction -------------------------------------------------------
    def _fit_height(self) -> None:
        """Hauteur raisonnable, plafonnée à l'écran ; l'ascenseur fait le reste."""
        height = 860
        screen = QGuiApplication.primaryScreen()
        if screen is not None:
            height = min(height, screen.availableGeometry().height() - 80)
        self.resize(760, max(560, height))

    @staticmethod
    def measurable(domain: str) -> bool:
        """Une taille est-elle calculable ? (Podman via `system df`, caches via `du`.)"""
        return domain == "podman" or local.clean_cache_path(domain) is not None

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
        name = QLabel(label, frame)
        name.setObjectName("hostName")
        top.addWidget(name)
        top.addStretch(1)
        size_label = QLabel("", frame)
        size_label.setObjectName("hint")
        if available:
            # « ? » : l'outil ne sait pas donner la taille ; « … » en attendant
            # la mesure (remplacé dès que le worker a fini).
            size_label.setText("…" if self.measurable(domain) else "?")
        self.size_labels[domain] = size_label
        top.addWidget(size_label)
        button = QPushButton("Vider", frame)
        button.setObjectName("ghost")
        button.setCursor(Qt.PointingHandCursor)
        button.setEnabled(available)
        if not available:
            button.setToolTip(self._missing_text(domain))
        button.clicked.connect(
            lambda _checked=False, chosen=domain: self.clean(chosen)
        )
        self.buttons[domain] = button
        top.addWidget(button)
        layout.addLayout(top)

        detail = QLabel(
            description if available else self._missing_text(domain), frame
        )
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        self.rows_layout.addWidget(frame)

    @staticmethod
    def _missing_text(domain: str) -> str:
        if domain == "podman":
            return "Podman n'est pas installé sur cette variante."
        if domain == "brew":
            return "Homebrew n'est pas installé sur ce poste."
        if domain == "flatpak":
            return "Flatpak n'est pas installé sur ce poste."
        return "Rien à nettoyer : ce cache est absent sur ce poste."

    # -- état ---------------------------------------------------------------
    def _start_sizes(self, domains: list[str] | None = None) -> None:
        """Mesure en fond les domaines qui ont un volume identifiable."""
        if domains is None:
            domains = [
                domain for domain in CLEAN_DOMAINS
                if self.available.get(domain) and self.measurable(domain)
            ]
        domains = [
            domain for domain in domains
            if self.available.get(domain) and self.measurable(domain)
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
        # Une mesure relancée après nettoyage ne concerne qu'un domaine : on
        # fusionne pour ne pas effacer les tailles des autres.
        self.sizes.update(sizes or {})
        for domain, label in self.size_labels.items():
            if not self.available.get(domain):
                continue
            if not self.measurable(domain):
                label.setText("?")
                continue
            size = self.sizes.get(domain)
            label.setText("?" if size is None else local.format_size(size))
        self._update_hint()

    def _update_hint(self) -> None:
        total = sum(size or 0 for size in self.sizes.values())
        parts: list[str] = []
        if total:
            parts.append(f"≈ {local.format_size(total)} récupérables")
        if any(
            label.text() == "?"
            for domain, label in self.size_labels.items()
            if self.available.get(domain)
        ):
            parts.append("« ? » : taille non mesurable")
        self.hint_label.setText(" · ".join(parts))

    # -- application --------------------------------------------------------
    def clean(self, domain: str) -> None:
        """Vide un seul domaine, après confirmation explicite."""
        if not self.available.get(domain):
            return
        label, description = CHOICE_LABELS[domain]
        size = self.sizes.get(domain)
        gain = ""
        if size:
            gain = f"\n\nEspace récupérable : {local.format_size(size)}."
        answer = QMessageBox.question(
            self, f"Vider — {label}",
            f"Vider « {label} » ?\n\n{description}{gain}\n\n"
            "Cette action est immédiate et ne peut pas être annulée.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        self.buttons[domain].setEnabled(False)
        task = LocalTaskDialog(
            self, title=f"Vider — {label}",
            subtitle="Suppression des restes inutilisés.",
            argv=["/usr/bin/bash", "-c", clean_system_script([domain])],
            progress_parser=parse_gablue_progress,
            progress_text="Nettoyage…",
            reveal_log_on_error=True,
        )
        task.completed.connect(
            lambda _ok, chosen=domain: self._domain_done(chosen)
        )
        start_task(self, task)

    def _domain_done(self, domain: str) -> None:
        """Relance la mesure du domaine vidé et réactive son bouton."""
        try:
            self.buttons[domain].setEnabled(self.available.get(domain, False))
            self.sizes.pop(domain, None)
            self._start_sizes([domain])
        except RuntimeError:
            # Dialogue fermé pendant la tâche : plus rien à rafraîchir.
            return
