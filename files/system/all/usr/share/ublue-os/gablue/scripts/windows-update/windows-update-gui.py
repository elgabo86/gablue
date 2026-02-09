#!/usr/bin/env python3

"""
Interface graphique moderne pour windows-update
Utilise PySide6 pour une expérience utilisateur stylée et gaming
"""

import sys
import subprocess
import re
from pathlib import Path
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QProgressBar, QPlainTextEdit, QCheckBox,
    QFrame, QScrollArea, QSizePolicy, QStackedWidget
)
from PySide6.QtCore import (
    Qt, QThread, Signal, QTimer, QSize
)
from PySide6.QtGui import (
    QFont, QFontDatabase, QColor, QPalette, QLinearGradient,
    QBrush, QPainter, QFontMetrics
)


# ============================================================================
# CONSTANTES ET CONFIGURATION
# ============================================================================

WINDOW_WIDTH = 900
WINDOW_HEIGHT = 650

# Couleurs - Thème sombre gaming
COLORS = {
    'bg_dark': '#121218',
    'bg_card': '#1c1c24',
    'bg_hover': '#262630',
    'primary': '#0096ff',
    'primary_light': '#64c8ff',
    'success': '#00c864',
    'warning': '#ffb400',
    'error': '#ff5050',
    'text': '#f0f0f5',
    'text_secondary': '#a0a0aa',
    'border': '#32323c',
    'progress_bg': '#282832',
    'log_bg': '#16161c',
}


# ============================================================================
# THREAD DE TRAITEMENT
# ============================================================================

class InstallThread(QThread):
    """Thread pour exécuter le script bash en arrière-plan"""
    
    log_signal = Signal(str)
    finished_signal = Signal(bool, str)
    
    def __init__(self, runner_choice, dxvk_choice, force_rebuild=False):
        super().__init__()
        self.runner_choice = runner_choice
        self.dxvk_choice = dxvk_choice
        self.force_rebuild = force_rebuild
        self.process = None
        self._is_running = True
        
    def run(self):
        """Exécuter le script bash"""
        script_path = Path(__file__).parent / "windows-update-core"
        
        if not script_path.exists():
            self.finished_signal.emit(False, f"Script non trouvé: {script_path}")
            return
        
        # Construire les arguments
        args = [str(script_path), "--gui"]
        
        if self.force_rebuild:
            args.append("--rebuild")
            self.log_signal.emit("[WARN] Rebuild forcé activé")
        
        if self.dxvk_choice == "standard":
            args.append("--dxvk-no-async")
        
        # Variable d'environnement pour le runner
        env = dict(subprocess.os.environ)
        if self.runner_choice:
            env["RUNNER_CHOICE"] = str(self.runner_choice)
        
        self.log_signal.emit(f"[INFO] Démarrage du script: {' '.join(args)}")
        
        try:
            self.process = subprocess.Popen(
                args,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                universal_newlines=True,
                bufsize=1,
                env=env
            )
            
            # Lire la sortie ligne par ligne
            if self.process.stdout:
                for line in self.process.stdout:
                    if not self._is_running:
                        break
                    line = line.strip()
                    if line:
                        self.log_signal.emit(line)
            
            # Attendre la fin
            return_code = self.process.wait()
            
            if return_code == 0:
                self.finished_signal.emit(True, "Installation terminée avec succès")
            else:
                self.finished_signal.emit(False, f"Code de retour {return_code}")
                
        except Exception as e:
            self.finished_signal.emit(False, f"Exception - {str(e)}")
    
    def stop(self):
        """Arrêter le thread"""
        self._is_running = False
        if self.process and self.process.poll() is None:
            self.process.terminate()


# ============================================================================
# WIDGETS PERSONNALISÉS
# ============================================================================

