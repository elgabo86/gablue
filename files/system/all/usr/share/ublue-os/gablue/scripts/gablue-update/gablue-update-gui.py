#!/usr/bin/env python3

"""
Interface graphique moderne pour gablue-update
Utilise PySide6 pour une expérience utilisateur stylée
"""

import sys
import subprocess
import re
from pathlib import Path
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QProgressBar, QPlainTextEdit, QFrame,
    QSizePolicy, QStackedWidget
)
from PySide6.QtCore import Qt, QThread, Signal
from PySide6.QtGui import QFont


# ============================================================================
# CONSTANTES ET CONFIGURATION
# ============================================================================

WINDOW_WIDTH = 800
WINDOW_HEIGHT = 600

# Couleurs - Thème sombre moderne
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

class UpdateThread(QThread):
    """Thread pour exécuter le script bash en arrière-plan"""
    
    log_signal = Signal(str)
    finished_signal = Signal(bool, str, str)  # success, message, state_type
    
    def __init__(self):
        super().__init__()
        self.process = None
        self._is_running = True
        self._is_cancelled = False
        self._no_updates = False
        
    def run(self):
        """Exécuter le script bash"""
        script_path = Path(__file__).parent / "gablue-update-core"
        
        if not script_path.exists():
            self.finished_signal.emit(False, f"Script non trouvé: {script_path}", "error")
            return
        
        args = [str(script_path), "--skip-reboot"]
        
        self.log_signal.emit("[INFO] Démarrage du script de mise à jour")
        
        try:
            self.process = subprocess.Popen(
                args,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                universal_newlines=True,
                bufsize=1
            )
            
            if self.process.stdout:
                for line in self.process.stdout:
                    if not self._is_running:
                        break
                    line = line.strip()
                    if line:
                        # Vérifier si pas de mises à jour
                        if line == "__NO_UPDATES__":
                            self._no_updates = True
                        else:
                            self.log_signal.emit(line)
            
            return_code = self.process.wait()
            
            # Si annulé volontairement, ne pas traiter comme une erreur
            if self._is_cancelled:
                self.finished_signal.emit(False, "Annulé par l'utilisateur", "cancelled")
            elif self._no_updates:
                self.finished_signal.emit(True, "Système déjà à jour", "no_updates")
            elif return_code == 0:
                self.finished_signal.emit(True, "Mise à jour terminée", "finished")
            else:
                self.finished_signal.emit(False, f"Code de retour {return_code}", "error")
                
        except Exception as e:
            if self._is_cancelled:
                self.finished_signal.emit(False, "Annulé par l'utilisateur", "cancelled")
            else:
                self.finished_signal.emit(False, f"Exception - {str(e)}", "error")
    
    def stop(self):
        """Arrêter le thread (annulation volontaire)"""
        self._is_cancelled = True
        self._is_running = False
        if self.process and self.process.poll() is None:
            self.process.terminate()


# ============================================================================
# WIDGETS PERSONNALISÉS
# ============================================================================

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
        self.appendPlainText(text)
        scrollbar = self.verticalScrollBar()
        scrollbar.setValue(scrollbar.maximum())


# ============================================================================
# ÉCRANS DE L'APPLICATION
# ============================================================================

class ReadyScreen(QWidget):
    """Écran de démarrage"""
    
    start_signal = Signal()
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 30, 50, 30)
        layout.setSpacing(20)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        # Titre
        title = QLabel("Mise à jour de Gablue")
        title.setFont(QFont("JetBrains Mono", 22, QFont.Weight.Bold))
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title.setStyleSheet(f"color: {COLORS['text']};")
        layout.addWidget(title)
        
        # Sous-titre
        subtitle = QLabel("Met à jour le système et les applications Flatpak")
        subtitle.setFont(QFont("JetBrains Mono", 11))
        subtitle.setAlignment(Qt.AlignmentFlag.AlignCenter)
        subtitle.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(subtitle)
        
        layout.addSpacing(40)
        
        # Info
        info = QLabel("Cette opération va :")
        info.setFont(QFont("JetBrains Mono", 14, QFont.Weight.Bold))
        info.setAlignment(Qt.AlignmentFlag.AlignCenter)
        info.setStyleSheet(f"color: {COLORS['text']};")
        layout.addWidget(info)
        
        # Liste des étapes
        steps = [
            "• Annuler les mises à jour en attente",
            "• Mettre à jour le système (rpm-ostree)",
            "• Mettre à jour les applications Flatpak",
        ]
        
        for step in steps:
            step_label = QLabel(step)
            step_label.setFont(QFont("JetBrains Mono", 11))
            step_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            step_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
            layout.addWidget(step_label)
        
        layout.addSpacing(40)
        
        # Bouton démarrer
        button_layout = QHBoxLayout()
        button_layout.addStretch()
        
        self.start_button = ModernButton("Démarrer la mise à jour", "primary")
        self.start_button.setFixedSize(240, 50)
        self.start_button.clicked.connect(self.start_signal.emit)
        button_layout.addWidget(self.start_button)
        
        button_layout.addStretch()
        layout.addLayout(button_layout)
        
        # Info redémarrage
        info2 = QLabel("Un redémarrage sera nécessaire après la mise à jour")
        info2.setFont(QFont("JetBrains Mono", 10))
        info2.setAlignment(Qt.AlignmentFlag.AlignCenter)
        info2.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(info2)
        
        layout.addStretch()


