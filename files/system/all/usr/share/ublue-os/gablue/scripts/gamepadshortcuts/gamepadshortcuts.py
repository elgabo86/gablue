import pygame
import os
import subprocess

# Initialiser Pygame
pygame.init()

# Désactiver la prévention de mise en veille
pygame.display.set_allow_screensaver(1)

# Configurer le nombre de manettes
pygame.joystick.init()

# Enlever la sortie sonore de pygame et autres
pygame.mixer.quit()
pygame.font.quit()

# Créer une horloge
clock = pygame.time.Clock()

# Variable pour suivre l'état d'exécution de mouse.py
mouse_script_running = False
mouse_process = None

# Boucle principale
try:
    joystick = None
    while True:
        # Attendre un événement
        event = pygame.event.wait()  # Bloque jusqu'à ce qu'un événement soit disponible

        if event.type not in (pygame.JOYBUTTONDOWN, pygame.JOYDEVICEADDED, pygame.JOYDEVICEREMOVED):
            continue  # Recommencer la boucle si ce n'est pas les événements recherchés

        elif event.type == pygame.JOYDEVICEADDED:
            print("Manette connectée.")
            joystick = pygame.joystick.Joystick(0)
            joystick.init()
            # Activer la prévention de mise en veille
            pygame.display.set_allow_screensaver(0)

        elif event.type == pygame.JOYDEVICEREMOVED:
            print("Manette déconnectée.")
            if joystick:
                joystick.quit()
            # Désactiver la prévention de mise en veille
            pygame.display.set_allow_screensaver(1)

        elif event.type == pygame.JOYBUTTONDOWN and joystick:
            # Vérifier les boutons
            home_button = joystick.get_button(5)
            select_button = joystick.get_button(4)
            start_button = joystick.get_button(6)
            triangle_button = joystick.get_button(3)
            square_button = joystick.get_button(2)
            circle_button = joystick.get_button(1)
            left_button = joystick.get_button(13)
            up_button = joystick.get_button(11)
            right_button = joystick.get_button(14)
            down_button = joystick.get_button(12)
            l3_button = joystick.get_button(7)
            r3_button = joystick.get_button(8)

            # Vérifier les combinaisons de boutons
            if home_button and select_button:
                print("KILL")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/killthemall &")
            elif home_button and start_button:
                print("ES")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/openes &")
            elif home_button and down_button:
                print("MANGO")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/showhidemango &")
            elif home_button and up_button:
                print("FPS")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/changefps &")
            elif home_button and left_button:
                print("SCREEN")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/takescreenshot &")
            elif home_button and right_button:
                print("RECORD")
                os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/startstoprecord &")
                pygame.time.wait(2000)
            elif home_button and r3_button:
                print("MOUSE")
                # Vérifier si le script mouse.py est déjà en cours d'exécution
                if not mouse_script_running:
                    # Lancer le script en arrière-plan avec subprocess
                    mouse_process = subprocess.Popen(
                        ["python", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/mouse.py"]
                    )
                    mouse_script_running = True
                else:
                    print("Le script mouse.py est déjà en cours d'exécution.")

        # Vérifier si le processus mouse.py s'est terminé
        if mouse_script_running and mouse_process.poll() is not None:
            mouse_script_running = False
            mouse_process = None
            print("Le script mouse.py s'est terminé.")

        # Limiter la fréquence des événements
        clock.tick(30)

except KeyboardInterrupt:
    print("Arrêt du script.")
finally:
    if mouse_process:
        mouse_process.terminate()  # Terminer le processus si encore actif
    pygame.quit()
