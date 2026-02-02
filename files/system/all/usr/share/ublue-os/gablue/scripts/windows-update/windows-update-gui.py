#!/usr/bin/env python3

"""
Interface graphique moderne pour windows-update
Utilise Pygame pour une expérience utilisateur stylée et gaming
"""

import os
os.environ['PYGAME_HIDE_SUPPORT_PROMPT'] = '1'

import pygame
import sys
import subprocess
import threading
import queue
import os
import re
from pathlib import Path

# Initialisation de Pygame
pygame.init()
pygame.font.init()

# ============================================================================
# CONSTANTES ET CONFIGURATION
# ============================================================================

SCREEN_WIDTH = 900
SCREEN_HEIGHT = 650
FPS = 60

# Couleurs - Thème sombre gaming
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
}

# Fonts
FONT_SIZE_SMALL = 16
FONT_SIZE_NORMAL = 18
FONT_SIZE_LARGE = 22
FONT_SIZE_TITLE = 32

try:
    FONT_SMALL = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_SMALL)
    FONT_NORMAL = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_NORMAL)
    FONT_LARGE = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_LARGE)
    FONT_TITLE = pygame.font.SysFont("JetBrains Mono", FONT_SIZE_TITLE, bold=True)
except:
    FONT_SMALL = pygame.font.SysFont("monospace", FONT_SIZE_SMALL)
    FONT_NORMAL = pygame.font.SysFont("monospace", FONT_SIZE_NORMAL)
    FONT_LARGE = pygame.font.SysFont("monospace", FONT_SIZE_LARGE)
    FONT_TITLE = pygame.font.SysFont("monospace", FONT_SIZE_TITLE, bold=True)

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
        pygame.draw.rect(screen, color, self.rect, border_radius=8)
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=1, border_radius=8)
        
        # Texte
        text_color = COLORS['text'] if self.enabled else COLORS['text_secondary']
        if self.clicked:
            text_color = COLORS['bg_dark']
        text_surface = FONT_NORMAL.render(self.text, True, text_color)
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

class Checkbox:
    """Case à cocher moderne"""
    
    def __init__(self, x, y, width, height, text, checked=False):
        self.rect = pygame.Rect(x, y, width, height)
        self.text = text
        self.checked = checked
        self.hovered = False
        self.box_size = 20
        
    def draw(self, screen):
        # Zone de texte
        text_surface = FONT_NORMAL.render(self.text, True, COLORS['text'])
        screen.blit(text_surface, (self.rect.x + self.box_size + 10, self.rect.y + 2))
        
        # Case à cocher
        box_rect = pygame.Rect(self.rect.x, self.rect.y, self.box_size, self.box_size)
        
        if self.checked:
            pygame.draw.rect(screen, COLORS['primary'], box_rect, border_radius=4)
            # Coche blanche
            check_points = [
                (box_rect.x + 4, box_rect.centery),
                (box_rect.x + 8, box_rect.y + 14),
                (box_rect.x + 16, box_rect.y + 5),
            ]
            pygame.draw.lines(screen, COLORS['text'], False, check_points, 2)
        else:
            pygame.draw.rect(screen, COLORS['bg_card'], box_rect, border_radius=4)
            pygame.draw.rect(screen, COLORS['border'], box_rect, width=2, border_radius=4)
            
        if self.hovered and not self.checked:
            pygame.draw.rect(screen, COLORS['primary'], box_rect, width=2, border_radius=4)
            
    def handle_event(self, event):
        if event.type == pygame.MOUSEMOTION:
            self.hovered = self.rect.collidepoint(event.pos)
            
        elif event.type == pygame.MOUSEBUTTONDOWN:
            if event.button == 1 and self.hovered:
                self.checked = not self.checked
                return True
                
        return False