class UpdateScreen(QWidget):
    """Écran de progression"""
    
    cancel_signal = Signal()
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 40, 50, 30)
        layout.setSpacing(20)
        
        # Barre de progression
        self.progress_bar = StyledProgressBar()
        self.progress_bar.setValue(0)
        self.progress_bar.setFormat("Prêt à démarrer...")
        layout.addWidget(self.progress_bar)
        
        # Logs
        log_label = QLabel("Journal de mise à jour")
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
        self.cancel_button.clicked.connect(self.cancel_signal.emit)
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


class FinishScreen(QWidget):
    """Écran de fin (succès)"""
    
    reboot_signal = Signal()
    later_signal = Signal()
    
    def __init__(self, no_updates=False, parent=None):
        super().__init__(parent)
        self.no_updates = no_updates
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 50, 50, 50)
        layout.setSpacing(20)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        # Icône
        icon_label = QLabel()
        icon_label.setFixedSize(100, 100)
        icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        icon_label.setText("✓")
        icon_label.setStyleSheet(f"""
            QLabel {{
                color: {COLORS['bg_dark']};
                background-color: {COLORS['success']};
                border: 3px solid {COLORS['text']};
                border-radius: 50px;
                font-size: 50px;
                font-weight: bold;
            }}
        """)
        layout.addWidget(icon_label, alignment=Qt.AlignmentFlag.AlignCenter)
        
        # Texte
        if self.no_updates:
            text = QLabel("Système déjà à jour !")
            desc = QLabel("Aucune mise à jour nécessaire. Gablue est à jour.")
            version_info = QLabel("Votre système est à jour")
            version_info.setFont(QFont("JetBrains Mono", 10))
            version_info.setAlignment(Qt.AlignmentFlag.AlignCenter)
            version_info.setStyleSheet(f"color: {COLORS['text_secondary']};")
            layout.addWidget(version_info)
        else:
            text = QLabel("Mise à jour réussie !")
            desc = QLabel("Gablue est à jour. Un redémarrage est nécessaire.")
        
        text.setFont(QFont("JetBrains Mono", 18, QFont.Weight.Bold))
        text.setStyleSheet(f"color: {COLORS['success']};")
        text.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(text)
        
        desc.setFont(QFont("JetBrains Mono", 11))
        desc.setStyleSheet(f"color: {COLORS['text_secondary']};")
        desc.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(desc)
        
        layout.addSpacing(30)
        
        # Boutons
        if not self.no_updates:
            # Bouton redémarrer
            self.reboot_button = ModernButton("Redémarrer maintenant", "success")
            self.reboot_button.setFixedSize(220, 50)
            self.reboot_button.clicked.connect(self.reboot_signal.emit)
            layout.addWidget(self.reboot_button, alignment=Qt.AlignmentFlag.AlignCenter)
            
            layout.addSpacing(10)
            
            # Bouton plus tard
            self.later_button = ModernButton("Redémarrer plus tard", "primary")
            self.later_button.setFixedSize(220, 50)
            self.later_button.clicked.connect(self.later_signal.emit)
            layout.addWidget(self.later_button, alignment=Qt.AlignmentFlag.AlignCenter)
        else:
            # Seulement un bouton fermer
            self.close_button = ModernButton("Fermer", "primary")
            self.close_button.setFixedSize(160, 50)
            self.close_button.clicked.connect(self.later_signal.emit)
            layout.addWidget(self.close_button, alignment=Qt.AlignmentFlag.AlignCenter)
        
        layout.addStretch()


