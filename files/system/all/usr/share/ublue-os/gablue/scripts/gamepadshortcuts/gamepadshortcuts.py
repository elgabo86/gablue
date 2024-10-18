import pygame
import time
import os

# Initialiser Pygame
pygame.init()

# Configurer le nombre de manettes
pygame.joystick.init()

# Fonction pour exécuter une commande spécifique
def execute_command(command):
    print(f"Exécution de la commande: {command}")

# Boucle principale
try:
    while True:
        # Vérifier les événements
        for event in pygame.event.get():
            if event.type == pygame.JOYDEVICEADDED:
                print("Manette connectée.")
            elif event.type == pygame.JOYDEVICEREMOVED:
                print("Manette déconnectée.")

        # Vérifier si une manette est connectée
        if pygame.joystick.get_count() > 0:
            joystick = pygame.joystick.Joystick(0)
            joystick.init()

            # Vérifier les boutons
            home_button = joystick.get_button(5) # Remplacez par l'index correct pour le bouton Home
            select_button = joystick.get_button(4) # Remplacez par l'index correct pour le bouton Select
            start_button = joystick.get_button(6) # Remplacez par l'index correct pour le bouton Start
            triangle_button= joystick.get_button(3)
            square_button= joystick.get_button(2)
            circle_button= joystick.get_button(1)
            left_button= joystick.get_button(13)
            right_button= joystick.get_button(14)

            # Vérifier les combinaisons de boutons
            if home_button and select_button:
                execute_command("Commande pour Home + Select")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/killthemall &")
                time.sleep(1) # Anti-rebond
            elif home_button and start_button:
                execute_command("Commande pour Home + Start")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/openes &")
                time.sleep(0.5) # Anti-rebond
            elif home_button and triangle_button:
                execute_command("Commande pour Home + Triangle")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/showhidemango &")
                time.sleep(0.5) # Anti-rebond
            elif home_button and left_button:
                execute_command("Commande pour Home + Left")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/takescreenshot &")
                time.sleep(0.5) # Anti-rebond
            elif home_button and right_button:
                execute_command("Commande pour Home + Right")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/startstoprecord &")
                time.sleep(1) # Anti-rebond

        # Limiter l'utilisation du CPU
        time.sleep(0.01)

except KeyboardInterrupt:
    print("Arrêt du script.")
finally:
    pygame.quit()
