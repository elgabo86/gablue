#!/usr/bin/env python3

"""
Interface graphique moderne pour sync-gamepads
Utilise Pygame pour une expérience utilisateur gaming
"""

import os
os.environ['PYGAME_HIDE_SUPPORT_PROMPT'] = '1'

import pygame
import sys
import subprocess
import threading
import queue
import re
from pathlib import Path
from collections import OrderedDict

# Initialisation de Pygame
pygame.init()
pygame.font.init()

# ============================================================================
# CONSTANTES ET CONFIGURATION
# ============================================================================

SCREEN_WIDTH = 850
SCREEN_HEIGHT = 700
FPS = 60

# Couleurs - Thème sombre gaming avec accents colorés
COLORS = {
    'bg_dark': (18, 18, 24),
    'bg_card': (28, 28, 36),
    'bg_hover': (38, 38, 48),
    'primary': (0, 150, 255),
    'primary_light': (100, 200, 255),
    'success': (0, 200, 100),
    'warning': (255, 180, 0),
    'error': (255, 80, 80),
    'text': (240, 240, 245),
    'text_secondary': (160, 160, 170),
    'border': (50, 50, 60),
    'progress_bg': (40, 40, 50),
    'log_bg': (22, 22, 28),
    # Couleurs spécifiques aux manettes
    'ds4': (0, 100, 200),        # Bleu PlayStation
    'dualsense': (255, 255, 255), # Blanc DualSense
    'switch': (230, 0, 18),      # Rouge Nintendo
    'wiiu': (0, 150, 255),       # Bleu Wii U
}

# Fonts
FONT_SIZE_SMALL = 14
FONT_SIZE_NORMAL = 16
FONT_SIZE_LARGE = 20
FONT_SIZE_TITLE = 28
FONT_SIZE_HUGE = 36

try:
    FONT_SMALL = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_SMALL)
    FONT_NORMAL = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_NORMAL)
    FONT_LARGE = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_LARGE)
    FONT_TITLE = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_TITLE, bold=True)
    FONT_HUGE = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_HUGE, bold=True)
except:
    FONT_SMALL = pygame.font.SysFont("monospace", FONT_SIZE_SMALL)
    FONT_NORMAL = pygame.font.SysFont("monospace", FONT_SIZE_NORMAL)
    FONT_LARGE = pygame.font.SysFont("monospace", FONT_SIZE_LARGE)
    FONT_TITLE = pygame.font.SysFont("monospace", FONT_SIZE_TITLE, bold=True)
    FONT_HUGE = pygame.font.SysFont("monospace", FONT_SIZE_HUGE, bold=True)

# ============================================================================
# CLASSES UTILITAIRES
# ============================================================================

class Button:
    """Bouton interactif moderne"""
    
    def __init__(self, x, y, width, height, text, callback=None, color_key='primary'):
        self.rect = pygame.Rect(x, y, width, height)
        self.text = text
        self.callback = callback
        self.color_key = color_key
        self.hovered = False
        self.clicked = False
        self.enabled = True
        
    def draw(self, screen):
        if not self.enabled:
            color = COLORS['bg_card']
        elif self.clicked:
            color = COLORS[self.color_key]
        elif self.hovered:
            color = COLORS['bg_hover']
        else:
            color = COLORS['bg_card']
            
        # Fond avec bordure arrondie
        pygame.draw.rect(screen, color, self.rect, border_radius=10)
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=2, border_radius=10)
        
        # Texte
        text_color = COLORS['text'] if self.enabled else COLORS['text_secondary']
        if self.clicked:
            text_color = COLORS['bg_dark']
        text_surface = FONT_LARGE.render(self.text, True, text_color)
        text_rect = text_surface.get_rect(center=self.rect.center)
        screen.blit(text_surface, text_rect)
        
    def handle_event(self, event):
        if not self.enabled:
            return False
            
        if event.type == pygame.MOUSEMOTION:
            self.hovered = self.rect.collidepoint(event.pos)
            
        elif event.type == pygame.MOUSEBUTTONDOWN:
            if event.button == 1 and self.hovered:
                self.clicked = True
                return True
                
        elif event.type == pygame.MOUSEBUTTONUP:
            if event.button == 1 and self.clicked:
                self.clicked = False
                if self.hovered and self.callback:
                    self.callback()
                return True
                
        return False

