"""Dialogue de débogage sans fil Android 11+ (appairage par code)."""

from PySide6.QtWidgets import (
    QDialog, QDialogButtonBox, QFormLayout, QLabel, QLineEdit, QVBoxLayout,
)

# =============================================================================
# DIALOGUE D'APPAIRAGE SANS FIL
# =============================================================================

class PairDialog(QDialog):
    """Dialogue pour le débogage sans fil (Android 11+, sans USB)."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setWindowTitle("Débogage sans fil")
        self.setMinimumWidth(420)

        layout = QVBoxLayout(self)
        layout.setSpacing(12)

        info = QLabel(
            "Sur le téléphone, activez le débogage sans fil :\n"
            "Options développeur → Débogage sans fil\n\n"
            "1. Recopiez l'adresse d'appairage et le code\n"
            "2. Après appairage, recopiez l'adresse de connexion"
        )
        info.setWordWrap(True)
        layout.addWidget(info)

        form = QFormLayout()
        form.setSpacing(8)

        self.pair_edit = QLineEdit()
        self.pair_edit.setPlaceholderText("192.168.1.42:38783")
        form.addRow("Adresse d'appairage :", self.pair_edit)

        self.code_edit = QLineEdit()
        self.code_edit.setPlaceholderText("123456")
        form.addRow("Code d'appairage :", self.code_edit)

        self.connect_edit = QLineEdit()
        self.connect_edit.setPlaceholderText("192.168.1.42:43667")
        form.addRow("Adresse de connexion :", self.connect_edit)

        layout.addLayout(form)

        note = QLabel(
            "L'adresse de connexion s'affiche sur le téléphone "
            "après l'appairage."
        )
        note.setWordWrap(True)
        note.setStyleSheet("font-size: 11px; color: #888;")
        layout.addWidget(note)

        buttons = QDialogButtonBox(
            QDialogButtonBox.Ok | QDialogButtonBox.Cancel
        )
        buttons.accepted.connect(self.accept)
        buttons.rejected.connect(self.reject)
        layout.addWidget(buttons)

    def get_values(self):
        """Retourne (pair_addr, code, connect_addr) ou (None, None, None)."""
        if self.result() == QDialog.Accepted:
            pair = self.pair_edit.text().strip()
            code = self.code_edit.text().strip()
            conn = self.connect_edit.text().strip()
            if pair and code and conn:
                return pair, code, conn
        return None, None, None
