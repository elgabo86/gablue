# Version 1.5
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

# Fonction pour obtenir les indices des boutons selon le type de manette
def get_button_indices(joystick):
    controller_name = joystick.get_name().lower()
    print(f"Controller detected: {controller_name}")

    # Par défaut, utiliser les indices pour DualSense/DualShock (PS4/PS5)
    home_button_idx = 5  # PS Button
    select_button_idx = 4  # Share Button
    start_button_idx = 6  # Options Button
    triangle_button_idx = 3  # Triangle
    square_button_idx = 2  # Square
    circle_button_idx = 1  # Circle
    l3_button_idx = 7  # L3
    r3_button_idx = 8  # R3

    if "xbox" in controller_name:
        # Xbox 360/One/Series
        home_button_idx = 10  # Guide Button
        select_button_idx = 6  # Back Button
        start_button_idx = 7  # Start Button
        triangle_button_idx = 3  # Y Button
        square_button_idx = 2  # X Button
        circle_button_idx = 1  # B Button
        l3_button_idx = 8  # L. Stick In
        r3_button_idx = 9  # R. Stick In
    elif "nintendo switch pro" in controller_name:
        # Nintendo Switch Pro Controller
        home_button_idx = 5  # Home Button
        select_button_idx = 4  # - Button
        start_button_idx = 6  # + Button
        triangle_button_idx = 3  # Y Button
        square_button_idx = 2  # X Button
        circle_button_idx = 1  # B Button
        l3_button_idx = 7  # L. Stick In
        r3_button_idx = 8  # R. Stick In

    return {
        "home": home_button_idx,
        "select": select_button_idx,
        "start": start_button_idx,
        "triangle": triangle_button_idx,
        "square": square_button_idx,
        "circle": circle_button_idx,
        "l3": l3_button_idx,
        "r3": r3_button_idx
    }

# Boucle principale
try:
    joystick = None
    button_indices = None
    while True:
        # Traiter tous les événements en file d'attente
        for event in pygame.event.get():
            if event.type == pygame.JOYDEVICEADDED:
                print("Manette connectée.")
                joystick = pygame.joystick.Joystick(0)
                joystick.init()
                num_buttons = joystick.get_numbuttons()
                print(f"Nombre de boutons détectés : {num_buttons}")
                button_indices = get_button_indices(joystick)
                pygame.display.set_allow_screensaver(0)

            elif event.type == pygame.JOYDEVICEREMOVED:
                print("Manette déconnectée.")
                if joystick:
                    joystick.quit()
                joystick = None
                button_indices = None
                # Désactiver la prévention de mise en veille
                pygame.display.set_allow_screensaver(1)

            elif event.type == pygame.JOYBUTTONDOWN and joystick and button_indices:
                num_buttons = joystick.get_numbuttons()
                home_button = joystick.get_button(button_indices["home"]) if num_buttons > button_indices["home"] else 0
                select_button = joystick.get_button(button_indices["select"]) if num_buttons > button_indices["select"] else 0
                start_button = joystick.get_button(button_indices["start"]) if num_buttons > button_indices["start"] else 0
                triangle_button = joystick.get_button(button_indices["triangle"]) if num_buttons > button_indices["triangle"] else 0
                square_button = joystick.get_button(button_indices["square"]) if num_buttons > button_indices["square"] else 0
                circle_button = joystick.get_button(button_indices["circle"]) if num_buttons > button_indices["circle"] else 0
                l3_button = joystick.get_button(button_indices["l3"]) if num_buttons > button_indices["l3"] else 0
                r3_button = joystick.get_button(button_indices["r3"]) if num_buttons > button_indices["r3"] else 0

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
                elif home_button and circle_button and not menuvsr_script_running:
                    print("MENUVR")
                    menuvsr_process = subprocess.Popen(
                        ["python", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/menuvsr.py"]
                    )
                    menuvsr_script_running = True
                    menuvsr_process.wait()  # Attendre la fin de l'exécution
                    menuvsr_script_running = False
                    menuvsr_process = None
                    print("Le script menuvsr.py s'est terminé.")

        # Vérifier en continu l'état du joystick si connecté
        if joystick and button_indices:
            # Vérifier l'état actuel des boutons, axes et hat
            num_buttons = joystick.get_numbuttons()
            home_button = joystick.get_button(button_indices["home"]) if num_buttons > button_indices["home"] else 0
            axis_1 = joystick.get_axis(1)  # -1 = haut, 1 = bas
            hat_value = joystick.get_hat(0) if joystick.get_numhats() > 0 else (0, 0)
            left_button = 1 if hat_value == (-1, 0) else 0
            up_button = 1 if hat_value == (0, 1) else 0
            right_button = 1 if hat_value == (1, 0) else 0
            down_button = 1 if hat_value == (0, -1) else 0

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
