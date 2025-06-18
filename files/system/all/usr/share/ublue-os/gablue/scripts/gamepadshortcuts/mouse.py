#!/usr/bin/env python3

import evdev
import uinput
import os
import sys
import time
import math
import subprocess

# Configuration
SPEED = 10       # Vitesse de base
DEAD_ZONE = 20   # Zone morte fixe, assez large pour couvrir X: -9
UPDATE_DELAY = 0.01  # Délai (10 ms, 100 Hz)
SMOOTHING = 0.015  # Lissage léger
SMALL_MOVEMENT_BOOST = 2  # Amplification légère des petits mouvements
RESET_THRESHOLD = 10  # Seuil de base pour arrêter le mouvement
START_SOUND_CMD = ["ffplay", "-nodisp", "-autoexit", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/clic.wav"]
EXIT_SOUND_CMD = ["ffplay", "-nodisp", "-autoexit", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/noclic.wav"]

def find_controller():
    devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
    for dev in devices:
        print(f"Device: {dev.path} - {dev.name}")
        if (("Sony" in dev.name or "DualSense" in dev.name or "DualShock" in dev.name or
             "Nintendo" in dev.name or "Xbox" in dev.name or "Microsoft" in dev.name)
            and "Touchpad" not in dev.name and "Motion Sensors" not in dev.name):
            print(f"Trouvé : {dev.path} - {dev.name}")
            return dev.path
    print("Erreur : Aucune manette valide trouvée (Sony, Nintendo, Xbox).")
    sys.exit(1)

def setup_uinput_device():
    events = [
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT),
        (evdev.ecodes.EV_REL, evdev.ecodes.REL_X),
        (evdev.ecodes.EV_REL, evdev.ecodes.REL_Y),
        # Touches clavier nécessaires
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_RIGHT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_UP),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_DOWN),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ENTER),      # Start
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ESC),        # L3
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTSHIFT),  # R2
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTALT),    # L2
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_BACKSPACE),  # Carré
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_SPACE),      # Croix
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_TAB),        # Rond
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F4),         # Triangle
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F11),        # Select
    ]
    try:
        device = uinput.Device(events, name="virtual-mouse")
        print("Périphérique virtuel créé avec succès.")
        return device
    except PermissionError:
        print("Erreur : Permissions insuffisantes pour uinput. Essayez avec sudo.")
        sys.exit(1)

def get_button_mappings(controller_name):
    controller_name = controller_name.lower()
    # Par défaut : Sony DualSense/DualShock
    mappings = {
        "south": evdev.ecodes.BTN_SOUTH,    # Croix (PS) / A (Xbox) / B (Switch)
        "east": evdev.ecodes.BTN_EAST,      # Rond (PS) / B (Xbox) / A (Switch)
        "north": evdev.ecodes.BTN_NORTH,    # Triangle (PS) / Y (Xbox) / Y (Switch)
        "west": evdev.ecodes.BTN_WEST,      # Carré (PS) / X (Xbox) / X (Switch)
        "tr": evdev.ecodes.BTN_TR,          # R1
        "tl": evdev.ecodes.BTN_TL,          # L1
        "tr2": evdev.ecodes.BTN_TR2,        # R2
        "tl2": evdev.ecodes.BTN_TL2,        # L2
        "thumbl": evdev.ecodes.BTN_THUMBL,  # L3
        "thumbr": evdev.ecodes.BTN_THUMBR,  # R3
        "start": evdev.ecodes.BTN_START,    # Options (PS) / Start (Xbox) / + (Switch)
        "select": evdev.ecodes.BTN_SELECT,  # Share (PS) / Back (Xbox) / - (Switch)
        "mode": evdev.ecodes.BTN_MODE,      # PS (PS) / Guide (Xbox) / Home (Switch)
    }

    if "xbox" in controller_name or "microsoft" in controller_name:
        # Xbox : mêmes codes evdev, mais A = South, B = East, etc.
        mappings.update({
            "south": evdev.ecodes.BTN_SOUTH,    # A
            "east": evdev.ecodes.BTN_EAST,      # B
            "north": evdev.ecodes.BTN_NORTH,    # Y
            "west": evdev.ecodes.BTN_WEST,      # X
        })
    elif "nintendo" in controller_name:
        # Switch Pro : B = South, A = East, etc.
        mappings.update({
            "south": evdev.ecodes.BTN_SOUTH,    # B
            "east": evdev.ecodes.BTN_EAST,      # A
            "north": evdev.ecodes.BTN_NORTH,    # Y
            "west": evdev.ecodes.BTN_WEST,      # X
        })

    return mappings

