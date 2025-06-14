import pygame
import os
import subprocess
import time
from pygame.locals import *

# Initialiser Pygame
pygame.init()
pygame.joystick.init()

# Configuration de la fenêtre
width, height = 400, 300
screen = pygame.display.set_mode((width, height), pygame.NOFRAME)
pygame.display.set_caption("Choix de l'action")

# Couleurs
WHITE = (255, 255, 255)
BLACK = (0, 0, 0)
BLUE = (50, 150, 255)

# Police
font = pygame.font.SysFont("arial", 24)

# Options du menu
options = ["Déconnecter Bluetooth", "Mettre en veille", "Éteindre", "Redémarrer"]
selected_option = 0
confirm_mode = False
confirm_action = None

# Variables pour la gestion des boutons et du timer
last_up = 0
last_down = 0
last_cross = 0
last_circle = 0
cooldown = 200
last_input_time = pygame.time.get_ticks()
inactivity_timeout = 5000  # 5 secondes en millisecondes

# Boucle principale
running = True
joystick = None
clock = pygame.time.Clock()

# Attendre un court instant pour stabiliser la fenêtre
time.sleep(0.1)
print("Fenêtre initialisée")

try:
    while running:
        current_time = pygame.time.get_ticks()

        # Vérifier l'inactivité
        if current_time - last_input_time > inactivity_timeout:
            print("Inactivité détectée, fermeture du menu")
            running = False

        # Gestion des événements
        for event in pygame.event.get():
            if event.type == pygame.QUIT:
                print("Événement QUIT détecté")
                running = False
            elif event.type == pygame.JOYDEVICEADDED:
                joystick = pygame.joystick.Joystick(0)
                joystick.init()
                print("Manette connectée")
            elif event.type == pygame.JOYDEVICEREMOVED:
                if joystick:
                    joystick.quit()
                    joystick = None
                    print("Manette déconnectée")

        if joystick and current_time - last_input_time > cooldown:
            # Récupérer l'état du hat et du joystick gauche
            hat_value = joystick.get_hat(0) if joystick.get_numhats() > 0 else (0, 0)
            axis_y = joystick.get_axis(1) if joystick.get_numaxes() > 1 else 0
            up_button = 1 if hat_value == (0, 1) or axis_y < -0.5 else 0
            down_button = 1 if hat_value == (0, -1) or axis_y > 0.5 else 0
            cross_button = joystick.get_button(0) if joystick.get_numbuttons() > 0 else 0
            circle_button = joystick.get_button(1) if joystick.get_numbuttons() > 1 else 0

            if confirm_mode:
                # Mode confirmation
                if cross_button and not last_cross:
                    print(f"Confirmation de l'action: {confirm_action}")
                    pygame.display.quit()  # Fermer la fenêtre avant l'action
                    if confirm_action == "Éteindre":
                        subprocess.run(["systemctl", "poweroff"])
                    elif confirm_action == "Redémarrer":
                        subprocess.run(["systemctl", "reboot"])
                    running = False
                    last_input_time = current_time
                elif circle_button and not last_circle:
                    print("Annulation de la confirmation")
                    confirm_mode = False
                    last_input_time = current_time
            else:
                # Navigation dans le menu principal
                if up_button and not last_up:
                    selected_option = (selected_option - 1) % len(options)
                    last_input_time = current_time
                    print(f"Option sélectionnée: {options[selected_option]}")
                elif down_button and not last_down:
                    selected_option = (selected_option + 1) % len(options)
                    last_input_time = current_time
                    print(f"Option sélectionnée: {options[selected_option]}")

                # Sélection (croix)
                if cross_button and not last_cross:
                    print(f"Action choisie: {options[selected_option]}")
                    if selected_option == 0:  # Déconnecter Bluetooth
                        pygame.display.quit()  # Fermer la fenêtre avant l'action
                        subprocess.run(["/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/decoblue"])
                        running = False
                    elif selected_option == 1:  # Mettre en veille
                        pygame.display.quit()  # Fermer la fenêtre avant l'action
                        subprocess.run(["systemctl", "suspend"])
                        running = False
                    elif selected_option == 2:  # Éteindre
                        confirm_mode = True
                        confirm_action = "Éteindre"
                    elif selected_option == 3:  # Redémarrer
                        confirm_mode = True
                        confirm_action = "Redémarrer"
                    last_input_time = current_time

                # Annuler (rond)
                if circle_button and not last_circle:
                    print("Annulation")
                    running = False
                    last_input_time = current_time

            # Mettre à jour l'état précédent
            last_up = up_button
            last_down = down_button
            last_cross = cross_button
            last_circle = circle_button

        # Dessiner l'arrière-plan
        screen.fill(BLACK)

        # Afficher le menu ou la confirmation
        if confirm_mode:
            text = font.render(f"Confirmer {confirm_action} ?", True, WHITE)
            screen.blit(text, (width // 2 - text.get_width() // 2, height // 2 - 20))
            text = font.render("Croix: Oui, Rond: Non", True, WHITE)
            screen.blit(text, (width // 2 - text.get_width() // 2, height // 2 + 20))
        else:
            menu_height = len(options) * 60
            start_y = (height - menu_height) // 2 + 10  # Réduction légère de la marge
            for i, option in enumerate(options):
                color = BLUE if i == selected_option else WHITE
                text = font.render(option, True, color)
                screen.blit(text, (width // 2 - text.get_width() // 2, start_y + i * 60))

        # Rafraîchir l'écran
        pygame.display.flip()
        clock.tick(30)

except KeyboardInterrupt:
    print("Arrêt du script par interruption")
finally:
    if joystick:
        joystick.quit()
    pygame.quit()
    print("Pygame quitté")
