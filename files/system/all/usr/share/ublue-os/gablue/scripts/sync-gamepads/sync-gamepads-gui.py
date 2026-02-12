#!/usr/bin/env python3

"""
Interface graphique moderne pour sync-gamepads
Utilise PySide6 pour une expérience utilisateur gaming
"""

import sys
import subprocess
import re
from pathlib import Path
from collections import OrderedDict
from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QProgressBar, QPlainTextEdit, QFrame,
    QSizePolicy, QStackedWidget, QGridLayout, QScrollArea
)
from PySide6.QtCore import Qt, QThread, Signal, QTimer, QSize
from PySide6.QtGui import QFont, QColor, QPainter, QBrush, QPen


# ============================================================================
# CONSTANTES ET CONFIGURATION
# ============================================================================

WINDOW_WIDTH = 850
WINDOW_HEIGHT = 700

# Couleurs - Thème sombre gaming avec accents colorés
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
    # Couleurs spécifiques aux manettes
    'ds4': '#0064c8',           # Bleu PlayStation
    'dualsense': '#ffffff',      # Blanc DualSense
    'switch': '#e60012',         # Rouge Nintendo
    'wiiu': '#0096ff',           # Bleu Wii U
}


# ============================================================================
# THREAD DE TRAITEMENT
# ============================================================================

class ScanThread(QThread):
    """Thread pour exécuter le script bash en arrière-plan"""
    
    log_signal = Signal(str)
    finished_signal = Signal(bool, str)
    
    def __init__(self):
        super().__init__()
        self.process = None
        self._is_running = True
        
    def run(self):
        """Exécuter le script bash"""
        script_path = Path(__file__).parent / "sync-gamepads-core"
        
        if not script_path.exists():
            self.finished_signal.emit(False, f"Script non trouvé: {script_path}")
            return
        
        args = [str(script_path)]
        
        self.log_signal.emit("[INFO] Démarrage du scan Bluetooth...")
        
        try:
            self.process = subprocess.Popen(
                args,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                universal_newlines=True,
                bufsize=1
            )
            
            # Lire la sortie ligne par ligne
            if self.process.stdout:
                for line in self.process.stdout:
                    if not self._is_running:
                        break
                    line = line.strip()
                    if line:
                        self.log_signal.emit(line)
            
            return_code = self.process.wait()
            
            if return_code == 0:
                self.finished_signal.emit(True, "Scan terminé avec succès")
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


class ControllerCard(QFrame):
    """Carte représentant une manette connectée"""
    
    CONTROLLER_TYPES = {
        'ds4': {'name': 'DS4', 'color': COLORS['ds4'], 'icon': 'PS4'},
        'dualsense': {'name': 'DualSense', 'color': COLORS['dualsense'], 'icon': 'PS5'},
        'wiiu': {'name': 'Wii U Pro', 'color': COLORS['wiiu'], 'icon': 'WiiU'},
        'switch_pro': {'name': 'Switch Pro', 'color': COLORS['switch'], 'icon': 'NS'},
        'joycon_l': {'name': 'Joy-Con (G)', 'color': COLORS['switch'], 'icon': 'JC-L'},
        'joycon_r': {'name': 'Joy-Con (D)', 'color': COLORS['switch'], 'icon': 'JC-R'},
    }
    
    def __init__(self, controller_type, count=1, parent=None):
        super().__init__(parent)
        self.controller_type = controller_type
        self.count = count
        self.info = self.CONTROLLER_TYPES.get(controller_type, 
                                              {'name': 'Manette', 'color': COLORS['primary'], 'icon': '?'}) 
        self.pulse_animation = 0
        self.setFixedSize(160, 100)
        self.setFrameStyle(QFrame.Shape.NoFrame)
        
        # Layout
        layout = QVBoxLayout(self)
        layout.setContentsMargins(10, 10, 10, 10)
        layout.setSpacing(5)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        # Icône
        self.icon_label = QLabel(self.info['icon'])
        self.icon_label.setFixedSize(44, 44)
        self.icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.icon_label.setFont(QFont("JetBrains Mono", 14, QFont.Weight.Bold))
        layout.addWidget(self.icon_label, alignment=Qt.AlignmentFlag.AlignCenter)
        
        # Nom
        self.name_label = QLabel(self.info['name'])
        self.name_label.setFont(QFont("JetBrains Mono", 10))
        self.name_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self.name_label)
        
        # Compteur si > 1
        if count > 1:
            self.count_label = QLabel(f"x{count}")
            self.count_label.setFont(QFont("JetBrains Mono", 12, QFont.Weight.Bold))
            self.count_label.setStyleSheet(f"color: {COLORS['success']};")
            layout.addWidget(self.count_label, alignment=Qt.AlignmentFlag.AlignRight)
        
        self.update_style()
        
        # Timer pour l'animation
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.update_animation)
        self.timer.start(50)  # 20 FPS
        
    def update_animation(self):
        """Mettre à jour l'animation de pulsation"""
        self.pulse_animation += 0.1
        if self.pulse_animation > 2:
            self.pulse_animation = 0
        self.update()
        
    def update_style(self):
        """Mettre à jour le style"""
        self.setStyleSheet(f"""
            ControllerCard {{
                background-color: {COLORS['bg_card']};
                border: 3px solid {self.info['color']};
                border-radius: 12px;
            }}
        """)
        
        self.icon_label.setStyleSheet(f"""
            QLabel {{
                background-color: {self.info['color']};
                color: {COLORS['bg_dark']};
                border: 2px solid {COLORS['text']};
                border-radius: 22px;
            }}
        """)
        
        self.name_label.setStyleSheet(f"color: {COLORS['text']};")


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