def calculate_movement(x_value, y_value, last_x, last_y, dt):
    if abs(x_value) < DEAD_ZONE:
        x_value = 0
    if abs(y_value) < DEAD_ZONE:
        y_value = 0

    if x_value == 0 and y_value == 0:
        return 0, 0

    if abs(x_value) < 20:
        x_value *= SMALL_MOVEMENT_BOOST
    if abs(y_value) < 20:
        y_value *= SMALL_MOVEMENT_BOOST

    normalized_x = x_value / 128.0
    normalized_y = y_value / 128.0
    raw_move_x = SPEED * normalized_x
    raw_move_y = SPEED * normalized_y

    raw_move_x = max(min(raw_move_x, SPEED), -SPEED)
    raw_move_y = max(min(raw_move_y, SPEED), -SPEED)

    move_x = int(last_x + (raw_move_x - last_x) * min(1.0, dt / SMOOTHING))
    move_y = int(last_y + (raw_move_y - last_y) * min(1.0, dt / SMOOTHING))

    if abs(x_value) < RESET_THRESHOLD:
        move_x = 0
        last_x = 0
    if abs(y_value) < RESET_THRESHOLD:
        move_y = 0
        last_y = 0

    return move_x, move_y

def play_sound(command):
    try:
        subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except Exception as e:
        print(f"Erreur lors du lancement du son : {e}")

