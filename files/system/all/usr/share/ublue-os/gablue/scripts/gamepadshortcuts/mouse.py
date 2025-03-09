#!/usr/bin/env python3

import evdev
import uinput
import os
import sys
import time
import math
import subprocess

# Configuration
SPEED = 15       # Vitesse de base
DEAD_ZONE = 30   # Zone morte
UPDATE_DELAY = 0.01  # Délai (10 ms, 100 Hz)
SMOOTHING = 0.05  # Lissage réactif
SOUND_CMD = ["ffplay", "-nodisp", "-autoexit", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/clic.wav"]

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
    magnitude = math.sqrt(x_value ** 2 + y_value ** 2)
    if magnitude < DEAD_ZONE:
        return 0, 0
    normalized_x = x_value / 128.0
    normalized_y = y_value / 128.0
    raw_move_x = SPEED * normalized_x
    raw_move_y = SPEED * normalized_y
    move_x = int(last_x + (raw_move_x - last_x) * min(1.0, dt / SMOOTHING))
    move_y = int(last_y + (raw_move_y - last_y) * min(1.0, dt / SMOOTHING))
    return move_x, move_y

def play_ready_sound():
    try:
        subprocess.Popen(SOUND_CMD, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        print("Son de démarrage lancé en arrière-plan.")
    except Exception as e:
        print(f"Erreur lors du lancement du son en arrière-plan : {e}")

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

    # Joue le son en arrière-plan
    play_ready_sound()

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
        l3_state = controller.active_keys().count(evdev.ecodes.BTN_THUMBL)  # L3
        r3_state = controller.active_keys().count(evdev.ecodes.BTN_THUMBR)  # R3

        if left_state != last_left:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT), left_state)
            mouse.syn()
            last_left = left_state

        if right_state != last_right:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT), right_state)
            mouse.syn()
            last_right = right_state

        if l3_state == 1 and r3_state == 1:
            print("L3 et R3 pressés : arrêt.")
            sys.exit(0)

        last_time = current_time
        time.sleep(UPDATE_DELAY)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nArrêt par l'utilisateur.")
        sys.exit(0)
