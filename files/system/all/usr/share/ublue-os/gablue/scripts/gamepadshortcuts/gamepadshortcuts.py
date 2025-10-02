import pygame
import os
import subprocess
import time

# Initialiser Pygame
pygame.init()
pygame.display.set_allow_screensaver(1)
pygame.joystick.init()
pygame.mixer.quit()
pygame.font.quit()

clock = pygame.time.Clock()
mouse_script_running = False
menuvsr_script_running = False
mouse_process = None
menuvsr_process = None
last_volume_time = 0
volume_cooldown = 200
last_hat_state = (0, 0)
DEBUG = False  # Activé pour voir les messages

def get_button_indices(joystick):
    controller_name = joystick.get_name().lower()
    print(f"Controller detected: {controller_name}")
    indices = {
        "home": 10, "select": 8, "start": 9, "triangle": 2,
        "square": 3, "circle": 1, "l3": 11, "r3": 12
    }
    if "xbox" in controller_name:
        indices.update({"home": 10, "select": 6, "start": 7, "triangle": 3, "square": 2, "circle": 1, "l3": 8, "r3": 9})
    elif "nintendo switch pro" in controller_name:
        indices.update({"home": 5, "select": 4, "start": 6, "triangle": 3, "square": 2, "circle": 1, "l3": 7, "r3": 8})
    return indices

try:
    joystick = None
    button_indices = None
    num_buttons = 0
    num_hats = 0

    def init_first_joystick():
        global joystick, button_indices, num_buttons, num_hats
        if pygame.joystick.get_count() > 0:
            joystick = pygame.joystick.Joystick(0)
            joystick.init()
            num_buttons = joystick.get_numbuttons()
            num_hats = joystick.get_numhats()
            button_indices = get_button_indices(joystick)
            pygame.display.set_allow_screensaver(0)
            print(f"Manette connectée, boutons: {num_buttons}")
        else:
            joystick = None
            button_indices = None
            pygame.display.set_allow_screensaver(1)
            print("Aucune manette détectée.")

    # Initialiser la première manette au démarrage
    init_first_joystick()

    while True:
        for event in pygame.event.get():
            if event.type == pygame.JOYDEVICEADDED:
                if not joystick:  # Si aucune manette n'est active
                    init_first_joystick()

            elif event.type == pygame.JOYDEVICEREMOVED:
                if joystick:
                    joystick.quit()
                    joystick = None
                    button_indices = None
                    print("Manette déconnectée.")
                    init_first_joystick()  # Tenter de réinitialiser avec une autre manette

            elif event.type == pygame.JOYBUTTONDOWN and joystick and button_indices:
                home = joystick.get_button(button_indices["home"]) if num_buttons > button_indices["home"] else 0
                select = joystick.get_button(button_indices["select"]) if num_buttons > button_indices["select"] else 0
                start = joystick.get_button(button_indices["start"]) if num_buttons > button_indices["start"] else 0
                triangle = joystick.get_button(button_indices["triangle"]) if num_buttons > button_indices["triangle"] else 0
                square = joystick.get_button(button_indices["square"]) if num_buttons > button_indices["square"] else 0
                circle = joystick.get_button(button_indices["circle"]) if num_buttons > button_indices["circle"] else 0
                l3 = joystick.get_button(button_indices["l3"]) if num_buttons > button_indices["l3"] else 0
                r3 = joystick.get_button(button_indices["r3"]) if num_buttons > button_indices["r3"] else 0

                if home:
                    if select:
                        print("KILL")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/killthemall &")
                    elif start:
                        print("ES")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/openes &")
                    elif r3 and not mouse_script_running:
                        print("MOUSE")
                        mouse_process = subprocess.Popen(["python", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/mouse.py"])
                        mouse_script_running = True
                    elif l3:
                        print("MUTE")
                        os.system("pactl set-sink-mute @DEFAULT_SINK@ toggle")
                    elif triangle:
                        print("LAUNCHYT")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/launchyt &")
                        pygame.time.wait(1000)
                    elif circle and not menuvsr_script_running:
                        print("MENUVR")
                        menuvsr_process = subprocess.Popen(["python", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/menuvsr.py"])
                        menuvsr_script_running = True
                        menuvsr_process.wait()
                        menuvsr_script_running = False
                        menuvsr_process = None
                        print("menuvsr.py terminé.")

            elif event.type == pygame.JOYHATMOTION and joystick and button_indices:
                home = joystick.get_button(button_indices["home"]) if num_buttons > button_indices["home"] else 0
                hat_value = joystick.get_hat(0) if num_hats > 0 else (0, 0)
                if home and hat_value != last_hat_state:
                    if hat_value == (-1, 0):
                        print("SCREEN")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/takescreenshot &")
                        pygame.time.wait(100)
                    elif hat_value == (1, 0):
                        print("RECORD")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/startstoprecord &")
                        pygame.time.wait(2000)
                    elif hat_value == (0, 1):
                        print("FPS")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/changefps")
                        pygame.time.wait(100)
                    elif hat_value == (0, -1):
                        print("MANGO")
                        os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/showhidemango")
                        pygame.time.wait(100)
                last_hat_state = hat_value

            elif event.type == pygame.JOYAXISMOTION and joystick and button_indices:
                home = joystick.get_button(button_indices["home"]) if num_buttons > button_indices["home"] else 0
                axis_1 = joystick.get_axis(1)
                current_time = pygame.time.get_ticks()
                if home and current_time - last_volume_time > volume_cooldown:
                    if axis_1 < -0.5:
                        print("VOLUME UP")
                        os.system("pactl set-sink-volume @DEFAULT_SINK@ +10%")
                        last_volume_time = current_time
                    elif axis_1 > 0.5:
                        print("VOLUME DOWN")
                        os.system("pactl set-sink-volume @DEFAULT_SINK@ -10%")
                        last_volume_time = current_time

        if mouse_script_running and mouse_process and mouse_process.poll() is not None:
            mouse_script_running = False
            mouse_process = None
            print("mouse.py terminé.")

        clock.tick(10)

except KeyboardInterrupt:
    print("Arrêt du script.")
finally:
    if mouse_process:
        mouse_process.terminate()
    if menuvsr_process:
        menuvsr_process.terminate()
    pygame.quit()