class SelectionCard(QFrame):
    """Carte de sélection moderne"""
    
    def __init__(self, title, description, value, parent=None):
        super().__init__(parent)
        self.title = title
        self.description = description
        self.value = value
        self._selected = False
        self._hovered = False
        
        self.setFixedSize(350, 60)
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.setFrameStyle(QFrame.Shape.NoFrame)
        
        # Layout
        layout = QHBoxLayout(self)
        layout.setContentsMargins(15, 10, 15, 10)
        layout.setSpacing(10)
        
        # Indicateur de sélection
        self.indicator = QLabel("●")
        self.indicator.setFixedSize(20, 20)
        self.indicator.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self.indicator)
        
        # Zone texte
        text_layout = QVBoxLayout()
        text_layout.setSpacing(4)
        
        self.title_label = QLabel(title)
        self.title_label.setFont(QFont("JetBrains Mono", 11, QFont.Weight.Bold))
        text_layout.addWidget(self.title_label)
        
        self.desc_label = QLabel(description)
        self.desc_label.setFont(QFont("JetBrains Mono", 9))
        text_layout.addWidget(self.desc_label)
        
        layout.addLayout(text_layout, 1)
        
        self.update_style()
        
    @property
    def selected(self):
        return self._selected
    
    @selected.setter
    def selected(self, value):
        self._selected = value
        self.update_style()
        
    def enterEvent(self, event):
        self._hovered = True
        self.update_style()
        super().enterEvent(event)
        
    def leaveEvent(self, event):
        self._hovered = False
        self.update_style()
        super().leaveEvent(event)
        
    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self.selected = True
            # Notifier le parent de sélection
            if hasattr(self, 'selection_parent'):
                self.selection_parent.card_selected(self)
        super().mousePressEvent(event)
        
    def update_style(self):
        """Mettre à jour le style selon l'état"""
        if self._selected:
            self.setStyleSheet(f"""
                SelectionCard {{
                    background-color: {COLORS['primary']};
                    border: 2px solid {COLORS['primary_light']};
                    border-radius: 12px;
                }}
                QLabel {{
                    color: {COLORS['bg_dark']};
                    background: transparent;
                }}
            """)
            self.indicator.setStyleSheet(f"color: {COLORS['text']};")
        elif self._hovered:
            self.setStyleSheet(f"""
                SelectionCard {{
                    background-color: {COLORS['bg_hover']};
                    border: 2px solid {COLORS['primary']};
                    border-radius: 12px;
                }}
                QLabel {{
                    color: {COLORS['text']};
                    background: transparent;
                }}
            """)
            self.indicator.setStyleSheet(f"color: {COLORS['border']};")
        else:
            self.setStyleSheet(f"""
                SelectionCard {{
                    background-color: {COLORS['bg_card']};
                    border: 2px solid {COLORS['border']};
                    border-radius: 12px;
                }}
                QLabel {{
                    color: {COLORS['text_secondary']};
                    background: transparent;
                }}
            """)
            self.indicator.setStyleSheet(f"color: {COLORS['border']};")


class ModernCheckbox(QCheckBox):
    """Case à cocher moderne"""
    
    def __init__(self, text, parent=None):
        super().__init__(text, parent)
        self.setFont(QFont("JetBrains Mono", 10))
        self.setStyleSheet(f"""
            QCheckBox {{
                color: {COLORS['text']};
                spacing: 10px;
            }}
            QCheckBox::indicator {{
                width: 20px;
                height: 20px;
                border: 2px solid {COLORS['border']};
                border-radius: 4px;
                background: {COLORS['bg_card']};
            }}
            QCheckBox::indicator:checked {{
                background: {COLORS['primary']};
                border: 2px solid {COLORS['primary']};
            }}
            QCheckBox::indicator:hover {{
                border: 2px solid {COLORS['primary']};
            }}
        """)


class ModernButton(QPushButton):
    """Bouton moderne"""
    
    def __init__(self, text, color_key='primary', parent=None):
        super().__init__(text, parent)
        self.color_key = color_key
        self.setFixedHeight(50)
        self.setFont(QFont("JetBrains Mono", 11, QFont.Weight.Bold))
        self.setCursor(Qt.CursorShape.PointingHandCursor)
        self.update_style()
        
    def update_style(self):
        color = COLORS[self.color_key]
        self.setStyleSheet(f"""
            QPushButton {{
                background-color: {COLORS['bg_card']};
                color: {COLORS['text']};
                border: 1px solid {COLORS['border']};
                border-radius: 8px;
                padding: 10px 20px;
            }}
            QPushButton:hover {{
                background-color: {COLORS['bg_hover']};
                border: 2px solid {color};
            }}
            QPushButton:pressed {{
                background-color: {color};
                color: {COLORS['bg_dark']};
            }}
            QPushButton:disabled {{
                background-color: {COLORS['bg_card']};
                color: {COLORS['text_secondary']};
            }}
        """)


