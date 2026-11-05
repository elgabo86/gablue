"""Section « Applications » : installer, mettre à jour ou supprimer les AppImages.

Émulateurs (ES-DE, Azahar, Eden, Xenia, ShadPS4) et outils (OpenCode 2), tous
installés dans `~/AppImages` et intégrés au menu par GearLever. Le dialogue sert
la machine courante (`host=None`) comme un poste distant (état lu en une seule
connexion SSH, actions dans un `RemoteTaskDialog`).
"""

from PySide6.QtCore import Qt
from PySide6.QtWidgets import (
    QApplication, QDialog, QFrame, QHBoxLayout, QLabel, QMessageBox,
    QPushButton, QScrollArea, QVBoxLayout, QWidget,
)

from ..apps import (
    APPS, GROUPS, REMOTE_STATE_SCRIPT, all_states, config_argv, config_command,
    copy_esde_config, describe, gearlever_available, install_argv,
    install_command, installed_any, remote_states, remove_argv, remove_command,
    script_has_option,
)
from ..constants import _which
from ..ssh import ssh_capture
from ..tasks import LocalTaskDialog, RemoteTaskDialog, start_task
from ..uiutil import restyle_label
from ..util import display_name


class AppsDialog(QDialog):
    """Les applications AppImage d'un poste, en un seul endroit.

    Dialogue **non modal**, comme les tâches : on peut enchaîner plusieurs
    installations et continuer à utiliser la fenêtre principale. Les scripts
    `/usr/bin/<app>-install` font le téléchargement et l'intégration GearLever ;
    ici on ne fait que les lancer et afficher leur journal.
    """

    def __init__(self, parent=None, host: dict | None = None):
        super().__init__(parent)
        self.host = dict(host) if host else None
        title = "Applications"
        if self.host is not None:
            title = f"Applications — {display_name(self.host)}"
        self.setWindowTitle(title)
        self.setModal(False)
        self.setAttribute(Qt.WA_DeleteOnClose)
        self.setMinimumWidth(740)

        root = QVBoxLayout(self)
        root.setContentsMargins(24, 22, 24, 20)
        root.setSpacing(10)

        heading = QLabel("Applications et émulateurs", self)
        heading.setObjectName("title")
        root.addWidget(heading)

        self.note = QLabel(self)
        self.note.setObjectName("subtitle")
        self.note.setWordWrap(True)
        root.addWidget(self.note)

        self.rows_widget = QWidget(self)
        self.rows_layout = QVBoxLayout(self.rows_widget)
        self.rows_layout.setContentsMargins(0, 0, 0, 0)
        self.rows_layout.setSpacing(8)
        # Défilement : six applications ne tiennent pas sur un écran 800 px
        # (Steam Deck) ; le dialogue garde sa hauteur naturelle ailleurs.
        self.scroll = QScrollArea(self)
        self.scroll.setWidgetResizable(True)
        self.scroll.setFrameShape(QFrame.NoFrame)
        self.scroll.setWidget(self.rows_widget)
        root.addWidget(self.scroll, 1)

        self.rows: dict[str, dict] = {}
        self.states: dict[str, dict] = {}
        self.gearlever = False
        for group, title in GROUPS:
            section = QLabel(title, self)
            section.setObjectName("groupName")
            self.rows_layout.addWidget(section)
            for app in APPS:
                if app.group == group:
                    self._add_row(app)

        buttons = QHBoxLayout()
        buttons.setSpacing(10)
        self.hint_label = QLabel(self)
        self.hint_label.setObjectName("hostStatus")
        buttons.addWidget(self.hint_label)
        buttons.addStretch(1)
        self.close_button = QPushButton("Fermer", self)
        self.close_button.setObjectName("ghost")
        self.close_button.setCursor(Qt.PointingHandCursor)
        self.close_button.setDefault(True)
        self.close_button.clicked.connect(self.accept)
        buttons.addWidget(self.close_button)
        root.addLayout(buttons)

        self.refresh()
        self._fit_rows()

    def _fit_rows(self) -> None:
        """Hauteur des cartes, plafonnée à l'écran (le reste défile).

        `fit_dialog_height` ne convient pas ici : dans une zone défilante, la
        hauteur naturelle est celle du contenu, pas celle du QScrollArea.
        """
        self.rows_widget.adjustSize()
        content = self.rows_widget.sizeHint().height()
        screen = QApplication.primaryScreen()
        available = screen.availableGeometry().height() if screen else 900
        # ~180 px de marges, titre, note et boutons ; 30 px de respiration.
        limit = max(240, available - 210)
        self.scroll.setMinimumHeight(min(content, limit))
        self.resize(740, min(self.sizeHint().height(), available - 40))

    # -- cartes -------------------------------------------------------------
    def _add_row(self, app) -> None:
        frame = QFrame(self)
        frame.setObjectName("hostCard")
        layout = QVBoxLayout(frame)
        layout.setContentsMargins(16, 12, 16, 12)
        layout.setSpacing(6)

        top = QHBoxLayout()
        top.setSpacing(8)
        name = QLabel(app.name, frame)
        name.setObjectName("hostName")
        state = QLabel(frame)
        state.setObjectName("hint")
        top.addWidget(name)
        top.addWidget(state)
        top.addStretch(1)
        layout.addLayout(top)

        detail = QLabel(app.description, frame)
        detail.setObjectName("hostDetail")
        detail.setWordWrap(True)
        layout.addWidget(detail)

        actions = QHBoxLayout()
        actions.setSpacing(8)
        actions.addStretch(1)
        row = {"state": state, "frame": frame}
        if app.key == "esde":
            config = QPushButton("Mettre à jour la config", frame)
            config.setObjectName("ghost")
            config.setCursor(Qt.PointingHandCursor)
            config.clicked.connect(self.update_esde_config)
            actions.addWidget(config)
            row["config"] = config
        remove = QPushButton("Supprimer", frame)
        remove.setObjectName("ghost")
        remove.setCursor(Qt.PointingHandCursor)
        remove.clicked.connect(lambda: self.remove(app))
        actions.addWidget(remove)
        action = QPushButton("Installer", frame)
        action.setObjectName("accent")
        action.setCursor(Qt.PointingHandCursor)
        action.clicked.connect(lambda: self.install(app))
        actions.addWidget(action)
        layout.addLayout(actions)

        row.update({"action": action, "remove": remove})
        self.rows[app.key] = row
        self.rows_layout.addWidget(frame)

    # -- état ---------------------------------------------------------------
    def refresh(self) -> None:
        if self.host is None:
            self.gearlever = gearlever_available()
            self.states = all_states()
            self._set_note()
        else:
            code, output = ssh_capture(
                self.host, REMOTE_STATE_SCRIPT, timeout=30
            )
            if code != 0:
                self.states = {}
                self.note.setText(
                    f"{display_name(self.host)} est injoignable : "
                    "l'état des applications est indisponible."
                )
                restyle_label(self.note, "warn")
                self._apply_rows()
                return
            self.states, self.gearlever = remote_states(output)
            self._set_note()
        self._apply_rows()

    def _set_note(self) -> None:
        target = "ce poste" if self.host is not None else "cette machine"
        if self.gearlever:
            self.note.setText(
                f"AppImages de {target} (~/AppImages), intégrées au menu par "
                "GearLever. Installer ou mettre à jour télécharge la dernière "
                "version publiée ; configuration, sauvegardes et jeux sont "
                "conservés, y compris à la suppression."
            )
            restyle_label(self.note, "subtitle")
        else:
            self.note.setText(
                f"GearLever (it.mijorus.gearlever) n'est pas installé sur "
                f"{target} : l'installation et la mise à jour sont "
                "indisponibles, la suppression reste possible."
            )
            restyle_label(self.note, "warn")

    def _apply_rows(self) -> None:
        for app in APPS:
            row = self.rows[app.key]
            state = self.states.get(app.key)
            if state is None:
                row["state"].setText("État indisponible")
                restyle_label(row["state"], "hint")
                row["action"].setEnabled(False)
                row["remove"].setEnabled(False)
                config = row.get("config")
                if config is not None:
                    config.setEnabled(False)
                continue

            installed = installed_any(app, state)
            row["state"].setText(describe(app, state))
            restyle_label(row["state"], "ok" if installed else "hint")

            row["action"].setText(
                "Mettre à jour" if installed else "Installer"
            )
            row["action"].setEnabled(self.gearlever and state["script_ok"])
            if not state["script_ok"]:
                row["action"].setToolTip(
                    f"{app.script} introuvable sur ce poste (image à mettre à "
                    "jour)."
                )
            else:
                row["action"].setToolTip("")
            row["remove"].setEnabled(installed)
            if app.remover and installed and not state.get("remover"):
                row["remove"].setEnabled(False)
                row["remove"].setToolTip(
                    f"{app.remover} introuvable sur ce poste (image à mettre "
                    "à jour)."
                )
            else:
                row["remove"].setToolTip("")
            config = row.get("config")
            if config is not None:
                config.setEnabled(state["installed"] and state["script_ok"])

        if not self.states:
            self.hint_label.setText("État indisponible.")
        elif not self.gearlever:
            self.hint_label.setText("GearLever manquant.")
        elif not any(state["script_ok"] for state in self.states.values()):
            self.hint_label.setText("Scripts d'installation absents de l'image.")
        else:
            self.hint_label.setText("")

    # -- actions ------------------------------------------------------------
    def install(self, app) -> None:
        state = self.states.get(app.key)
        if state is None or not state["script_ok"]:
            return
        if installed_any(app, state):
            title = f"{app.name} — mise à jour"
            question = (
                f"Mettre à jour {app.name} ?\n\nLa version actuelle sera "
                "remplacée une fois la nouvelle téléchargée et vérifiée."
            )
        else:
            title = f"{app.name} — installation"
            question = (
                f"Installer {app.name} ?\n\nLa dernière version sera "
                "téléchargée puis intégrée au menu par GearLever."
            )
        answer = QMessageBox.question(
            self, title, question,
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        subtitle = f"{app.script} — téléchargement et intégration GearLever."
        if self.host is None:
            self._run(LocalTaskDialog(
                self, title=title, subtitle=subtitle,
                argv=install_argv(app),
            ))
        else:
            self._run(RemoteTaskDialog(
                self, host=self.host, title=title, subtitle=subtitle,
                command=install_command(app, state),
            ))

    def remove(self, app) -> None:
        state = self.states.get(app.key)
        if state is None:
            return
        if app.cli:
            detail = (
                "Le programme et son entrée de menu seront retirés. La "
                "configuration et les sessions (~/.config/opencode, "
                "~/.local/share/opencode) ne sont pas touchées."
            )
        else:
            detail = (
                "L'AppImage et son entrée de menu seront retirées. La "
                "configuration, les sauvegardes et les jeux ne sont pas "
                "touchés."
            )
        answer = QMessageBox.question(
            self, f"Supprimer {app.name}",
            f"Supprimer {app.name} ?\n\n{detail}",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        subtitle = "Programme et entrée de menu — configuration conservée."
        if self.host is None:
            self._run(LocalTaskDialog(
                self, title=f"{app.name} — suppression",
                subtitle=subtitle, argv=remove_argv(app),
            ))
            return
        command = remove_command(app, state)
        if not command:
            QMessageBox.warning(
                self, f"Supprimer {app.name}",
                f"{app.remover} est absent de {display_name(self.host)} : "
                "mettre à jour l'image de ce poste pour pouvoir supprimer "
                "proprement.",
            )
            return
        self._run(RemoteTaskDialog(
            self, host=self.host, title=f"{app.name} — suppression",
            subtitle=subtitle, command=command,
        ))

    def update_esde_config(self) -> None:
        answer = QMessageBox.question(
            self, "Configuration d'ES-DE",
            "Recopier les fichiers Gablue d'ES-DE (custom_systems et lanceurs "
            "Tools) ?\n\nLes lanceurs personnalisés déjà présents ne sont pas "
            "écrasés.",
            QMessageBox.Yes | QMessageBox.No, QMessageBox.No,
        )
        if answer != QMessageBox.Yes:
            return
        subtitle = (
            "custom_systems et lanceurs Tools, sans toucher à l'AppImage."
        )
        if self.host is not None:
            state = self.states.get("esde")
            command = config_command(state) if state else ""
            if not command:
                QMessageBox.warning(
                    self, "Configuration d'ES-DE",
                    f"esde-install de {display_name(self.host)} ne connaît pas "
                    "« --config-only » : mettre à jour l'image de ce poste.",
                )
                return
            self._run(RemoteTaskDialog(
                self, host=self.host, title="ES-DE — configuration",
                subtitle=subtitle, command=command,
            ))
            return
        script = _which("esde-install")
        if script_has_option(script, "--config-only"):
            self._run(LocalTaskDialog(
                self, title="ES-DE — configuration", subtitle=subtitle,
                argv=config_argv(),
            ))
            return
        # Image plus ancienne que le manager : le script ignore l'option et
        # relancerait une réinstallation interactive. Même copie, sans shell.
        try:
            copied = copy_esde_config()
        except OSError as error:
            QMessageBox.warning(
                self, "Configuration d'ES-DE",
                f"La copie a échoué :\n\n{error}",
            )
            return
        if copied:
            detail = "Configuration Gablue mise à jour :\n\n" + "\n".join(
                f"• {name}" for name in copied
            )
        else:
            detail = (
                "Rien à copier : les fichiers Gablue d'ES-DE sont absents de "
                "l'image."
            )
        QMessageBox.information(self, "Configuration d'ES-DE", detail)

    def _run(self, dialog) -> None:
        dialog.completed.connect(lambda _ok: self.refresh())
        start_task(self, dialog)