class ProgressBar:
    """Barre de progression animée avec pulsation"""
    
    def __init__(self, x, y, width, height):
        self.rect = pygame.Rect(x, y, width, height)
        self.progress = 0
        self.target_progress = 0
        self.text = "En attente..."
        self.pulse_offset = 0
        
    def set_progress(self, value, text=""):
        self.target_progress = max(0, min(100, value))
        if text:
            self.text = text
            
    def update(self):
        # Animation fluide
        diff = self.target_progress - self.progress
        self.progress += diff * 0.1
        self.pulse_offset += 0.1
        
    def draw(self, screen):
        # Fond
        pygame.draw.rect(screen, COLORS['progress_bg'], self.rect, border_radius=10)
        
        # Barre de progression avec gradient
        if self.progress > 0:
            fill_width = int((self.progress / 100) * (self.rect.width - 4))
            fill_rect = pygame.Rect(
                self.rect.x + 2, 
                self.rect.y + 2, 
                fill_width, 
                self.rect.height - 4
            )
            
            # Gradient animé
            for i in range(fill_width):
                ratio = i / fill_width if fill_width > 0 else 0
                pulse = abs((self.pulse_offset % 2) - 1) * 30
                r = int(COLORS['primary'][0] * (1 - ratio * 0.3))
                g = int(COLORS['primary'][1] + ratio * 20 + pulse)
                b = int(COLORS['primary'][2])
                pygame.draw.line(screen, (r, g, b), 
                               (fill_rect.x + i, fill_rect.y),
                               (fill_rect.x + i, fill_rect.y + fill_rect.height))
            
        # Bordure
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=2, border_radius=10)
        
        # Pourcentage
        percent_text = f"{int(self.progress)}%"
        percent_surface = FONT_LARGE.render(percent_text, True, COLORS['text'])
        percent_rect = percent_surface.get_rect(midright=(self.rect.right - 20, self.rect.centery))
        screen.blit(percent_surface, percent_rect)
        
        # Texte de statut
        status_surface = FONT_NORMAL.render(self.text, True, COLORS['text_secondary'])
        status_rect = status_surface.get_rect(midleft=(self.rect.x + 20, self.rect.centery))
        screen.blit(status_surface, status_rect)

class ControllerCard:
    """Carte représentant une manette connectée"""
    
    CONTROLLER_TYPES = {
        'ds4': {'name': 'DS4', 'color': COLORS['ds4'], 'icon': 'PS4'},
        'dualsense': {'name': 'DualSense', 'color': COLORS['dualsense'], 'icon': 'PS5'},
        'wiiu': {'name': 'Wii U Pro', 'color': COLORS['wiiu'], 'icon': 'WiiU'},
        'switch_pro': {'name': 'Switch Pro', 'color': COLORS['switch'], 'icon': 'NS'},
        'joycon_l': {'name': 'Joy-Con (G)', 'color': COLORS['switch'], 'icon': 'JC-L'},
        'joycon_r': {'name': 'Joy-Con (D)', 'color': COLORS['switch'], 'icon': 'JC-R'},
    }
    
    def __init__(self, x, y, controller_type, count=1):
        self.rect = pygame.Rect(x, y, 160, 100)
        self.controller_type = controller_type
        self.count = count
        self.info = self.CONTROLLER_TYPES.get(controller_type, 
                                              {'name': 'Manette', 'color': COLORS['primary'], 'icon': '?'}) 
        self.animation_offset = 0
        
    def update(self):
        self.animation_offset += 0.05
        
    def draw(self, screen):
        # Animation de pulsation subtile
        pulse = abs((self.animation_offset % 2) - 1) * 3
        expanded_rect = self.rect.inflate(pulse, pulse)
        
        # Fond avec couleur spécifique à la manette
        pygame.draw.rect(screen, COLORS['bg_card'], expanded_rect, border_radius=12)
        pygame.draw.rect(screen, self.info['color'], expanded_rect, width=3, border_radius=12)
        
        # Icône (cercle avec initiales)
        icon_center = (expanded_rect.centerx, expanded_rect.y + 35)
        pygame.draw.circle(screen, self.info['color'], icon_center, 22)
        pygame.draw.circle(screen, COLORS['text'], icon_center, 22, 2)
        
        # Texte de l'icône
        icon_text = FONT_NORMAL.render(self.info['icon'], True, COLORS['bg_dark'])
        icon_rect = icon_text.get_rect(center=icon_center)
        screen.blit(icon_text, icon_rect)
        
        # Nom de la manette
        name_text = FONT_SMALL.render(self.info['name'], True, COLORS['text'])
        name_rect = name_text.get_rect(center=(expanded_rect.centerx, expanded_rect.y + 70))
        screen.blit(name_text, name_rect)
        
        # Compteur si > 1
        if self.count > 1:
            count_text = FONT_LARGE.render(f"x{self.count}", True, COLORS['success'])
            count_rect = count_text.get_rect(center=(expanded_rect.right - 25, expanded_rect.y + 25))
            screen.blit(count_text, count_rect)