class StyledProgressBar(QProgressBar):
    """Barre de progression stylisée"""
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setFixedHeight(50)
        self.setTextVisible(True)
        self.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.setFont(QFont("JetBrains Mono", 12, QFont.Weight.Bold))
        self.setStyleSheet(f"""
            QProgressBar {{
                background-color: {COLORS['progress_bg']};
                border: 1px solid {COLORS['border']};
                border-radius: 8px;
                text-align: center;
                color: {COLORS['text']};
            }}
            QProgressBar::chunk {{
                background-color: {COLORS['primary']};
                border-radius: 7px;
            }}
        """)


class LogViewer(QPlainTextEdit):
    """Visionneur de logs en temps réel"""
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.setReadOnly(True)
        self.setFont(QFont("JetBrains Mono", 10))
        self.setStyleSheet(f"""
            QPlainTextEdit {{
                background-color: {COLORS['log_bg']};
                color: {COLORS['text']};
                border: 1px solid {COLORS['border']};
                border-radius: 8px;
                padding: 10px;
            }}
        """)
        self.setMaximumBlockCount(100)
        
    def add_log(self, text, level="INFO"):
        """Ajouter une ligne de log"""
        color = COLORS['text']
        if level == "ERROR":
            color = COLORS['error']
        elif level == "WARN":
            color = COLORS['warning']
        elif level == "SUCCESS":
            color = COLORS['success']
        
        # Ajouter la ligne (QPlainTextEdit ne supporte pas le HTML)
        self.appendPlainText(text)
        # Scroll vers le bas
        scrollbar = self.verticalScrollBar()
        scrollbar.setValue(scrollbar.maximum())


# ============================================================================
# ÉCRANS DE L'APPLICATION
# ============================================================================

class SelectionScreen(QWidget):
    """Écran de sélection des options"""
    
    start_signal = Signal(str, str, bool)
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(40, 20, 40, 20)
        layout.setSpacing(15)
        
        # Titre
        title = QLabel("Windows Update")
        title.setFont(QFont("JetBrains Mono", 24, QFont.Weight.Bold))
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title.setStyleSheet(f"color: {COLORS['text']};")
        layout.addWidget(title)
        
        # Sous-titre
        subtitle = QLabel("Configuration de l'environnement Windows pour Gablue")
        subtitle.setFont(QFont("JetBrains Mono", 11))
        subtitle.setAlignment(Qt.AlignmentFlag.AlignCenter)
        subtitle.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(subtitle)
        
        # Espace
        layout.addSpacing(15)
        
        # Zone de sélection (2 colonnes)
        selection_layout = QHBoxLayout()
        selection_layout.setSpacing(20)
        
        # Colonne Wine
        wine_layout = QVBoxLayout()
        wine_title = QLabel("Version de Wine")
        wine_title.setFont(QFont("JetBrains Mono", 14, QFont.Weight.Bold))
        wine_title.setStyleSheet(f"color: {COLORS['primary']};")
        wine_layout.addWidget(wine_title)
        
        self.runner_cards = []
        runners = [
            ("Gwine-Proton", "Version optimisée pour le gaming (recommandé)", "gwine-proton"),
            ("Gwine", "Version standard", "gwine"),
            ("Gwine-Proton-WoW64", "Version WoW64 expérimentale", "gwine-proton-wow64"),
            ("Soda", "Version stable legacy", "soda"),
        ]
        
        for title, desc, value in runners:
            card = SelectionCard(title, desc, value)
            card.selection_parent = self  # Pour la gestion de sélection
            self.runner_cards.append(card)
            wine_layout.addWidget(card)
            wine_layout.addSpacing(8)
        
        # Sélection par défaut
        self.runner_cards[0].selected = True
        
        wine_layout.addStretch()
        selection_layout.addLayout(wine_layout, 1)
        
        # Colonne DXVK
        dxvk_layout = QVBoxLayout()
        dxvk_title = QLabel("Type de DXVK")
        dxvk_title.setFont(QFont("JetBrains Mono", 14, QFont.Weight.Bold))
        dxvk_title.setStyleSheet(f"color: {COLORS['primary']};")
        dxvk_layout.addWidget(dxvk_title)
        
        self.dxvk_cards = []
        dxvk_options = [
            ("DXVK GPLAsync", "Recommandé - Moins de sacades, shaders en arrière-plan", "gplasync"),
            ("DXVK Standard", "Moins d'artefacts visuels", "standard"),
        ]
        
        for title, desc, value in dxvk_options:
            card = SelectionCard(title, desc, value)
            card.selection_parent = self
            self.dxvk_cards.append(card)
            dxvk_layout.addWidget(card)
            dxvk_layout.addSpacing(8)
        
        # Sélection par défaut
        self.dxvk_cards[0].selected = True
        
        # Checkbox rebuild forcé
        dxvk_layout.addSpacing(15)
        self.force_rebuild_checkbox = ModernCheckbox("Forcer le rebuild complet")
        dxvk_layout.addWidget(self.force_rebuild_checkbox)
        
        dxvk_layout.addStretch()
        selection_layout.addLayout(dxvk_layout, 1)
        
        layout.addLayout(selection_layout)
        
        # Ligne de séparation
        line = QFrame()
        line.setFrameShape(QFrame.Shape.HLine)
        line.setStyleSheet(f"background-color: {COLORS['border']};")
        line.setFixedHeight(1)
        layout.addWidget(line)
        
        # Bouton démarrer
        button_layout = QHBoxLayout()
        button_layout.addStretch()
        
        self.start_button = ModernButton("Démarrer l'installation", "primary")
        self.start_button.setFixedSize(240, 45)
        self.start_button.clicked.connect(self.start_installation)
        button_layout.addWidget(self.start_button)
        
        button_layout.addStretch()
        layout.addLayout(button_layout)
        
        # Info bulle
        info = QLabel("ℹ️  Les paramètres recommandés sont présélectionnés")
        info.setFont(QFont("JetBrains Mono", 9))
        info.setAlignment(Qt.AlignmentFlag.AlignCenter)
        info.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(info)
        
    def card_selected(self, selected_card):
        """Gérer la sélection d'une carte"""
        # Désélectionner les autres cartes du même groupe
        if selected_card in self.runner_cards:
            for card in self.runner_cards:
                if card != selected_card:
                    card.selected = False
        elif selected_card in self.dxvk_cards:
            for card in self.dxvk_cards:
                if card != selected_card:
                    card.selected = False
                    
    def start_installation(self):
        """Démarrer l'installation"""
        runner_choice = None
        dxvk_choice = None
        
        for card in self.runner_cards:
            if card.selected:
                runner_choice = card.value
                break
        
        for card in self.dxvk_cards:
            if card.selected:
                dxvk_choice = card.value
                break
        
        if runner_choice and dxvk_choice:
            self.start_signal.emit(
                runner_choice,
                dxvk_choice,
                self.force_rebuild_checkbox.isChecked()
            )


