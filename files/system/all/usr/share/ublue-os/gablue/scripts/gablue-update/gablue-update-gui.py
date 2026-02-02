#!/usr/bin/env python3

"""
Interface graphique moderne pour gablue-update
Utilise Pygame pour une expérience utilisateur stylée
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

SCREEN_WIDTH = 800
SCREEN_HEIGHT = 600
FPS = 60

# Couleurs - Thème sombre moderne
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
FONT_SIZE_SMALL = 14
FONT_SIZE_NORMAL = 16
FONT_SIZE_LARGE = 20
FONT_SIZE_TITLE = 28

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

class ProgressBar:
    """Barre de progression animée"""
    
    def __init__(self, x, y, width, height):
        self.rect = pygame.Rect(x, y, width, height)
        self.progress = 0
        self.target_progress = 0
        self.text = "Prêt à démarrer..."
        
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
        pygame.draw.rect(screen, COLORS['log_bg'], self.rect, border_radius=8)
        pygame.draw.rect(screen, COLORS['border'], self.rect, width=1, border_radius=8)
        
        # Titre
        title_surface = FONT_NORMAL.render("Journal de mise à jour", True, COLORS['text_secondary'])
        screen.blit(title_surface, (self.rect.x + 10, self.rect.y - 25))
        
        # Lignes de log
        y_offset = 10
        for text, color in self.lines[-15:]:  # Afficher les 15 dernières lignes
            if y_offset < self.rect.height - 20:
                # Tronquer si trop long
                max_chars = (self.rect.width - 30) // 8
                if len(text) > max_chars:
                    text = text[:max_chars-3] + "..."
                    
                line_surface = FONT_SMALL.render(text, True, color)
                screen.blit(line_surface, (self.rect.x + 10, self.rect.y + y_offset))
                y_offset += 16

# ============================================================================
# CLASSE PRINCIPALE DE L'APPLICATION
# ============================================================================

class GablueUpdateGUI:
    """Application principale"""
    
    def __init__(self):
        self.screen = pygame.display.set_mode((SCREEN_WIDTH, SCREEN_HEIGHT))
        pygame.display.set_caption("Mise à jour de Gablue")
        self.clock = pygame.time.Clock()
        self.running = True
        self.state = "ready"  # ready, updating, finished, error
        
        # Queue pour la communication avec le subprocess
        self.log_queue = queue.Queue()
        self.process = None

        # Flag pour savoir si on a déjà vérifié les mises à jour
        self.updates_checked = False

        # UI Elements
        self.setup_ui()
        
    def setup_ui(self):
        """Initialiser les éléments d'interface"""
        
        # === ÉCRAN PRÊT ===
        self.start_button = Button(
            SCREEN_WIDTH // 2 - 100, 300, 200, 50, 
            "Démarrer la mise à jour", 
            self.start_update
        )
        
        # === ÉCRAN DE MISE À JOUR ===
        self.progress_bar = ProgressBar(50, 120, SCREEN_WIDTH - 100, 50)
        self.log_viewer = LogViewer(50, 200, SCREEN_WIDTH - 100, 280)
        
        # Bouton annuler
        self.cancel_button = Button(
            SCREEN_WIDTH // 2 - 60, 500, 120, 40,
            "Annuler",
            self.cancel_update,
            color_key='error'
        )
        
        # === ÉCRAN DE FIN ===
        self.reboot_button = Button(
            SCREEN_WIDTH // 2 - 100, 320, 200, 50,
            "Redémarrer maintenant",
            self.reboot_system,
            color_key='success'
        )
        
        self.later_button = Button(
            SCREEN_WIDTH // 2 - 100, 380, 200, 50,
            "Redémarrer plus tard",
            self.quit,
            color_key='primary'
        )
        
        self.error_button = Button(
            SCREEN_WIDTH // 2 - 80, 380, 160, 50,
            "Fermer",
            self.quit,
            color_key='error'
        )
        
    def start_update(self):
        """Démarrer la mise à jour"""
        self.state = "updating"
        self.log_viewer.add_line("Démarrage de la mise à jour de Gablue", "INFO")
        self.start_bash_script()
        
    def start_bash_script(self):
        """Lancer le script bash et capturer sa sortie"""
        
        def run_script():
            script_path = Path(__file__).parent / "gablue-update-core"
            
            # Vérifier que le script existe
            if not script_path.exists():
                self.log_queue.put("__ERROR__:Script non trouvé: {script_path}")
                return
            
            # Construire les arguments
            args = [str(script_path), "--skip-reboot"]
            
            # Log de démarrage
            self.log_queue.put("[INFO] Démarrage du script de mise à jour")
            
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
        
    def cancel_update(self):
        """Annuler la mise à jour"""
        if self.process and self.process.poll() is None:
            self.process.terminate()
            self.log_viewer.add_line("Mise à jour annulée par l'utilisateur", "WARN")
            self.state = "error"
            
    def reboot_system(self):
        """Redémarrer le système"""
        self.log_viewer.add_line("Redémarrage du système...", "INFO")
        try:
            subprocess.Popen(["systemctl", "reboot"])
        except:
            pass
        self.quit()
        
    def quit(self):
        """Quitter l'application"""
        if self.process and self.process.poll() is None:
            self.process.terminate()
        self.running = False
        
    def parse_log_line(self, line):
        """Parser une ligne de log et mettre à jour l'interface"""
        
        # Détection des messages de progression
        progress_match = re.search(r'Progress:\s*(\d+)%', line, re.IGNORECASE)
        if progress_match:
            try:
                progress = int(progress_match.group(1))
                # Extraire le message après le pourcentage
                message = line.split('-', 1)[1].strip() if '-' in line else "Mise à jour en cours..."
                self.progress_bar.set_progress(progress, message)
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
                    if self.state in ["finished", "error"]:
                        self.quit()
                        
            # Écran prêt
            if self.state == "ready":
                self.start_button.handle_event(event)
                
            # Écran de mise à jour
            elif self.state == "updating":
                self.cancel_button.handle_event(event)
                
            # Écran de fin
            elif self.state == "finished":
                self.reboot_button.handle_event(event)
                self.later_button.handle_event(event)
                
            # Écran d'erreur
            elif self.state == "error":
                self.error_button.handle_event(event)

            # Écran "déjà à jour"
            elif self.state == "no-updates":
                self.later_button.handle_event(event)
                    
    def update(self):
        """Mettre à jour la logique de l'application"""
        
        # Traiter les messages de la queue
        try:
            while True:
                line = self.log_queue.get_nowait()

                if line == "__SUCCESS__":
                    # Ne pas changer l'état si on est déjà dans un état final (no-updates, finished, error)
                    if self.state not in ["no-updates", "finished", "error"]:
                        self.state = "finished"
                        self.progress_bar.set_progress(100, "Mise à jour terminée avec succès !")
                        self.log_viewer.add_line("Mise à jour terminée avec succès !", "SUCCESS")
                        # Remettre le texte original du bouton
                        self.later_button.text = "Redémarrer plus tard"
                elif line == "__NO_UPDATES__":
                    self.state = "no-updates"
                    self.progress_bar.set_progress(100, "Système déjà à jour")
                    self.log_viewer.add_line("Système déjà à jour - Aucune mise à jour nécessaire", "SUCCESS")
                    # Changer le texte du bouton pour "Fermer" car pas besoin de redémarrer
                    self.later_button.text = "Fermer"
                elif line.startswith("__ERROR__"):
                    self.state = "error"
                    error_msg = line.split(":", 1)[1] if ":" in line else "Erreur inconnue"
                    self.log_viewer.add_line(f"Erreur: {error_msg}", "ERROR")
                else:
                    self.parse_log_line(line)

        except queue.Empty:
            pass
            
        # Mettre à jour la barre de progression
        if self.state == "updating":
            self.progress_bar.update()
            
    def draw(self):
        """Dessiner l'interface"""
        # Fond
        self.screen.fill(COLORS['bg_dark'])
        
        # Titre principal
        title = FONT_TITLE.render("Mise à jour de Gablue", True, COLORS['text'])
        self.screen.blit(title, (SCREEN_WIDTH // 2 - title.get_width() // 2, 30))
        
        # Sous-titre
        subtitle = FONT_NORMAL.render("Met à jour le système et les applications Flatpak", True, COLORS['text_secondary'])
        self.screen.blit(subtitle, (SCREEN_WIDTH // 2 - subtitle.get_width() // 2, 65))
        
        if self.state == "ready":
            self.draw_ready_screen()
        elif self.state == "updating":
            self.draw_update_screen()
        elif self.state == "finished":
            self.draw_finish_screen()
        elif self.state == "error":
            self.draw_error_screen()
        elif self.state == "no-updates":
            self.draw_no_updates_screen()
            
        pygame.display.flip()
        
    def draw_ready_screen(self):
        """Dessiner l'écran de démarrage"""
        
        # Icône/info
        info_text = "Cette opération va :"
        info_surface = FONT_LARGE.render(info_text, True, COLORS['text'])
        self.screen.blit(info_surface, (SCREEN_WIDTH // 2 - info_surface.get_width() // 2, 150))
        
        # Liste des étapes
        steps = [
            "• Annuler les mises à jour en attente",
            "• Mettre à jour le système (rpm-ostree)",
            "• Mettre à jour les applications Flatpak",
        ]
        
        y_pos = 190
        for step in steps:
            step_surface = FONT_NORMAL.render(step, True, COLORS['text_secondary'])
            self.screen.blit(step_surface, (SCREEN_WIDTH // 2 - step_surface.get_width() // 2, y_pos))
            y_pos += 30
        
        # Bouton démarrer
        self.start_button.draw(self.screen)
        
        # Info
        info_text2 = "Un redémarrage sera nécessaire après la mise à jour"
        info_surface2 = FONT_SMALL.render(info_text2, True, COLORS['text_secondary'])
        self.screen.blit(info_surface2, (SCREEN_WIDTH // 2 - info_surface2.get_width() // 2, 370))
        
    def draw_update_screen(self):
        """Dessiner l'écran de mise à jour"""
        
        # Barre de progression
        self.progress_bar.draw(self.screen)
        
        # Logs
        self.log_viewer.draw(self.screen)
        
        # Bouton annuler
        self.cancel_button.draw(self.screen)
        
    def draw_finish_screen(self):
        """Dessiner l'écran de fin"""
        
        # Icône de succès (cercle vert avec check)
        center = (SCREEN_WIDTH // 2, 160)
        pygame.draw.circle(self.screen, COLORS['success'], center, 50)
        pygame.draw.circle(self.screen, COLORS['text'], center, 50, 3)
        
        # Check mark
        check_points = [
            (center[0] - 20, center[1]),
            (center[0] - 8, center[1] + 12),
            (center[0] + 20, center[1] - 16),
        ]
        pygame.draw.lines(self.screen, COLORS['bg_dark'], False, check_points, 5)
        
        # Texte de succès
        success_text = FONT_TITLE.render("Mise à jour réussie !", True, COLORS['success'])
        self.screen.blit(success_text, (SCREEN_WIDTH // 2 - success_text.get_width() // 2, 230))
        
        # Description
        desc_text = "Gablue est à jour. Un redémarrage est nécessaire."
        desc_surface = FONT_NORMAL.render(desc_text, True, COLORS['text_secondary'])
        self.screen.blit(desc_surface, (SCREEN_WIDTH // 2 - desc_surface.get_width() // 2, 270))
        
        # Boutons
        self.reboot_button.draw(self.screen)
        self.later_button.draw(self.screen)
        
    def draw_error_screen(self):
        """Dessiner l'écran d'erreur avec logs visibles"""

        # Icône d'erreur (croix rouge)
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
        error_text = FONT_LARGE.render("Mise à jour échouée", True, COLORS['error'])
        self.screen.blit(error_text, (SCREEN_WIDTH // 2 - error_text.get_width() // 2, 170))

        # Logs (affichés pour voir ce qui s'est passé)
        self.log_viewer.rect.y = 210
        self.log_viewer.rect.height = 150
        self.log_viewer.draw(self.screen)

        # Bouton fermer
        self.error_button.draw(self.screen)

    def draw_no_updates_screen(self):
        """Dessiner l'écran quand le système est déjà à jour"""

        # Icône de succès (cercle vert avec check)
        center = (SCREEN_WIDTH // 2, 160)
        pygame.draw.circle(self.screen, COLORS['success'], center, 50)
        pygame.draw.circle(self.screen, COLORS['text'], center, 50, 3)

        # Check mark
        check_points = [
            (center[0] - 20, center[1]),
            (center[0] - 8, center[1] + 12),
            (center[0] + 20, center[1] - 16),
        ]
        pygame.draw.lines(self.screen, COLORS['bg_dark'], False, check_points, 5)

        # Texte principal
        success_text = FONT_TITLE.render("Système déjà à jour !", True, COLORS['success'])
        self.screen.blit(success_text, (SCREEN_WIDTH // 2 - success_text.get_width() // 2, 230))

        # Description
        desc_text = "Aucune mise à jour nécessaire. Gablue est à jour."
        desc_surface = FONT_NORMAL.render(desc_text, True, COLORS['text_secondary'])
        self.screen.blit(desc_surface, (SCREEN_WIDTH // 2 - desc_surface.get_width() // 2, 270))

        # Version actuelle si disponible
        version_text = FONT_SMALL.render("Votre système est à jour", True, COLORS['text_secondary'])
        self.screen.blit(version_text, (SCREEN_WIDTH // 2 - version_text.get_width() // 2, 300))

        # Bouton fermer
        self.later_button.draw(self.screen)

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
    app = GablueUpdateGUI()
    app.run()