class LogViewer:
    """Visionneur de logs en temps réel"""
    
    def __init__(self, x, y, width, height):
        self.rect = pygame.Rect(x, y, width, height)
        self.lines = []
        self.max_lines = 100
        
    def add_line(self, text, level="INFO"):
        color = COLORS['text']
        if level == "ERROR":
            color = COLORS['error']
        elif level == "WARN":
            color = COLORS['warning']
        elif level == "SUCCESS":
            color = COLORS['success']
            
        self.lines.append((text, color))
        if len(self.lines) > self.max_lines:
            self.lines.pop(0)
            
    def draw(self, screen):
        # Fond
        pygame.draw.rect(screen, COLORS['log_bg'], self.rect, border_radius=10)
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=1, border_radius=10)
        
        # Titre
        title_surface = FONT_NORMAL.render("Journal de synchronisation", True, COLORS['text_secondary'])
        screen.blit(title_surface, (self.rect.x + 15, self.rect.y - 28))
        
        # Lignes de log
        y_offset = 15
        for text, color in self.lines[-12:]:  # Afficher les 12 dernières lignes
            if y_offset < self.rect.height - 20:
                # Tronquer si trop long
                max_chars = (self.rect.width - 40) // 8
                if len(text) > max_chars:
                    text = text[:max_chars-3] + "..."
                    
                line_surface = FONT_SMALL.render(text, True, color)
                screen.blit(line_surface, (self.rect.x + 15, self.rect.y + y_offset))
                y_offset += 18

# ============================================================================
# CLASSE PRINCIPALE DE L'APPLICATION
# ============================================================================