class ProgressScreen(QWidget):
    """Écran de progression de l'installation"""
    
    cancel_signal = Signal()
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 50, 50, 30)
        layout.setSpacing(20)
        
        # Barre de progression
        self.progress_bar = StyledProgressBar()
        layout.addWidget(self.progress_bar)
        
        # Logs
        log_label = QLabel("Logs")
        log_label.setFont(QFont("JetBrains Mono", 11))
        log_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(log_label)
        
        self.log_viewer = LogViewer()
        layout.addWidget(self.log_viewer, 1)
        
        # Bouton annuler
        button_layout = QHBoxLayout()
        button_layout.addStretch()
        
        self.cancel_button = ModernButton("Annuler", "error")
        self.cancel_button.setFixedSize(120, 40)
        self.cancel_button.clicked.connect(self.cancel_installation)
        button_layout.addWidget(self.cancel_button)
        
        button_layout.addStretch()
        layout.addLayout(button_layout)
        
    def set_progress(self, value, text=""):
        """Mettre à jour la progression"""
        self.progress_bar.setValue(int(value))
        if text:
            self.progress_bar.setFormat(f"{text} - %p%")
        else:
            self.progress_bar.setFormat("%p%")
            
    def add_log(self, text, level="INFO"):
        """Ajouter un log"""
        self.log_viewer.add_log(text, level)
        
    def cancel_installation(self):
        """Annuler l'installation"""
        self.cancel_signal.emit()