class Card:
    """Carte sélectionnable style moderne"""
    
    def __init__(self, x, y, width, height, title, description, value, group=None):
        self.rect = pygame.Rect(x, y, width, height)
        self.title = title
        self.description = description
        self.value = value
        self.group = group
        self.selected = False
        self.hovered = False
        
    def draw(self, screen):
        # Couleur de fond selon l'état
        if self.selected:
            color = COLORS['primary']
            border_color = COLORS['primary_light']
        elif self.hovered:
            color = COLORS['bg_hover']
            border_color = COLORS['primary']
        else:
            color = COLORS['bg_card']
            border_color = COLORS['border']
            
        pygame.draw.rect(screen, color, self.rect, border_radius=12)
        pygame.draw.rect(screen, border_color, self.rect, width=2, border_radius=12)
        
        # Indicateur de sélection
        if self.selected:
            indicator_rect = pygame.Rect(self.rect.x + 15, self.rect.centery - 6, 12, 12)
            pygame.draw.circle(screen, COLORS['text'], indicator_rect.center, 6)
        else:
            indicator_rect = pygame.Rect(self.rect.x + 15, self.rect.centery - 6, 12, 12)
            pygame.draw.circle(screen, COLORS['border'], indicator_rect.center, 6, 2)
        
        # Titre
        text_color = COLORS['bg_dark'] if self.selected else COLORS['text']
        title_surface = FONT_LARGE.render(self.title, True, text_color)
        screen.blit(title_surface, (self.rect.x + 40, self.rect.y + 15))
        
        # Description
        desc_color = COLORS['bg_dark'] if self.selected else COLORS['text_secondary']
        desc_surface = FONT_SMALL.render(self.description, True, desc_color)
        screen.blit(desc_surface, (self.rect.x + 40, self.rect.y + 40))
        
    def handle_event(self, event):
        if event.type == pygame.MOUSEMOTION:
            self.hovered = self.rect.collidepoint(event.pos)
            
        elif event.type == pygame.MOUSEBUTTONDOWN:
            if event.button == 1 and self.hovered:
                self.selected = True
                # Désélectionner les autres cartes du même groupe
                if self.group:
                    for card in self.group:
                        if card != self:
                            card.selected = False
                return True
                
        return False

class ProgressBar:
    """Barre de progression animée"""
    
    def __init__(self, x, y, width, height):
        self.rect = pygame.Rect(x, y, width, height)
        self.progress = 0
        self.target_progress = 0
        self.text = "Initialisation..."
        
    def set_progress(self, value, text=""):
        self.target_progress = max(0, min(100, value))
        if text:
            self.text = text
            
    def update(self):
        # Animation fluide
        diff = self.target_progress - self.progress
        self.progress += diff * 0.1
        
    def draw(self, screen):
        # Fond
        pygame.draw.rect(screen, COLORS['progress_bg'], self.rect, border_radius=8)
        
        # Barre de progression avec gradient
        if self.progress > 0:
            fill_width = int((self.progress / 100) * (self.rect.width - 4))
            fill_rect = pygame.Rect(
                self.rect.x + 2, 
                self.rect.y + 2, 
                fill_width, 
                self.rect.height - 4
            )
            
            # Gradient
            for i in range(fill_width):
                ratio = i / fill_width if fill_width > 0 else 0
                r = int(COLORS['primary'][0] * (1 - ratio * 0.3))
                g = int(COLORS['primary'][1] + ratio * 20)
                b = int(COLORS['primary'][2])
                pygame.draw.line(screen, (r, g, b), 
                               (fill_rect.x + i, fill_rect.y),
                               (fill_rect.x + i, fill_rect.y + fill_rect.height))
            
        # Bordure
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=1, border_radius=8)
        
        # Pourcentage
        percent_text = f"{int(self.progress)}%"
        percent_surface = FONT_LARGE.render(percent_text, True, COLORS['text'])
        percent_rect = percent_surface.get_rect(midright=(self.rect.right - 15, self.rect.centery))
        screen.blit(percent_surface, percent_rect)
        
        # Texte de statut
        status_surface = FONT_NORMAL.render(self.text, True, COLORS['text_secondary'])
        status_rect = status_surface.get_rect(midleft=(self.rect.x + 15, self.rect.centery))
        screen.blit(status_surface, status_rect)

class LogViewer:
    """Visionneur de logs en temps réel"""
    
    def __init__(self, x, y, width, height):
        self.rect = pygame.Rect(x, y, width, height)
        self.lines = []
        self.max_lines = 50
        self.scroll_offset = 0
        self.surface = pygame.Surface((width - 20, height - 20))
        
    def add_line(self, text, level="INFO"):
        timestamp = pygame.time.get_ticks() // 1000
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
        pygame.draw.rect(screen, COLORS['log_bg'], self.rect, border_radius=8)
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=1, border_radius=8)
        
        # Titre
        title_surface = FONT_NORMAL.render("Logs", True, COLORS['text_secondary'])
        screen.blit(title_surface, (self.rect.x + 10, self.rect.y - 25))
        
        # Lignes de log
        y_offset = 10
        for text, color in self.lines[-20:]:  # Afficher les 20 dernières lignes
            if y_offset < self.rect.height - 20:
                # Tronquer si trop long
                max_chars = (self.rect.width - 30) // 9
                if len(text) > max_chars:
                    text = text[:max_chars-3] + "..."
                    
                line_surface = FONT_SMALL.render(text, True, color)
                screen.blit(line_surface, (self.rect.x + 10, self.rect.y + y_offset))
                y_offset += 18