class ErrorScreen(QWidget):
    """Écran d'erreur ou d'annulation"""
    
    close_signal = Signal()
    
    def __init__(self, cancelled=False, parent=None):
        super().__init__(parent)
        self.cancelled = cancelled
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 30, 50, 30)
        layout.setSpacing(15)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        if self.cancelled:
            # Icône d'annulation (warning)
            icon_label = QLabel("!")
            icon_label.setFixedSize(80, 80)
            icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            icon_label.setStyleSheet(f"""
                QLabel {{
                    color: {COLORS['bg_dark']};
                    background-color: {COLORS['warning']};
                    border: 2px solid {COLORS['text']};
                    border-radius: 40px;
                    font-size: 50px;
                    font-weight: bold;
                }}
            """)
            layout.addWidget(icon_label, alignment=Qt.AlignmentFlag.AlignCenter)
            
            # Texte d'annulation
            title_text = QLabel("Mise à jour annulée")
            title_text.setFont(QFont("JetBrains Mono", 16, QFont.Weight.Bold))
            title_text.setStyleSheet(f"color: {COLORS['warning']};")
            title_text.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(title_text)
            
            # Logs
            log_label = QLabel("Logs:")
            log_label.setFont(QFont("JetBrains Mono", 10))
            log_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
            layout.addWidget(log_label)
            
            self.log_viewer = LogViewer()
            self.log_viewer.setFixedHeight(200)
            layout.addWidget(self.log_viewer)
            
            # Bouton fermer (couleur warning)
            button_layout = QHBoxLayout()
            button_layout.addStretch()
            
            self.close_button = ModernButton("Fermer", "warning")
            self.close_button.setFixedSize(160, 50)
            self.close_button.clicked.connect(self.close_signal.emit)
            button_layout.addWidget(self.close_button)
            
            button_layout.addStretch()
            layout.addLayout(button_layout)
            
        else:
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
            title_text = QLabel("Mise à jour échouée")
            title_text.setFont(QFont("JetBrains Mono", 16, QFont.Weight.Bold))
            title_text.setStyleSheet(f"color: {COLORS['error']};")
            title_text.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(title_text)
            
            # Logs
            log_label = QLabel("Logs de l'erreur:")
            log_label.setFont(QFont("JetBrains Mono", 10))
            log_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
            layout.addWidget(log_label)
            
            self.log_viewer = LogViewer()
            self.log_viewer.setFixedHeight(200)
            layout.addWidget(self.log_viewer)
            
            # Bouton fermer
            button_layout = QHBoxLayout()
            button_layout.addStretch()
            
            self.close_button = ModernButton("Fermer", "error")
            self.close_button.setFixedSize(160, 50)
            self.close_button.clicked.connect(self.close_signal.emit)
            button_layout.addWidget(self.close_button)
            
            button_layout.addStretch()
            layout.addLayout(button_layout)
        
        layout.addStretch()
        
    def add_log(self, text, level="INFO"):
        """Ajouter un log"""
        self.log_viewer.add_log(text, level)


# ============================================================================
# FENÊTRE PRINCIPALE
# ============================================================================