class FinishScreen(QWidget):
    """Écran de fin (succès ou erreur)"""
    
    close_signal = Signal()
    
    def __init__(self, success=True, parent=None):
        super().__init__(parent)
        self.success = success
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 50, 50, 50)
        layout.setSpacing(20)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        # Icône (simulée avec un label stylisé)
        icon_label = QLabel()
        icon_label.setFixedSize(120, 120)
        icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        if self.success:
            icon_label.setText("✓")
            icon_label.setStyleSheet(f"""
                QLabel {{
                    color: {COLORS['bg_dark']};
                    background-color: {COLORS['success']};
                    border: 3px solid {COLORS['text']};
                    border-radius: 60px;
                    font-size: 60px;
                    font-weight: bold;
                }}
            """)
        else:
            icon_label.setText("✕")
            icon_label.setStyleSheet(f"""
                QLabel {{
                    color: {COLORS['bg_dark']};
                    background-color: {COLORS['error']};
                    border: 3px solid {COLORS['text']};
                    border-radius: 60px;
                    font-size: 60px;
                    font-weight: bold;
                }}
            """)
        
        layout.addWidget(icon_label, alignment=Qt.AlignmentFlag.AlignCenter)
        
        # Texte
        if self.success:
            text = QLabel("Installation réussie !")
            text.setStyleSheet(f"color: {COLORS['success']};")
        else:
            text = QLabel("Installation échouée")
            text.setStyleSheet(f"color: {COLORS['error']};")
        
        text.setFont(QFont("JetBrains Mono", 20, QFont.Weight.Bold))
        text.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(text)
        
        # Description
        if self.success:
            desc = QLabel("L'environnement Windows est prêt à être utilisé.")
        else:
            desc = QLabel("Consultez les logs pour plus de détails.")
        
        desc.setFont(QFont("JetBrains Mono", 11))
        desc.setStyleSheet(f"color: {COLORS['text_secondary']};")
        desc.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(desc)
        
        layout.addSpacing(40)
        
        # Bouton
        if self.success:
            button = ModernButton("Terminer", "success")
        else:
            button = ModernButton("Fermer", "error")
        
        button.setFixedSize(160, 50)
        button.clicked.connect(self.close_signal.emit)
        layout.addWidget(button, alignment=Qt.AlignmentFlag.AlignCenter)
        
        layout.addStretch()


class ErrorScreen(QWidget):
    """Écran d'erreur avec logs visibles"""
    
    close_signal = Signal()
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 30, 50, 30)
        layout.setSpacing(15)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        # Icône d'erreur
        icon_label = QLabel("✕")
        icon_label.setFixedSize(80, 80)
        icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        icon_label.setStyleSheet(f"""
            QLabel {{
                color: {COLORS['bg_dark']};
                background-color: {COLORS['error']};
                border: 2px solid {COLORS['text']};
                border-radius: 40px;
                font-size: 40px;
                font-weight: bold;
            }}
        """)
        layout.addWidget(icon_label, alignment=Qt.AlignmentFlag.AlignCenter)
        
        # Texte d'erreur
        error_text = QLabel("Installation échouée")
        error_text.setFont(QFont("JetBrains Mono", 16, QFont.Weight.Bold))
        error_text.setStyleSheet(f"color: {COLORS['error']};")
        error_text.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(error_text)
        
        # Logs
        log_label = QLabel("Logs de l'erreur:")
        log_label.setFont(QFont("JetBrains Mono", 10))
        log_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(log_label)
        
        self.log_viewer = LogViewer()
        self.log_viewer.setFixedHeight(350)
        layout.addWidget(self.log_viewer)
        
        # Bouton fermer
        button_layout = QHBoxLayout()
        button_layout.addStretch()
        
        button = ModernButton("Fermer", "error")
        button.setFixedSize(120, 40)
        button.clicked.connect(self.close_signal.emit)
        button_layout.addWidget(button)
        
        button_layout.addStretch()
        layout.addLayout(button_layout)
        
    def add_log(self, text, level="INFO"):
        """Ajouter un log"""
        self.log_viewer.add_log(text, level)


# ============================================================================
# FENÊTRE PRINCIPALE
# ============================================================================