def main():
    device_path = find_controller()
    if not os.path.exists(device_path):
        print(f"Erreur : {device_path} non trouvé.")
        sys.exit(1)

    try:
        controller = evdev.InputDevice(device_path)
        print(f"Utilisation de : {controller.name}")
    except PermissionError:
        print(f"Erreur : Permissions insuffisantes pour {device_path}.")
        sys.exit(1)

    mouse = setup_uinput_device()
    button_mappings = get_button_mappings(controller.name)
    play_sound(START_SOUND_CMD)
    print("Son de démarrage lancé en arrière-plan.")

    last_x, last_y = 0, 0
    last_time = time.time()
    last_left = 0
    last_right = 0
    last_dpad_up = 0
    last_dpad_down = 0
    last_dpad_left = 0
    last_dpad_right = 0
    last_cross = 0
    last_circle = 0
    last_triangle = 0
    last_square = 0
    last_l3 = 0
    last_l2 = 0
    last_r2 = 0
    last_start = 0
    last_select = 0
    last_mode = 0
    last_r3 = 0

    while True:
        current_time = time.time()
        dt = current_time - last_time

        abs_info_x = controller.absinfo(evdev.ecodes.ABS_RX)
        abs_info_y = controller.absinfo(evdev.ecodes.ABS_RY)
        x_value = ((abs_info_x.value - abs_info_x.min) / (abs_info_x.max - abs_info_x.min)) * 255 - 128
        y_value = ((abs_info_y.value - abs_info_y.min) / (abs_info_y.max - abs_info_y.min)) * 255 - 128

        move_x, move_y = calculate_movement(x_value, y_value, last_x, last_y, dt)

        if move_x != 0:
            mouse.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_X), move_x)
            mouse.syn()
        if move_y != 0:
            mouse.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_Y), move_y)
            mouse.syn()

        last_x = move_x
        last_y = move_y

        # Boutons
        left_state = controller.active_keys().count(button_mappings["tr"])       # R1
        right_state = controller.active_keys().count(button_mappings["tl"])      # L1
        mode_state = controller.active_keys().count(button_mappings["mode"])     # PS / Guide / Home
        r3_state = controller.active_keys().count(button_mappings["thumbr"])     # R3
        l3_state = controller.active_keys().count(button_mappings["thumbl"])     # L3
        l2_state = controller.active_keys().count(button_mappings["tl2"])        # L2
        r2_state = controller.active_keys().count(button_mappings["tr2"])        # R2
        start_state = controller.active_keys().count(button_mappings["start"])   # Start
        select_state = controller.active_keys().count(button_mappings["select"]) # Select
        cross_state = controller.active_keys().count(button_mappings["south"])   # Croix / A / B
        circle_state = controller.active_keys().count(button_mappings["east"])   # Rond / B / A
        triangle_state = controller.active_keys().count(button_mappings["north"]) # Triangle / Y
        square_state = controller.active_keys().count(button_mappings["west"])   # Carré / X

        # D-pad
        dpad_up_state = controller.absinfo(evdev.ecodes.ABS_HAT0Y).value == -1
        dpad_down_state = controller.absinfo(evdev.ecodes.ABS_HAT0Y).value == 1
        dpad_left_state = controller.absinfo(evdev.ecodes.ABS_HAT0X).value == -1
        dpad_right_state = controller.absinfo(evdev.ecodes.ABS_HAT0X).value == 1

        # Gestion des clics souris
        if left_state != last_left:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT), left_state)
            mouse.syn()
            last_left = left_state

        if right_state != last_right:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT), right_state)
            mouse.syn()
            last_right = right_state

        # D-pad -> Touches fléchées
        if dpad_up_state != last_dpad_up:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_UP), 1 if dpad_up_state else 0)
            mouse.syn()
            last_dpad_up = dpad_up_state

        if dpad_down_state != last_dpad_down:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_DOWN), 1 if dpad_down_state else 0)
            mouse.syn()
            last_dpad_down = dpad_down_state

        if dpad_left_state != last_dpad_left:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFT), 1 if dpad_left_state else 0)
            mouse.syn()
            last_dpad_left = dpad_left_state

        if dpad_right_state != last_dpad_right:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_RIGHT), 1 if dpad_right_state else 0)
            mouse.syn()
            last_dpad_right = dpad_right_state

        # L3 -> Esc
        if l3_state != last_l3:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ESC), 1 if l3_state else 0)
            mouse.syn()
            last_l3 = l3_state

        # L2 -> Alt gauche
        if l2_state != last_l2:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTALT), 1 if l2_state else 0)
            mouse.syn()
            last_l2 = l2_state

        # R2 -> Shift gauche
        if r2_state != last_r2:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTSHIFT), 1 if r2_state else 0)
            mouse.syn()
            last_r2 = r2_state

        # Start -> Entrée
        if start_state != last_start:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ENTER), 1 if start_state else 0)
            mouse.syn()
            last_start = start_state

        # Select -> F11
        if select_state != last_select:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F11), 1 if select_state else 0)
            mouse.syn()
            last_select = select_state

        # Carré -> Backspace
        if square_state != last_square:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_BACKSPACE), 1 if square_state else 0)
            mouse.syn()
            last_square = square_state

        # Croix -> Space
        if cross_state != last_cross:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_SPACE), 1 if cross_state else 0)
            mouse.syn()
            last_cross = cross_state

        # Rond -> Tab
        if circle_state != last_circle:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_TAB), 1 if circle_state else 0)
            mouse.syn()
            last_circle = circle_state

        # Triangle -> F4
        if triangle_state != last_triangle:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F4), 1 if triangle_state else 0)
            mouse.syn()
            last_triangle = triangle_state

        # Condition de sortie
        if mode_state == 1 and r3_state == 1:
            print("Bouton Home/Guide et R3 pressés : arrêt.")
            play_sound(EXIT_SOUND_CMD)
            print("Son de sortie lancé en arrière-plan.")
            time.sleep(0.7)
            sys.exit(0)

        last_time = current_time
        time.sleep(UPDATE_DELAY)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nArrêt par l'utilisateur.")
        sys.exit(0)