class ScanningScreen(QWidget):
    """Écran de scan actif"""
    
    cancel_signal = Signal()
    
    def __init__(self, parent=None):
        super().__init__(parent)
        self.controllers = OrderedDict()
        self.total_connected = 0
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 40, 50, 30)
        layout.setSpacing(20)
        
        # Logs
        log_label = QLabel("Journal de synchronisation")
        log_label.setFont(QFont("JetBrains Mono", 11))
        log_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
        layout.addWidget(log_label)
        
        self.log_viewer = LogViewer()
        layout.addWidget(self.log_viewer, 1)
        
        # Zone des manettes
        self.controllers_widget = QWidget()
        self.controllers_layout = QHBoxLayout(self.controllers_widget)
        self.controllers_layout.setSpacing(15)
        self.controllers_layout.setAlignment(Qt.AlignmentFlag.AlignLeft)
        self.controllers_layout.addStretch()
        
        layout.addWidget(self.controllers_widget)
        
        # Compteur
        self.count_label = QLabel("Total: 0 manette(s)")
        self.count_label.setFont(QFont("JetBrains Mono", 14, QFont.Weight.Bold))
        self.count_label.setStyleSheet(f"color: {COLORS['success']};")
        self.count_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.addWidget(self.count_label)
        
        # Bouton terminer
        button_layout = QHBoxLayout()
        button_layout.addStretch()
        
        self.cancel_button = ModernButton("Terminer", "error")
        self.cancel_button.setFixedSize(140, 45)
        self.cancel_button.clicked.connect(self.cancel_signal.emit)
        button_layout.addWidget(self.cancel_button)
        
        button_layout.addStretch()
        layout.addLayout(button_layout)
        
    def add_log(self, text, level="INFO"):
        """Ajouter un log"""
        self.log_viewer.add_log(text, level)
        
    def add_controller(self, controller_type):
        """Ajouter une manette à la liste"""
        if controller_type in self.controllers:
            self.controllers[controller_type] += 1
        else:
            self.controllers[controller_type] = 1
        self.total_connected += 1
        self.update_controllers_display()
        
    def update_controllers_display(self):
        """Mettre à jour l'affichage des manettes"""
        # Vider le layout actuel
        while self.controllers_layout.count():
            item = self.controllers_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()
        
        # Ajouter les cartes
        for controller_type, count in self.controllers.items():
            card = ControllerCard(controller_type, count)
            self.controllers_layout.addWidget(card)
        
        self.controllers_layout.addStretch()
        
        # Mettre à jour le compteur
        if self.total_connected == 0:
            self.count_label.setText("En attente de manettes...")
            self.count_label.setStyleSheet(f"color: {COLORS['text_secondary']};")
        else:
            self.count_label.setText(f"Total: {self.total_connected} manette(s)")
            self.count_label.setStyleSheet(f"color: {COLORS['success']};")


