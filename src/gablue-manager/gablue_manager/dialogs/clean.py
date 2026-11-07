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
    QScrollArea, QTabWidget, QVBoxLayout, QWidget,
)

from .. import local
from ..local import (
    CLEAN_DOMAINS, REMOTE_CLEAN_PROBE, clean_domain_available,
    clean_system_script, parse_gablue_progress, parse_remote_clean_state,
    remote_clean_script, remote_helpers_nopasswd,
)
from ..ssh import (
    ask_remote_password, remote_sudo_prefix, ssh_capture,
)
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..util import display_name
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

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host) if host else None
        self.setWindowTitle(
            "Nettoyer le système" if self.host is None
            else f"Nettoyer le système — {display_name(self.host)}"
        )
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
            "pas touchés."
            + ("" if self.host is None else " Exécuté sur le poste distant, "
               "par SSH."),
            self,
        )
        note.setObjectName("subtitle")
        note.setWordWrap(True)
        root_layout.addWidget(note)

        # Deux onglets : le nettoyage par domaine d'un côté, les anciennes
        # versions Syncthing de l'autre. Les mélanger dans une seule liste
        # allongeait le dialogue et brouillait les deux gestes (l'un supprime,
        # l'autre ouvre Dolphin). Un ascenseur par onglet tient sur un 1080p.
        self.tabs = QTabWidget(self)
        self.tabs.setDocumentMode(True)
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
        self.tabs.addTab(self.scroll, "Nettoyage")
        root_layout.addWidget(self.tabs, 1)

        self.buttons: dict[str, QPushButton] = {}
        self.size_labels: dict[str, QLabel] = {}
        self.sizes: dict[str, int | None] = {}
        # L'état vient d'ici en local ou d'une sonde SSH sur un poste distant.
        self.state = self.read_state()
        # Garde aussi `clean_helper` (hors CLEAN_DOMAINS) : il décide si la
        # partie système des Flatpaks a besoin du mot de passe sudo.
        self.available = dict(self.state.get("available") or {})
        for domain in CLEAN_DOMAINS:
            self._add_row(domain)

        # Anciennes versions Syncthing : onglet local seulement (le dossier
        # appartient à CETTE session, un poste distant a les siennes). Il
        # n'existe que si Syncthing tourne et déclare des `.stversions`.
        self.versions = None
        if self.host is None:
            versions = StVersionsSection(self)
            if versions.refresh():
                self.versions = versions
                self.tabs.addTab(versions, "Anciennes versions Syncthing")
            else:
                versions.deleteLater()

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

    def read_state(self) -> dict:
        """Disponibilités et tailles : local, ou une sonde SSH unique."""
        if self.host is None:
            return {"available": {
                domain: clean_domain_available(domain)
                for domain in CLEAN_DOMAINS
            }}
        code, text = ssh_capture(self.host, REMOTE_CLEAN_PROBE, timeout=60)
        if code != 0 and not text:
            return {"available": {}, "sizes": {}}
        return parse_remote_clean_state(text)

    def measurable(self, domain: str) -> bool:
        """Une taille est-elle calculable ? (Podman via `system df`, caches via `du`.)"""
        if self.host is not None:
            return domain in (self.state.get("sizes") or {})
        return domain == "podman" or local.clean_cache_path(domain) is not None

    def _add_row(self, domain: str) -> None:
        label, description = CHOICE_LABELS[domain]
        available = self.available.get(domain, False)

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
        if self.host is not None:
            # La sonde SSH a déjà tout mesuré : rien à lancer en fond.
            self._sizes_done(dict(self.state.get("sizes") or {}))
            return
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
        if self.host is not None:
            self._remote_clean(domain, label)
            return
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

    def _remote_clean(self, domain: str, label: str) -> None:
        """Vide un domaine sur le poste distant, mot de passe seulement si root."""
        password = ""
        needs_root = (
            domain == "flatpak" and self.available.get("clean_helper", False)
        )
        if needs_root and not remote_helpers_nopasswd(self.host):
            password = ask_remote_password(self, self.host)
            if password is None:
                self.buttons[domain].setEnabled(
                    self.available.get(domain, False)
                )
                return
        command = remote_sudo_prefix(password) + remote_clean_script(
            domain, self.available
        )
        task = RemoteTaskDialog(
            self, host=self.host, title=f"Vider — {label}",
            subtitle="Suppression des restes inutilisés, par SSH.",
            command=command, password=password,
            progress_mode="busy",
            progress_text="Nettoyage…",
        )
        task.completed.connect(
            lambda _ok, chosen=domain: self._domain_done(chosen)
        )
        start_task(self, task)

    def _domain_done(self, domain: str) -> None:
        """Après la tâche : ligne mise à jour sans bloquer la boucle GUI."""
        try:
            self.sizes.pop(domain, None)
            if self.host is None:
                # Mesure en fond (`du -sb`) : le dialogue reste utilisable.
                self.buttons[domain].setEnabled(
                    self.available.get(domain, False)
                )
                self._start_sizes([domain])
                return
            # Jamais de connexion SSH ici : `completed` est émis pendant que
            # `LocalTaskDialog`/`RemoteTaskDialog._finished` tient encore la
            # boucle GUI ; une sonde synchrone (jusqu'à 60 s) gèlerait toute
            # l'app, la fenêtre de progression comprise. Rouvrir le dialogue
            # refait une sonde exacte.
            self.size_labels[domain].setText("?")
            if domain in local.CLEAN_CACHE_SUFFIXES:
                # Le cache vient d'être supprimé : plus rien à vider.
                self.available[domain] = False
            self.buttons[domain].setEnabled(
                self.available.get(domain, False)
            )
            self._update_hint()
        except RuntimeError:
            # Dialogue fermé pendant la tâche : plus rien à rafraîchir.
            return
