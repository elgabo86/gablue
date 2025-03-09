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

# Variables pour éviter les répétitions rapides des commandes de volume
last_volume_time = 0
volume_cooldown = 200  # Délai en millisecondes entre chaque ajustement de volume

# Boucle principale
try:
    joystick = None
    while True:
        # Traiter tous les événements en file d'attente
        for event in pygame.event.get():
            if event.type == pygame.JOYDEVICEADDED:
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

                # Vérifier les combinaisons de boutons (actions instantanées)
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
                    if not mouse_script_running:
                        mouse_process = subprocess.Popen(
                            ["python", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/mouse.py"]
                        )
                        mouse_script_running = True
                    else:
                        print("Le script mouse.py est déjà en cours d'exécution.")
                elif home_button and l3_button:
                    print("MUTE")
                    os.system("pactl set-sink-mute @DEFAULT_SINK@ toggle")

        # Vérifier en continu l'état du joystick si connecté
        if joystick:
            # Vérifier l'état actuel des boutons et axes
            home_button = joystick.get_button(5)
            axis_1 = joystick.get_axis(1)  # -1 = haut, 1 = bas

            # Gestion du volume avec home maintenu
            current_time = pygame.time.get_ticks()
            if home_button and current_time - last_volume_time > volume_cooldown:
                if axis_1 < -0.5:  # Haut (augmenter le volume)
                    print("VOLUME UP")
                    os.system("pactl set-sink-volume @DEFAULT_SINK@ +10%")
                    last_volume_time = current_time
                elif axis_1 > 0.5:  # Bas (diminuer le volume)
                    print("VOLUME DOWN")
                    os.system("pactl set-sink-volume @DEFAULT_SINK@ -10%")
                    last_volume_time = current_time

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