# ============================================================================
# CLASSE PRINCIPALE DE L'APPLICATION
# ============================================================================

class WindowsUpdateGUI:
    """Application principale"""
    
    def __init__(self):
        self.screen = pygame.display.set_mode((SCREEN_WIDTH, SCREEN_HEIGHT))
        pygame.display.set_caption("Windows Update - Gablue")
        self.clock = pygame.time.Clock()
        self.running = True
        self.state = "selection"  # selection, downloading, finished, error
        
        # Queue pour la communication avec le subprocess
        self.log_queue = queue.Queue()
        self.process = None
        
        # Sélections
        self.runner_choice = None
        self.dxvk_choice = None
        
        # UI Elements
        self.setup_ui()
        
    def setup_ui(self):
        """Initialiser les éléments d'interface"""
        
        # === ÉCRAN DE SÉLECTION ===
        self.runner_cards = []
        runners = [
            ("Gwine-Proton", "Version optimisée pour le gaming (recommandé)", "gwine-proton"),
            ("Gwine", "Version standard", "gwine"),
            ("Gwine-Proton-WoW64", "Version WoW64 expérimentale", "gwine-proton-wow64"),
            ("Soda", "Version stable legacy", "soda"),
        ]
        
        y_pos = 140
        for title, desc, value in runners:
            card = Card(50, y_pos, 400, 70, title, desc, value, self.runner_cards)
            self.runner_cards.append(card)
            y_pos += 85
            
        # Sélection par défaut
        self.runner_cards[0].selected = True
        
        self.dxvk_cards = []
        dxvk_options = [
            ("DXVK GPLAsync", "Recommandé - Moins de sacades, shaders en arrière-plan", "gplasync"),
            ("DXVK Standard", "Moins d'artefacts visuels", "standard"),
        ]
        
        y_pos = 140
        for title, desc, value in dxvk_options:
            card = Card(480, y_pos, 370, 70, title, desc, value, self.dxvk_cards)
            self.dxvk_cards.append(card)
            y_pos += 85
            
        # Sélection par défaut
        self.dxvk_cards[0].selected = True
        
        # Checkbox rebuild forcé
        self.force_rebuild_checkbox = Checkbox(
            480, 325, 370, 30,
            "Forcer le rebuild complet (même si à jour)",
            checked=False
        )
        
        # Bouton démarrer
        self.start_button = Button(
            SCREEN_WIDTH // 2 - 100, 520, 200, 50, 
            "Démarrer l'installation", 
            self.start_installation
        )
        
        # === ÉCRAN DE TÉLÉCHARGEMENT ===
        self.progress_bar = ProgressBar(50, 80, SCREEN_WIDTH - 100, 50)
        self.log_viewer = LogViewer(50, 160, SCREEN_WIDTH - 100, 400)
        
        # Bouton annuler
        self.cancel_button = Button(
            SCREEN_WIDTH // 2 - 60, 580, 120, 40,
            "Annuler",
            self.cancel_installation,
            color_key='error'
        )
        
        # === ÉCRAN DE FIN ===
        self.finish_button = Button(
            SCREEN_WIDTH // 2 - 80, 400, 160, 50,
            "Terminer",
            self.quit,
            color_key='success'
        )
        
        self.error_button = Button(
            SCREEN_WIDTH // 2 - 80, 400, 160, 50,
            "Fermer",
            self.quit,
            color_key='error'
        )
        
    def start_installation(self):
        """Démarrer l'installation"""
        # Récupérer les sélections
        for card in self.runner_cards:
            if card.selected:
                self.runner_choice = card.value
                break
                
        for card in self.dxvk_cards:
            if card.selected:
                self.dxvk_choice = card.value
                break
                
        if not self.runner_choice or not self.dxvk_choice:
            return
            
        self.state = "downloading"
        self.log_viewer.add_line(f"Installation démarrée - Runner: {self.runner_choice}, DXVK: {self.dxvk_choice}", "INFO")
        
        # Lancer le script bash en arrière-plan
        self.start_bash_script()
        
    def start_bash_script(self):
        """Lancer le script bash et capturer sa sortie"""
        
        def run_script():
            script_path = Path(__file__).parent / "windows-update-core"
            
            # Vérifier que le script existe
            if not script_path.exists():
                self.log_queue.put(f"__ERROR__:Script non trouvé: {script_path}")
                return
            
            # Construire les arguments
            args = [str(script_path), "--gui"]
            
            # Ajouter rebuild forcé si checkbox cochée
            if self.force_rebuild_checkbox.checked:
                args.append("--rebuild")
                self.log_viewer.add_line("Rebuild forcé activé", "WARN")
            
            if self.dxvk_choice == "standard":
                args.append("--dxvk-no-async")
                
            # Variable d'environnement pour le runner
            env = os.environ.copy()
            if self.runner_choice:
                env["RUNNER_CHOICE"] = str(self.runner_choice)
            
            # Log de démarrage
            self.log_queue.put(f"[INFO] Démarrage du script: {' '.join(args)}")
            
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
        
    def cancel_installation(self):
        """Annuler l'installation"""
        if self.process and self.process.poll() is None:
            self.process.terminate()
            self.log_viewer.add_line("Installation annulée par l'utilisateur", "WARN")
            self.state = "error"
            
    def quit(self):
        """Quitter l'application"""
        if self.process and self.process.poll() is None:
            self.process.terminate()
        self.running = False
        
    def parse_log_line(self, line):
        """Parser une ligne de log et mettre à jour l'interface"""
        
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
                    self.progress_bar.set_progress(progress, line)
                    return
                except:
                    pass
                    
        # Détection du niveau de log
        level = "INFO"
        if "[ERROR]" in line or "error" in line.lower():
            level = "ERROR"
        elif "[WARN]" in line or "warn" in line.lower():
            level = "WARN"
        elif "succès" in line.lower() or "réussie" in line.lower():
            level = "SUCCESS"
            
        # Ajouter à la vue des logs
        self.log_viewer.add_line(line, level)
        
    def handle_events(self):
        """Gérer les événements Pygame"""
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                self.quit()
                return
                
            if event.type == pygame.KEYDOWN:
                if event.key == pygame.K_ESCAPE:
                    if self.state == "selection":
                        self.quit()
                    elif self.state in ["finished", "error"]:
                        self.quit()
                        
            # Écran de sélection
            if self.state == "selection":
                for card in self.runner_cards:
                    card.handle_event(event)
                for card in self.dxvk_cards:
                    card.handle_event(event)
                self.force_rebuild_checkbox.handle_event(event)
                self.start_button.handle_event(event)
                
            # Écran de téléchargement
            elif self.state == "downloading":
                self.cancel_button.handle_event(event)
                
            # Écran de fin
            elif self.state in ["finished", "error"]:
                if self.state == "finished":
                    self.finish_button.handle_event(event)
                else:
                    self.error_button.handle_event(event)
                    
    def update(self):
        """Mettre à jour la logique de l'application"""
        
        # Traiter les messages de la queue
        try:
            while True:
                line = self.log_queue.get_nowait()
                
                if line == "__SUCCESS__":
                    self.state = "finished"
                    self.progress_bar.set_progress(100, "Installation terminée avec succès !")
                elif line.startswith("__ERROR__"):
                    self.state = "error"
                    error_msg = line.split(":", 1)[1] if ":" in line else "Erreur inconnue"
                    self.log_viewer.add_line(f"Erreur: {error_msg}", "ERROR")
                else:
                    self.parse_log_line(line)
                    
        except queue.Empty:
            pass
            
        # Mettre à jour la barre de progression
        if self.state == "downloading":
            self.progress_bar.update()
            
    def draw(self):
        """Dessiner l'interface"""
        # Fond
        self.screen.fill(COLORS['bg_dark'])
        
        # Titre principal
        title = FONT_TITLE.render("Windows Update", True, COLORS['text'])
        self.screen.blit(title, (SCREEN_WIDTH // 2 - title.get_width() // 2, 30))
        
        # Sous-titre
        subtitle = FONT_NORMAL.render("Configuration de l'environnement Windows pour Gablue", True, COLORS['text_secondary'])
        self.screen.blit(subtitle, (SCREEN_WIDTH // 2 - subtitle.get_width() // 2, 65))
        
        if self.state == "selection":
            self.draw_selection_screen()
        elif self.state == "downloading":
            self.draw_download_screen()
        elif self.state == "finished":
            self.draw_finish_screen()
        elif self.state == "error":
            self.draw_error_screen()
            
        pygame.display.flip()
        
    def draw_selection_screen(self):
        """Dessiner l'écran de sélection"""
        
        # Section Wine
        wine_title = FONT_LARGE.render("Version de Wine", True, COLORS['primary'])
        self.screen.blit(wine_title, (50, 110))
        
        for card in self.runner_cards:
            card.draw(self.screen)
            
        # Section DXVK
        dxvk_title = FONT_LARGE.render("Type de DXVK", True, COLORS['primary'])
        self.screen.blit(dxvk_title, (480, 110))
        
        for card in self.dxvk_cards:
            card.draw(self.screen)
        
        # Checkbox rebuild forcé
        self.force_rebuild_checkbox.draw(self.screen)
        
        # Ligne de séparation
        pygame.draw.line(self.screen, COLORS['border'], 
                        (50, 500), (SCREEN_WIDTH - 50, 500), 1)
            
        # Bouton démarrer
        self.start_button.draw(self.screen)
        
        # Info bulle
        info_text = "ℹ️  Les paramètres recommandés sont présélectionnés"
        info_surface = FONT_SMALL.render(info_text, True, COLORS['text_secondary'])
        self.screen.blit(info_surface, (SCREEN_WIDTH // 2 - info_surface.get_width() // 2, 585))
        
    def draw_download_screen(self):
        """Dessiner l'écran de téléchargement"""
        
        # Barre de progression
        self.progress_bar.draw(self.screen)
        
        # Logs
        self.log_viewer.draw(self.screen)
        
        # Bouton annuler
        self.cancel_button.draw(self.screen)
        
    def draw_finish_screen(self):
        """Dessiner l'écran de fin"""
        
        # Icône de succès (cercle vert avec check)
        center = (SCREEN_WIDTH // 2, 250)
        pygame.draw.circle(self.screen, COLORS['success'], center, 60)
        pygame.draw.circle(self.screen, COLORS['text'], center, 60, 3)
        
        # Check mark
        check_points = [
            (center[0] - 25, center[1]),
            (center[0] - 10, center[1] + 15),
            (center[0] + 25, center[1] - 20),
        ]
        pygame.draw.lines(self.screen, COLORS['bg_dark'], False, check_points, 6)
        
        # Texte de succès
        success_text = FONT_TITLE.render("Installation réussie !", True, COLORS['success'])
        self.screen.blit(success_text, (SCREEN_WIDTH // 2 - success_text.get_width() // 2, 330))
        
        # Description
        desc_text = "L'environnement Windows est prêt à être utilisé."
        desc_surface = FONT_NORMAL.render(desc_text, True, COLORS['text_secondary'])
        self.screen.blit(desc_surface, (SCREEN_WIDTH // 2 - desc_surface.get_width() // 2, 370))
        
        # Bouton terminer
        self.finish_button.draw(self.screen)
        
    def draw_error_screen(self):
        """Dessiner l'écran d'erreur avec logs visibles"""
        
        # Icône d'erreur (croix rouge) - position réduite
        center = (SCREEN_WIDTH // 2, 120)
        pygame.draw.circle(self.screen, COLORS['error'], center, 40)
        pygame.draw.circle(self.screen, COLORS['text'], center, 40, 2)
        
        # Croix
        pygame.draw.line(self.screen, COLORS['bg_dark'], 
                        (center[0] - 15, center[1] - 15),
                        (center[0] + 15, center[1] + 15), 4)
        pygame.draw.line(self.screen, COLORS['bg_dark'],
                        (center[0] + 15, center[1] - 15),
                        (center[0] - 15, center[1] + 15), 4)
        
        # Texte d'erreur
        error_text = FONT_LARGE.render("Installation échouée", True, COLORS['error'])
        self.screen.blit(error_text, (SCREEN_WIDTH // 2 - error_text.get_width() // 2, 170))
        
        # Logs (affichés pour voir ce qui s'est passé)
        self.log_viewer.rect.y = 210
        self.log_viewer.rect.height = 350
        self.log_viewer.draw(self.screen)
        
        # Bouton fermer - repositionné
        self.error_button.rect.y = 580
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
    app = WindowsUpdateGUI()
    app.run()