class WindowsUpdateGUI(QMainWindow):
    """Application principale"""
    
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Windows Update - Gablue")
        self.setFixedSize(WINDOW_WIDTH, WINDOW_HEIGHT)
        
        # Thread d'installation
        self.install_thread = None
        
        # Widget central avec stack
        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        
        layout = QVBoxLayout(self.central_widget)
        layout.setContentsMargins(0, 0, 0, 0)
        
        self.stack = QStackedWidget()
        layout.addWidget(self.stack)
        
        # Écrans
        self.selection_screen = SelectionScreen()
        self.selection_screen.start_signal.connect(self.start_installation)
        self.stack.addWidget(self.selection_screen)
        
        self.progress_screen = ProgressScreen()
        self.progress_screen.cancel_signal.connect(self.cancel_installation)
        self.stack.addWidget(self.progress_screen)
        
        self.finish_screen = None
        self.error_screen = None
        
        # Thème sombre global
        self.apply_dark_theme()
        
    def apply_dark_theme(self):
        """Appliquer le thème sombre gaming"""
        self.setStyleSheet(f"""
            QMainWindow {{
                background-color: {COLORS['bg_dark']};
            }}
            QWidget {{
                background-color: {COLORS['bg_dark']};
            }}
        """)
        
    def start_installation(self, runner_choice, dxvk_choice, force_rebuild):
        """Démarrer l'installation"""
        self.runner_choice = runner_choice
        self.dxvk_choice = dxvk_choice
        
        # Changer vers l'écran de progression
        self.stack.setCurrentWidget(self.progress_screen)
        self.progress_screen.add_log(
            f"Installation démarrée - Runner: {runner_choice}, DXVK: {dxvk_choice}",
            "INFO"
        )
        
        if force_rebuild:
            self.progress_screen.add_log("Rebuild forcé activé", "WARN")
        
        # Lancer le thread
        self.install_thread = InstallThread(runner_choice, dxvk_choice, force_rebuild)
        self.install_thread.log_signal.connect(self.handle_log)
        self.install_thread.finished_signal.connect(self.installation_finished)
        self.install_thread.start()
        
    def handle_log(self, line):
        """Traiter une ligne de log"""
        # Détection des messages de progression
        progress_patterns = [
            (r'Progress:\s*(\d+)%', 'progress'),
            (r'update_progress\s+(\d+)', 'progress'),
            (r'(\d+)%', 'progress'),
        ]
        
        for pattern, ptype in progress_patterns:
            match = re.search(pattern, line, re.IGNORECASE)
            if match:
                try:
                    progress = int(match.group(1))
                    self.progress_screen.set_progress(progress, line[:50])
                    break
                except:
                    pass
        else:
            # Détection du niveau de log
            level = "INFO"
            if "[ERROR]" in line or "error" in line.lower():
                level = "ERROR"
            elif "[WARN]" in line or "warn" in line.lower():
                level = "WARN"
            elif "succès" in line.lower() or "réussie" in line.lower():
                level = "SUCCESS"
            
            self.progress_screen.add_log(line, level)
            
    def installation_finished(self, success, message):
        """Installation terminée"""
        if success:
            self.progress_screen.set_progress(100, "Installation terminée avec succès !")
            
            # Créer l'écran de fin
            self.finish_screen = FinishScreen(success=True)
            self.finish_screen.close_signal.connect(self.close)
            self.stack.addWidget(self.finish_screen)
            self.stack.setCurrentWidget(self.finish_screen)
        else:
            # Créer l'écran d'erreur avec logs
            self.error_screen = ErrorScreen()
            self.error_screen.close_signal.connect(self.close)
            
            # Copier les logs
            for i in range(self.progress_screen.log_viewer.document().blockCount()):
                block = self.progress_screen.log_viewer.document().findBlockByNumber(i)
                if block.text():
                    self.error_screen.add_log(block.text())
            
            self.error_screen.add_log(f"Erreur: {message}", "ERROR")
            self.stack.addWidget(self.error_screen)
            self.stack.setCurrentWidget(self.error_screen)
            
    def cancel_installation(self):
        """Annuler l'installation"""
        if self.install_thread and self.install_thread.isRunning():
            self.install_thread.stop()
            self.install_thread.wait()
            
        # Créer l'écran d'erreur
        self.error_screen = ErrorScreen()
        self.error_screen.close_signal.connect(self.close)
        self.error_screen.add_log("Installation annulée par l'utilisateur", "WARN")
        self.stack.addWidget(self.error_screen)
        self.stack.setCurrentWidget(self.error_screen)
        
    def closeEvent(self, event):
        """Gérer la fermeture de la fenêtre"""
        if self.install_thread and self.install_thread.isRunning():
            self.install_thread.stop()
            self.install_thread.wait()
        event.accept()


# ============================================================================
# POINT D'ENTRÉE
# ============================================================================

if __name__ == "__main__":
    app = QApplication(sys.argv)
    
    # Configurer la police par défaut
    font = QFont("JetBrains Mono", 10)
    app.setFont(font)
    
    window = WindowsUpdateGUI()
    window.show()
    
    sys.exit(app.exec())