class FinishedScreen(QWidget):
    """Écran de fin (succès)"""
    
    close_signal = Signal()
    
    def __init__(self, controllers, total_connected, parent=None):
        super().__init__(parent)
        self.controllers = controllers
        self.total_connected = total_connected
        self.init_ui()
        
    def init_ui(self):
        layout = QVBoxLayout(self)
        layout.setContentsMargins(50, 50, 50, 50)
        layout.setSpacing(20)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        
        if self.total_connected > 0:
            # Icône de succès
            icon_label = QLabel("✓")
            icon_label.setFixedSize(100, 100)
            icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
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
            success_text = QLabel("Synchronisation réussie !")
            success_text.setFont(QFont("JetBrains Mono", 18, QFont.Weight.Bold))
            success_text.setStyleSheet(f"color: {COLORS['success']};")
            success_text.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(success_text)
            
            # Description
            desc = QLabel(f"{self.total_connected} manette(s) connectée(s) avec succès")
            desc.setFont(QFont("JetBrains Mono", 11))
            desc.setStyleSheet(f"color: {COLORS['text_secondary']};")
            desc.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(desc)
            
            # Afficher les manettes
            if self.controllers:
                controllers_layout = QHBoxLayout()
                controllers_layout.setSpacing(15)
                controllers_layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
                
                for controller_type, count in self.controllers.items():
                    card = ControllerCard(controller_type, count)
                    controllers_layout.addWidget(card)
                
                layout.addLayout(controllers_layout)
        else:
            # Icône d'avertissement
            icon_label = QLabel("!")
            icon_label.setFixedSize(100, 100)
            icon_label.setAlignment(Qt.AlignmentFlag.AlignCenter)
            icon_label.setStyleSheet(f"""
                QLabel {{
                    color: {COLORS['bg_dark']};
                    background-color: {COLORS['warning']};
                    border: 3px solid {COLORS['text']};
                    border-radius: 50px;
                    font-size: 50px;
                    font-weight: bold;
                }}
            """)
            layout.addWidget(icon_label, alignment=Qt.AlignmentFlag.AlignCenter)
            
            # Texte
            warn_text = QLabel("Aucune manette détectée")
            warn_text.setFont(QFont("JetBrains Mono", 18, QFont.Weight.Bold))
            warn_text.setStyleSheet(f"color: {COLORS['warning']};")
            warn_text.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(warn_text)
            
            # Description
            desc = QLabel("Vérifiez que vos manettes sont en mode pairing")
            desc.setFont(QFont("JetBrains Mono", 11))
            desc.setStyleSheet(f"color: {COLORS['text_secondary']};")
            desc.setAlignment(Qt.AlignmentFlag.AlignCenter)
            layout.addWidget(desc)
        
        layout.addSpacing(30)
        
        # Bouton fermer
        self.close_button = ModernButton("Terminé", "success")
        self.close_button.setFixedSize(200, 50)
        self.close_button.clicked.connect(self.close_signal.emit)
        layout.addWidget(self.close_button, alignment=Qt.AlignmentFlag.AlignCenter)
        
        layout.addStretch()


class ErrorScreen(QWidget):
    """Écran d'erreur"""
    
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
        error_text = QLabel("Erreur de synchronisation")
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
        self.log_viewer.setFixedHeight(300)
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
        
    def add_log(self, text, level="INFO"):
        """Ajouter un log"""
        self.log_viewer.add_log(text, level)


# ============================================================================
# FENÊTRE PRINCIPALE
# ============================================================================

