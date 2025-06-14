# Version 1.4
import pygame
import os
import subprocess
import time

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

# Variable pour suivre l'état d'exécution de mouse.py et menuvsr.py
mouse_script_running = False
menuvsr_script_running = False
mouse_process = None
menuvsr_process = None

# Variables pour éviter les répétitions rapides des commandes de volume
last_volume_time = 0
volume_cooldown = 200  # Délai en millisecondes entre chaque ajustement de volume

# Variables pour suivre l'état précédent des boutons du hat
last_left_button = 0
last_right_button = 0
last_up_button = 0
last_down_button = 0

# Variables pour gérer la pression prolongée du bouton home
home_button_pressed = False
home_press_start_time = 0
home_press_duration = 1500  # 1.5 seconde en millisecondes

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
                num_buttons = joystick.get_numbuttons()
                print(f"Nombre de boutons détectés : {num_buttons}")
                pygame.display.set_allow_screensaver(0)

            elif event.type == pygame.JOYDEVICEREMOVED:
                print("Manette déconnectée.")
                if joystick:
                    joystick.quit()
                # Désactiver la prévention de mise en veille
                pygame.display.set_allow_screensaver(1)

            elif event.type == pygame.JOYBUTTONDOWN and joystick:
                num_buttons = joystick.get_numbuttons()
                home_button = joystick.get_button(5) if num_buttons > 5 else 0
                select_button = joystick.get_button(4) if num_buttons > 4 else 0
                start_button = joystick.get_button(6) if num_buttons > 6 else 0
                triangle_button = joystick.get_button(3) if num_buttons > 3 else 0
                square_button = joystick.get_button(2) if num_buttons > 2 else 0
                circle_button = joystick.get_button(1) if num_buttons > 1 else 0
                l3_button = joystick.get_button(7) if num_buttons > 7 else 0
                r3_button = joystick.get_button(8) if num_buttons > 8 else 0

                # Vérifier si le bouton home est pressé
                if home_button and not home_button_pressed:
                    home_button_pressed = True
                    home_press_start_time = pygame.time.get_ticks()

                # Vérifier les combinaisons de boutons (actions instantanées)
                if home_button and select_button:
                    print("KILL")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/killthemall &")
                elif home_button and start_button:
                    print("ES")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/openes &")
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
                elif home_button and triangle_button:
                    print("LAUNCHYT")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/launchyt &")
                    pygame.time.wait(1000)

            elif event.type == pygame.JOYBUTTONUP and joystick:
                num_buttons = joystick.get_numbuttons()
                home_button = joystick.get_button(5) if num_buttons > 5 else 0
                if not home_button and home_button_pressed:
                    home_button_pressed = False
                    home_press_start_time = 0  # Réinitialiser le compteur

        # Vérifier en continu l'état du joystick si connecté
        if joystick:
            # Vérifier l'état actuel des boutons, axes et hat
            num_buttons = joystick.get_numbuttons()
            home_button = joystick.get_button(5) if num_buttons > 5 else 0
            axis_1 = joystick.get_axis(1)  # -1 = haut, 1 = bas
            hat_value = joystick.get_hat(0) if joystick.get_numhats() > 0 else (0, 0)
            left_button = 1 if hat_value == (-1, 0) else 0
            up_button = 1 if hat_value == (0, 1) else 0
            right_button = 1 if hat_value == (1, 0) else 0
            down_button = 1 if hat_value == (0, -1) else 0

            # Gestion de la pression prolongée du bouton home
            if home_button and home_button_pressed and not menuvsr_script_running:
                if pygame.time.get_ticks() - home_press_start_time >= home_press_duration:
                    print("MENUVSR")
                    menuvsr_process = subprocess.Popen(
                        ["python", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/menuvsr.py"]
                    )
                    menuvsr_script_running = True
                    menuvsr_process.wait()  # Attendre la fin de l'exécution de menuvsr.py
                    menuvsr_script_running = False
                    menuvsr_process = None
                    home_button_pressed = False  # Réinitialiser après exécution
                    home_press_start_time = 0  # Réinitialiser le compteur
                    print("Le script menuvsr.py s'est terminé.")

            # Gestion des combinaisons avec le hat
            if home_button:
                if left_button and not last_left_button:
                    print("SCREEN")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/takescreenshot &")
                    pygame.time.wait(100)
                elif right_button and not last_right_button:
                    print("RECORD")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/startstoprecord &")
                    pygame.time.wait(2000)
                elif up_button and not last_up_button:
                    print("FPS")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/changefps")
                    pygame.time.wait(100)
                elif down_button and not last_down_button:
                    print("MANGO")
                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/showhidemango")
                    pygame.time.wait(100)

            # Mettre à jour l'état précédent des boutons du hat
            last_left_button = left_button
            last_right_button = right_button
            last_up_button = up_button
            last_down_button = down_button

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
    if menuvsr_process:
        menuvsr_process.terminate()  # Terminer le processus si encore actif
    pygame.quit()