class SyncGamepadsGUI:
    """Application principale"""
    
    def __init__(self):
        self.screen = pygame.display.set_mode((SCREEN_WIDTH, SCREEN_HEIGHT))
        pygame.display.set_caption("Synchronisation des Manettes - Gablue")
        self.clock = pygame.time.Clock()
        self.running = True
        self.state = "scanning"  # scanning, finished, error
        
        # Queue pour la communication avec le subprocess
        self.log_queue = queue.Queue()
        self.process = None
        
        # Données des manettes
        self.controllers = OrderedDict()
        self.total_connected = 0
        
        # UI Elements
        self.setup_ui()
        
        # Démarrer le scan immédiatement
        self.start_scan()
        
    def setup_ui(self):
        """Initialiser les éléments d'interface"""
        
        # === ÉCRAN PRÊT ===
        self.start_button = Button(
            SCREEN_WIDTH // 2 - 120, 520, 240, 55, 
            "Démarrer le scan", 
            self.start_scan
        )
        
        # === ÉCRAN DE SCAN ===
        self.log_viewer = LogViewer(50, 120, SCREEN_WIDTH - 100, 340)
        
        # Bouton terminer (pour arrêter et fermer)
        self.cancel_button = Button(
            SCREEN_WIDTH // 2 - 70, 580, 140, 45,
            "Terminer",
            self.finish_and_quit,
            color_key='error'
        )
        
        # === ÉCRAN DE FIN ===
        self.finish_button = Button(
            SCREEN_WIDTH // 2 - 100, 620, 200, 50,
            "Terminé",
            self.quit,
            color_key='success'
        )
        
        self.error_button = Button(
            SCREEN_WIDTH // 2 - 80, 620, 160, 50,
            "Fermer",
            self.quit,
            color_key='error'
        )
        
    def start_scan(self):
        """Démarrer le scan Bluetooth"""
        self.state = "scanning"
        self.controllers.clear()
        self.total_connected = 0
        self.log_viewer.lines.clear()
        self.log_viewer.add_line("Démarrage de la synchronisation...", "INFO")
        self.start_bash_script()
        
    def start_bash_script(self):
        """Lancer le script bash et capturer sa sortie"""
        
        def run_script():
            script_path = Path(__file__).parent / "sync-gamepads-core"
            
            # Vérifier que le script existe
            if not script_path.exists():
                self.log_queue.put("__ERROR__:Script non trouvé: {script_path}")
                return
            
            # Construire les arguments
            args = [str(script_path)]
            
            # Log de démarrage
            self.log_queue.put("[INFO] Démarrage du scan Bluetooth...")
            
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
                        line = line.strip()
                        if line:
                            self.log_queue.put(line)
                        
                # Attendre la fin
                return_code = self.process.wait()
                
                if return_code == 0:
                    self.log_queue.put("__SUCCESS__")
                else:
                    self.log_queue.put(f"__ERROR__:Code de retour {return_code}")
                    
            except Exception as e:
                self.log_queue.put(f"__ERROR__:Exception - {str(e)}")
                
        # Lancer dans un thread séparé
        thread = threading.Thread(target=run_script, daemon=True)
        thread.start()
        
    def finish_and_quit(self):
        """Arrêter le scan et fermer l'application"""
        if self.process and self.process.poll() is None:
            self.process.terminate()
        self.running = False
            
    def quit(self):
        """Quitter l'application"""
        if self.process and self.process.poll() is None:
            self.process.terminate()
        self.running = False
        
    def parse_log_line(self, line):
        """Parser une ligne de log et mettre à jour l'interface"""
        
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
            self.add_controller('ds4')
        elif "Type: DualSense" in line:
            self.add_controller('dualsense')
        elif "Type: Wii U Pro" in line:
            self.add_controller('wiiu')
        elif "Type: Switch Pro" in line:
            self.add_controller('switch_pro')
        elif "Type: Joy-Con Gauche" in line:
            self.add_controller('joycon_l')
        elif "Type: Joy-Con Droite" in line:
            self.add_controller('joycon_r')
        elif "Connexion réussie" in line:
            # Extraire le nom de la manette
            match = re.search(r'à\s+(.+?)\s+!$', line)
            if match:
                controller_name = match.group(1)
                self.log_viewer.add_line(f"✓ {controller_name} connectée", "SUCCESS")
                return
            
        # Ajouter à la vue des logs
        self.log_viewer.add_line(line, level)
        
    def add_controller(self, controller_type):
        """Ajouter une manette à la liste"""
        if controller_type in self.controllers:
            self.controllers[controller_type] += 1
        else:
            self.controllers[controller_type] = 1
        self.total_connected += 1
        
    def handle_events(self):
        """Gérer les événements Pygame"""
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                self.quit()
                return
                
            if event.type == pygame.KEYDOWN:
                if event.key == pygame.K_ESCAPE:
                    self.finish_and_quit()
                        
            # Écran de scan
            if self.state == "scanning":
                self.cancel_button.handle_event(event)
                
            # Écran de fin
            elif self.state == "finished":
                self.finish_button.handle_event(event)
                
            # Écran d'erreur
            elif self.state == "error":
                self.error_button.handle_event(event)
                    
    def update(self):
        """Mettre à jour la logique de l'application"""
        
        # Traiter les messages de la queue
        try:
            while True:
                line = self.log_queue.get_nowait()
                
                if line == "__SUCCESS__":
                    self.log_viewer.add_line("Scan actif - Prêt à connecter", "SUCCESS")
                elif line.startswith("__ERROR__"):
                    error_msg = line.split(":", 1)[1] if ":" in line else "Erreur inconnue"
                    self.log_viewer.add_line(f"Erreur: {error_msg}", "ERROR")
                else:
                    self.parse_log_line(line)
                    
        except queue.Empty:
            pass
            
        # Mettre à jour les cartes de manettes
        for card in self.get_controller_cards():
            card.update()
            
    def get_controller_cards(self):
        """Générer les cartes de manettes pour l'affichage"""
        cards = []
        x_start = 50
        y_pos = 480
        x_offset = 175
        
        for i, (controller_type, count) in enumerate(self.controllers.items()):
            x = x_start + (i % 4) * x_offset
            card = ControllerCard(x, y_pos, controller_type, count)
            cards.append(card)
            
        return cards
            
    def draw(self):
        """Dessiner l'interface"""
        # Fond
        self.screen.fill(COLORS['bg_dark'])
        
        # Titre principal
        title = FONT_TITLE.render("Synchronisation des Manettes", True, COLORS['text'])
        self.screen.blit(title, (SCREEN_WIDTH // 2 - title.get_width() // 2, 25))
        
        # Sous-titre
        subtitle = FONT_NORMAL.render("Connectez vos manettes Bluetooth en mode pairing", True, COLORS['text_secondary'])
        self.screen.blit(subtitle, (SCREEN_WIDTH // 2 - subtitle.get_width() // 2, 60))
        
        if self.state == "scanning":
            self.draw_scanning_screen()
            
        pygame.display.flip()
        
    def draw_ready_screen(self):
        """Dessiner l'écran de démarrage"""
        
        # Instructions
        y_pos = 130
        instructions = [
            ("DS4 (PlayStation 4)", "Maintenez Share + bouton PS", COLORS['ds4']),
            ("DualSense (PlayStation 5)", "Maintenez PS + Create", COLORS['dualsense']),
            ("Switch Pro Controller", "Maintenez le bouton Sync", COLORS['switch']),
            ("Joy-Con Gauche", "Maintenez le petit bouton noir", COLORS['switch']),
            ("Joy-Con Droite", "Maintenez le petit bouton noir", COLORS['switch']),
            ("Wii U Pro Controller", "Appuyez sur le bouton Sync", COLORS['wiiu']),
        ]
        
        for controller, instruction, color in instructions:
            # Icône colorée
            icon_rect = pygame.Rect(80, y_pos, 15, 15)
            pygame.draw.rect(self.screen, color, icon_rect, border_radius=4)
            
            # Nom du contrôleur
            controller_text = FONT_LARGE.render(controller, True, COLORS['text'])
            self.screen.blit(controller_text, (110, y_pos - 2))
            
            # Instruction
            instruction_text = FONT_NORMAL.render(instruction, True, COLORS['text_secondary'])
            self.screen.blit(instruction_text, (110, y_pos + 22))
            
            y_pos += 55
        
        # Ligne de séparation
        pygame.draw.line(self.screen, COLORS['border'], 
                        (50, 480), (SCREEN_WIDTH - 50, 480), 1)
        
        # Info supplémentaire
        info_text = "Les manettes seront automatiquement détectées et connectées"
        info_surface = FONT_NORMAL.render(info_text, True, COLORS['text_secondary'])
        self.screen.blit(info_surface, (SCREEN_WIDTH // 2 - info_surface.get_width() // 2, 500))
        
        # Bouton démarrer
        self.start_button.draw(self.screen)
        
    def draw_scanning_screen(self):
        """Dessiner l'écran de scan"""
        
        # Logs
        self.log_viewer.draw(self.screen)
        
        # Manettes détectées
        if self.controllers:
            detected_title = FONT_NORMAL.render("Manettes connectées:", True, COLORS['text_secondary'])
            self.screen.blit(detected_title, (50, 470))
            
            for card in self.get_controller_cards():
                card.draw(self.screen)
        else:
            waiting_text = FONT_NORMAL.render("En attente de manettes...", True, COLORS['text_secondary'])
            self.screen.blit(waiting_text, (SCREEN_WIDTH // 2 - waiting_text.get_width() // 2, 500))
        
        # Compteur
        count_text = FONT_LARGE.render(f"Total: {self.total_connected} manette(s)", True, COLORS['success'])
        self.screen.blit(count_text, (SCREEN_WIDTH // 2 - count_text.get_width() // 2, 590))
        
        # Bouton annuler
        self.cancel_button.draw(self.screen)
        
    def draw_finished_screen(self):
        """Dessiner l'écran de fin"""
        
        if self.total_connected > 0:
            # Icône de succès
            center = (SCREEN_WIDTH // 2, 150)
            pygame.draw.circle(self.screen, COLORS['success'], center, 55)
            pygame.draw.circle(self.screen, COLORS['text'], center, 55, 3)
            
            # Check mark
            check_points = [
                (center[0] - 22, center[1] + 2),
                (center[0] - 8, center[1] + 18),
                (center[0] + 24, center[1] - 14),
            ]
            pygame.draw.lines(self.screen, COLORS['bg_dark'], False, check_points, 6)
            
            # Texte de succès
            success_text = FONT_TITLE.render("Synchronisation réussie !", True, COLORS['success'])
            self.screen.blit(success_text, (SCREEN_WIDTH // 2 - success_text.get_width() // 2, 220))
            
            # Description
            desc_text = f"{self.total_connected} manette(s) connectée(s) avec succès"
            desc_surface = FONT_LARGE.render(desc_text, True, COLORS['text_secondary'])
            self.screen.blit(desc_surface, (SCREEN_WIDTH // 2 - desc_surface.get_width() // 2, 260))
            
            # Afficher les cartes des manettes
            if self.controllers:
                y_start = 320
                for i, (controller_type, count) in enumerate(self.controllers.items()):
                    x = 50 + (i % 4) * 175
                    card = ControllerCard(x, y_start, controller_type, count)
                    card.draw(self.screen)
        else:
            # Icône d'avertissement
            center = (SCREEN_WIDTH // 2, 150)
            pygame.draw.circle(self.screen, COLORS['warning'], center, 55)
            pygame.draw.circle(self.screen, COLORS['text'], center, 55, 3)
            
            # Point d'exclamation
            excl_text = FONT_HUGE.render("!", True, COLORS['bg_dark'])
            excl_rect = excl_text.get_rect(center=center)
            self.screen.blit(excl_text, excl_rect)
            
            # Texte
            warn_text = FONT_TITLE.render("Aucune manette détectée", True, COLORS['warning'])
            self.screen.blit(warn_text, (SCREEN_WIDTH // 2 - warn_text.get_width() // 2, 220))
            
            desc_text = "Vérifiez que vos manettes sont en mode pairing"
            desc_surface = FONT_NORMAL.render(desc_text, True, COLORS['text_secondary'])
            self.screen.blit(desc_surface, (SCREEN_WIDTH // 2 - desc_surface.get_width() // 2, 260))
        
        # Bouton terminer
        self.finish_button.draw(self.screen)
        
    def draw_error_screen(self):
        """Dessiner l'écran d'erreur"""
        
        # Icône d'erreur
        center = (SCREEN_WIDTH // 2, 120)
        pygame.draw.circle(self.screen, COLORS['error'], center, 50)
        pygame.draw.circle(self.screen, COLORS['text'], center, 50, 3)
        
        # Croix
        pygame.draw.line(self.screen, COLORS['bg_dark'], 
                        (center[0] - 18, center[1] - 18),
                        (center[0] + 18, center[1] + 18), 5)
        pygame.draw.line(self.screen, COLORS['bg_dark'],
                        (center[0] + 18, center[1] - 18),
                        (center[0] - 18, center[1] + 18), 5)
        
        # Texte d'erreur
        error_text = FONT_TITLE.render("Erreur de synchronisation", True, COLORS['error'])
        self.screen.blit(error_text, (SCREEN_WIDTH // 2 - error_text.get_width() // 2, 185))
        
        # Logs d'erreur
        self.log_viewer.rect.y = 230
        self.log_viewer.rect.height = 300
        self.log_viewer.draw(self.screen)
        
        # Bouton fermer
        self.error_button.draw(self.screen)
        
    def run(self):
        """Boucle principale"""
        while self.running:
            self.handle_events()
            self.update()
            self.draw()
            self.clock.tick(FPS)
            
        pygame.quit()
        sys.exit(0)

# ============================================================================
# POINT D'ENTRÉE
# ============================================================================

if __name__ == "__main__":
    app = SyncGamepadsGUI()
    app.run()