class SyncGamepadsGUI(QMainWindow):
    """Application principale"""
    
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Synchronisation des Manettes - Gablue")
        self.setFixedSize(WINDOW_WIDTH, WINDOW_HEIGHT)
        
        # Thread de scan
        self.scan_thread = None
        
        # Widget central avec stack
        self.central_widget = QWidget()
        self.setCentralWidget(self.central_widget)
        
        layout = QVBoxLayout(self.central_widget)
        layout.setContentsMargins(0, 0, 0, 0)
        
        # Titre et sous-titre
        title_layout = QVBoxLayout()
        title_layout.setSpacing(5)
        
        title = QLabel("Synchronisation des Manettes")
        title.setFont(QFont("JetBrains Mono", 22, QFont.Weight.Bold))
        title.setStyleSheet(f"color: {COLORS['text']};")
        title.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title_layout.addWidget(title)
        
        subtitle = QLabel("Connectez vos manettes Bluetooth en mode pairing")
        subtitle.setFont(QFont("JetBrains Mono", 11))
        subtitle.setStyleSheet(f"color: {COLORS['text_secondary']};")
        subtitle.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title_layout.addWidget(subtitle)
        
        layout.addLayout(title_layout)
        
        # Stack pour les écrans
        self.stack = QStackedWidget()
        layout.addWidget(self.stack)
        
        # Écrans
        self.scanning_screen = ScanningScreen()
        self.scanning_screen.cancel_signal.connect(self.finish_scan)
        self.stack.addWidget(self.scanning_screen)
        
        self.finished_screen = None
        self.error_screen = None
        
        # Thème sombre global
        self.apply_dark_theme()
        
        # Démarrer le scan immédiatement
        self.start_scan()
        
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
        
    def start_scan(self):
        """Démarrer le scan Bluetooth"""
        self.scanning_screen.add_log("Démarrage de la synchronisation...", "INFO")
        
        # Lancer le thread
        self.scan_thread = ScanThread()
        self.scan_thread.log_signal.connect(self.handle_log)
        self.scan_thread.finished_signal.connect(self.scan_finished)
        self.scan_thread.start()
        
    def handle_log(self, line):
        """Traiter une ligne de log"""
        # Ignorer les messages de progression
        if re.search(r'Progress:', line, re.IGNORECASE):
            return
        
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
        
        # Détection des types de manettes
        if "Type: DS4" in line:
            self.scanning_screen.add_controller('ds4')
        elif "Type: DualSense" in line:
            self.scanning_screen.add_controller('dualsense')
        elif "Type: Wii U Pro" in line:
            self.scanning_screen.add_controller('wiiu')
        elif "Type: Switch Pro" in line:
            self.scanning_screen.add_controller('switch_pro')
        elif "Type: Joy-Con Gauche" in line:
            self.scanning_screen.add_controller('joycon_l')
        elif "Type: Joy-Con Droite" in line:
            self.scanning_screen.add_controller('joycon_r')
        elif "Connexion réussie" in line:
            # Extraire le nom de la manette
            match = re.search(r'à\s+(.+?)\s+!$', line)
            if match:
                controller_name = match.group(1)
                self.scanning_screen.add_log(f"✓ {controller_name} connectée", "SUCCESS")
                return
        
        # Ajouter à la vue des logs
        self.scanning_screen.add_log(line, level)
        
    def scan_finished(self, success, message):
        """Scan terminé"""
        if success:
            self.scanning_screen.add_log("Scan terminé avec succès", "SUCCESS")
        else:
            self.scanning_screen.add_log(f"Erreur: {message}", "ERROR")
            
    def finish_scan(self):
        """Terminer le scan et afficher l'écran de fin"""
        if self.scan_thread and self.scan_thread.isRunning():
            self.scan_thread.stop()
            self.scan_thread.wait()
        
        # Créer et afficher l'écran de fin
        self.finished_screen = FinishedScreen(
            self.scanning_screen.controllers,
            self.scanning_screen.total_connected
        )
        self.finished_screen.close_signal.connect(self.close)
        self.stack.addWidget(self.finished_screen)
        self.stack.setCurrentWidget(self.finished_screen)
        
    def closeEvent(self, event):
        """Gérer la fermeture de la fenêtre"""
        if self.scan_thread and self.scan_thread.isRunning():
            self.scan_thread.stop()
            self.scan_thread.wait()
        event.accept()


# ============================================================================
# POINT D'ENTRÉE
# ============================================================================

if __name__ == "__main__":
    app = QApplication(sys.argv)
    
    # Configurer la police par défaut
    font = QFont("JetBrains Mono", 10)
    app.setFont(font)
    
    window = SyncGamepadsGUI()
    window.show()
    
    sys.exit(app.exec())
