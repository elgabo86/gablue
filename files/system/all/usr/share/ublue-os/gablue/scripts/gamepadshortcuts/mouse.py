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
DEAD_ZONE = 15   # Zone morte fixe, assez large pour couvrir X: -9
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
        if ("Sony" in dev.name or "DualSense" in dev.name or "DualShock" in dev.name or "Wireless Controller" in dev.name) and "Touchpad" not in dev.name and "Motion Sensors" not in dev.name:
            print(f"Trouvé : {dev.path} - {dev.name}")
            return dev.path
    print("Erreur : Aucune manette Sony valide trouvée (touchpad et motion sensors exclus).")
    sys.exit(1)

def setup_uinput_device():
    events = [
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT),
        (evdev.ecodes.EV_REL, evdev.ecodes.REL_X),
        (evdev.ecodes.EV_REL, evdev.ecodes.REL_Y),
    ]
    try:
        device = uinput.Device(events, name="virtual-mouse")
        print("Périphérique virtuel créé avec succès.")
        return device
    except PermissionError:
        print("Erreur : Permissions insuffisantes pour uinput. Essayez avec sudo.")
        sys.exit(1)

def calculate_movement(x_value, y_value, last_x, last_y, dt):
    # Applique une zone morte fixe
    if abs(x_value) < DEAD_ZONE:
        x_value = 0
    if abs(y_value) < DEAD_ZONE:
        y_value = 0

    # Si pas de mouvement après ajustement, réinitialise
    if x_value == 0 and y_value == 0:
        return 0, 0

    # Amplification des petits mouvements
    if abs(x_value) < 20:
        x_value *= SMALL_MOVEMENT_BOOST
    if abs(y_value) < 20:
        y_value *= SMALL_MOVEMENT_BOOST

    # Normalisation et calcul de la vitesse
    normalized_x = x_value / 128.0
    normalized_y = y_value / 128.0
    raw_move_x = SPEED * normalized_x
    raw_move_y = SPEED * normalized_y

    # Limite la vitesse max
    raw_move_x = max(min(raw_move_x, SPEED), -SPEED)
    raw_move_y = max(min(raw_move_y, SPEED), -SPEED)

    # Lissage
    move_x = int(last_x + (raw_move_x - last_x) * min(1.0, dt / SMOOTHING))
    move_y = int(last_y + (raw_move_y - last_y) * min(1.0, dt / SMOOTHING))

    # Réinitialise si proche du centre
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
    play_sound(START_SOUND_CMD)
    print("Son de démarrage lancé en arrière-plan.")

    last_x, last_y = 0, 0
    last_time = time.time()
    last_left = 0
    last_right = 0

    while True:
        current_time = time.time()
        dt = current_time - last_time

        x_value = controller.absinfo(evdev.ecodes.ABS_RX).value - 128
        y_value = controller.absinfo(evdev.ecodes.ABS_RY).value - 128

        move_x, move_y = calculate_movement(x_value, y_value, last_x, last_y, dt)

        if move_x != 0:
            mouse.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_X), move_x)
            mouse.syn()
        if move_y != 0:
            mouse.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_Y), move_y)
            mouse.syn()

        last_x = move_x
        last_y = move_y

        left_state = controller.active_keys().count(evdev.ecodes.BTN_TR)  # R1
        right_state = controller.active_keys().count(evdev.ecodes.BTN_TL)  # L1
        ps_state = controller.active_keys().count(evdev.ecodes.BTN_MODE)  # Bouton PS (Home)
        r3_state = controller.active_keys().count(evdev.ecodes.BTN_THUMBR)  # R3

        if left_state != last_left:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT), left_state)
            mouse.syn()
            last_left = left_state

        if right_state != last_right:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT), right_state)
            mouse.syn()
            last_right = right_state

        if ps_state == 1 and r3_state == 1:
            print("Bouton PS et R3 pressés : arrêt.")
            play_sound(EXIT_SOUND_CMD)
            print("Son de sortie lancé en arrière-plan.")
            time.sleep(0.7)  # Délai pour laisser le son jouer
            sys.exit(0)

        last_time = current_time
        time.sleep(UPDATE_DELAY)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nArrêt par l'utilisateur.")
        sys.exit(0)