class GablueUpdateGUI(QMainWindow):
    """Application principale"""
    
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Mise à jour de Gablue")
        self.setFixedSize(WINDOW_WIDTH, WINDOW_HEIGHT)
        
        # Thread d'installation
        self.update_thread = None
        
        # Widget central avec stack
        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        
        layout = QVBoxLayout(self.central_widget)
        layout.setContentsMargins(0, 0, 0, 0)
        
        self.stack = QStackedWidget()
        layout.addWidget(self.stack)
        
        # Écrans
        self.ready_screen = ReadyScreen()
        self.ready_screen.start_signal.connect(self.start_update)
        self.stack.addWidget(self.ready_screen)
        
        self.update_screen = UpdateScreen()
        self.update_screen.cancel_signal.connect(self.cancel_update)
        self.stack.addWidget(self.update_screen)
        
        self.finish_screen = None
        self.no_updates_screen = None
        self.error_screen = None
        
        # Thème sombre global
        self.apply_dark_theme()
        
    def apply_dark_theme(self):
        """Appliquer le thème sombre"""
        self.setStyleSheet(f"""
            QMainWindow {{
                background-color: {COLORS['bg_dark']};
            }}
            QWidget {{
                background-color: {COLORS['bg_dark']};
            }}
        """)
        
    def start_update(self):
        """Démarrer la mise à jour"""
        self.stack.setCurrentWidget(self.update_screen)
        self.update_screen.add_log("Démarrage de la mise à jour de Gablue", "INFO")
        
        # Lancer le thread
        self.update_thread = UpdateThread()
        self.update_thread.log_signal.connect(self.handle_log)
        self.update_thread.finished_signal.connect(self.update_finished)
        self.update_thread.start()
        
    def handle_log(self, line):
        """Traiter une ligne de log"""
        # Détection des messages de progression
        progress_match = re.search(r'Progress:\s*(\d+)%', line, re.IGNORECASE)
        if progress_match:
            try:
                progress = int(progress_match.group(1))
                message = line.split('-', 1)[1].strip() if '-' in line else "Mise à jour en cours..."
                self.update_screen.set_progress(progress, message[:40])
                return
            except:
                pass
        
        # Détection du niveau de log
        level = "INFO"
        if "[ERROR]" in line:
            level = "ERROR"
            line = line.replace("[ERROR] ", "")
        elif "[WARN]" in line:
            level = "WARN"
            line = line.replace("[WARN] ", "")
        elif "[SUCCESS]" in line:
            level = "SUCCESS"
            line = line.replace("[SUCCESS] ", "")
        elif "[INFO]" in line:
            line = line.replace("[INFO] ", "")
        
        self.update_screen.add_log(line, level)
        
    def update_finished(self, success, message, state_type):
        """Mise à jour terminée"""
        if state_type == "finished":
            self.update_screen.set_progress(100, "Mise à jour terminée avec succès !")
            self.update_screen.add_log("Mise à jour terminée avec succès !", "SUCCESS")
            
            # Créer l'écran de fin
            self.finish_screen = FinishScreen(no_updates=False)
            self.finish_screen.reboot_signal.connect(self.reboot_system)
            self.finish_screen.later_signal.connect(self.close)
            self.stack.addWidget(self.finish_screen)
            self.stack.setCurrentWidget(self.finish_screen)
            
        elif state_type == "no_updates":
            self.update_screen.set_progress(100, "Système déjà à jour")
            self.update_screen.add_log("Système déjà à jour - Aucune mise à jour nécessaire", "SUCCESS")
            
            # Créer l'écran "déjà à jour"
            self.no_updates_screen = FinishScreen(no_updates=True)
            self.no_updates_screen.later_signal.connect(self.close)
            self.stack.addWidget(self.no_updates_screen)
            self.stack.setCurrentWidget(self.no_updates_screen)
            
        elif state_type == "cancelled":
            # Créer l'écran d'annulation
            self.error_screen = ErrorScreen(cancelled=True)
            self.error_screen.close_signal.connect(self.close)
            self.error_screen.add_log("Mise à jour annulée par l'utilisateur", "WARN")
            self.stack.addWidget(self.error_screen)
            self.stack.setCurrentWidget(self.error_screen)
            
        else:  # error
            # Créer l'écran d'erreur
            self.error_screen = ErrorScreen()
            self.error_screen.close_signal.connect(self.close)
            
            # Copier les logs
            for i in range(self.update_screen.log_viewer.document().blockCount()):
                block = self.update_screen.log_viewer.document().findBlockByNumber(i)
                if block.text():
                    self.error_screen.add_log(block.text())
            
            self.error_screen.add_log(f"Erreur: {message}", "ERROR")
            self.stack.addWidget(self.error_screen)
            self.stack.setCurrentWidget(self.error_screen)
            
    def cancel_update(self):
        """Annuler la mise à jour"""
        # Ajouter un log immédiat pour feedback visuel
        self.update_screen.add_log("Annulation en cours...", "WARN")
        
        if self.update_thread and self.update_thread.isRunning():
            self.update_thread.stop()
            # Ne pas créer l'écran ici, laisser update_finished le faire
            # quand le thread aura terminé proprement
        
    def reboot_system(self):
        """Redémarrer le système"""
        self.update_screen.add_log("Redémarrage du système...", "INFO")
        try:
            subprocess.Popen(["systemctl", "reboot"])
        except:
            pass
        self.close()
        
    def closeEvent(self, event):
        """Gérer la fermeture de la fenêtre"""
        if self.update_thread and self.update_thread.isRunning():
            self.update_thread.stop()
            self.update_thread.wait()
        event.accept()


# ============================================================================
# POINT D'ENTRÉE
# ============================================================================

if __name__ == "__main__":
    app = QApplication(sys.argv)
    
    # Configurer la police par défaut
    font = QFont("JetBrains Mono", 10)
    app.setFont(font)
    
    window = GablueUpdateGUI()
    window.show()
    
    sys.exit(app.exec())
