#!/usr/bin/env python3

import evdev
import uinput
import os
import sys
import time
import math
import subprocess

# Configuration
SPEED = 10       # Vitesse de base pour la souris
DEAD_ZONE = 15   # Zone morte pour les joysticks
UPDATE_DELAY = 0.01  # Délai (10 ms, 100 Hz)
SMOOTHING = 0.015  # Lissage léger
SMALL_MOVEMENT_BOOST = 2  # Amplification des petits mouvements
RESET_THRESHOLD = 10  # Seuil pour arrêter le mouvement
START_SOUND_CMD = ["ffplay", "-nodisp", "-autoexit", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/clic.wav"]
EXIT_SOUND_CMD = ["ffplay", "-nodisp", "-autoexit", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/noclic.wav"]

def find_controller():
    devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
    for dev in devices:
        print(f"Device: {dev.path} - {dev.name}")
        if ("Sony" in dev.name or "DualSense" in dev.name or "DualShock" in dev.name or "Wireless Controller" in dev.name) and "Touchpad" not in dev.name and "Motion Sensors" not in dev.name:
            print(f"Trouvé : {dev.path} - {dev.name}")
            return dev.path
    print("Erreur : Aucune manette Sony valide trouvée.")
    sys.exit(1)

def setup_uinput_device():
    events = [
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.BTN_MIDDLE),  # Ajout pour clic milieu
        (evdev.ecodes.EV_REL, evdev.ecodes.REL_X),
        (evdev.ecodes.EV_REL, evdev.ecodes.REL_Y),
        # Touches clavier
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_W),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_A),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_S),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_D),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_UP),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_DOWN),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_RIGHT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTSHIFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_E),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ENTER),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_BACKSPACE),
    ]
    try:
        device = uinput.Device(events, name="virtual-controller")
        print("Périphérique virtuel créé avec succès.")
        return device
    except PermissionError:
        print("Erreur : Permissions insuffisantes pour uinput. Essayez avec sudo.")
        sys.exit(1)

def calculate_movement(x_value, y_value, last_x, last_y, dt, is_mouse=False):
    if abs(x_value) < DEAD_ZONE:
        x_value = 0
    if abs(y_value) < DEAD_ZONE:
        y_value = 0

    if x_value == 0 and y_value == 0:
        return 0, 0

    if is_mouse:
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
    else:
        return x_value, y_value

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

    virtual_device = setup_uinput_device()
    play_sound(START_SOUND_CMD)
    print("Son de démarrage lancé en arrière-plan.")

    last_mouse_x, last_mouse_y = 0, 0
    last_time = time.time()
    last_l2 = 0
    last_r2 = 0
    last_r3 = 0  # Ajout pour suivre l'état de R3
    last_cross = 0
    last_circle = 0
    last_triangle = 0
    last_square = 0
    last_l3 = 0
    l3_press_count = 0
    last_dpad_up = 0
    last_dpad_down = 0
    last_dpad_left = 0
    last_dpad_right = 0

    while True:
        current_time = time.time()
        dt = current_time - last_time

        # Joystick droit (souris)
        rx_value = controller.absinfo(evdev.ecodes.ABS_RX).value - 128
        ry_value = controller.absinfo(evdev.ecodes.ABS_RY).value - 128
        mouse_x, mouse_y = calculate_movement(rx_value, ry_value, last_mouse_x, last_mouse_y, dt, is_mouse=True)

        if mouse_x != 0:
            virtual_device.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_X), mouse_x)
            virtual_device.syn()
        if mouse_y != 0:
            virtual_device.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_Y), mouse_y)
            virtual_device.syn()

        last_mouse_x = mouse_x
        last_mouse_y = mouse_y

        # Joystick gauche (WASD)
        lx_value = controller.absinfo(evdev.ecodes.ABS_X).value - 128
        ly_value = controller.absinfo(evdev.ecodes.ABS_Y).value - 128
        move_x, move_y = calculate_movement(lx_value, ly_value, 0, 0, dt, is_mouse=False)

        w_state = 1 if move_y < -DEAD_ZONE else 0
        s_state = 1 if move_y > DEAD_ZONE else 0
        a_state = 1 if move_x < -DEAD_ZONE else 0
        d_state = 1 if move_x > DEAD_ZONE else 0

        virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_W), w_state)
        virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_S), s_state)
        virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_A), a_state)
        virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_D), d_state)
        virtual_device.syn()

        # D-pad (flèches)
        dpad_up_state = controller.absinfo(evdev.ecodes.ABS_HAT0Y).value == -1
        dpad_down_state = controller.absinfo(evdev.ecodes.ABS_HAT0Y).value == 1
        dpad_left_state = controller.absinfo(evdev.ecodes.ABS_HAT0X).value == -1
        dpad_right_state = controller.absinfo(evdev.ecodes.ABS_HAT0X).value == 1

        if dpad_up_state != last_dpad_up:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_UP), 1 if dpad_up_state else 0)
            virtual_device.syn()
            last_dpad_up = dpad_up_state
        if dpad_down_state != last_dpad_down:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_DOWN), 1 if dpad_down_state else 0)
            virtual_device.syn()
            last_dpad_down = dpad_down_state
        if dpad_left_state != last_dpad_left:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFT), 1 if dpad_left_state else 0)
            virtual_device.syn()
            last_dpad_left = dpad_left_state
        if dpad_right_state != last_dpad_right:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_RIGHT), 1 if dpad_right_state else 0)
            virtual_device.syn()
            last_dpad_right = dpad_right_state

        # Boutons
        l2_state = controller.active_keys().count(evdev.ecodes.BTN_TL2)     # L2
        r2_state = controller.active_keys().count(evdev.ecodes.BTN_TR2)     # R2
        r3_state = controller.active_keys().count(evdev.ecodes.BTN_THUMBR)  # R3 (clic milieu)
        cross_state = controller.active_keys().count(evdev.ecodes.BTN_SOUTH)   # Croix
        circle_state = controller.active_keys().count(evdev.ecodes.BTN_EAST)    # Rond
        triangle_state = controller.active_keys().count(evdev.ecodes.BTN_NORTH) # Triangle
        square_state = controller.active_keys().count(evdev.ecodes.BTN_WEST)    # Carré
        l3_state = controller.active_keys().count(evdev.ecodes.BTN_THUMBL)      # L3
        ps_state = controller.active_keys().count(evdev.ecodes.BTN_MODE)        # PS

        # L2 -> Clic droit
        if l2_state != last_l2:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT), l2_state)
            virtual_device.syn()
            last_l2 = l2_state

        # R2 -> Clic gauche
        if r2_state != last_r2:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT), r2_state)
            virtual_device.syn()
            last_r2 = r2_state

        # R3 -> Clic milieu
        if r3_state != last_r3:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_MIDDLE), r3_state)
            virtual_device.syn()
            last_r3 = r3_state

        # Croix -> Enter
        if cross_state != last_cross:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ENTER), 1 if cross_state else 0)
            virtual_device.syn()
            last_cross = cross_state

        # Rond -> F
        if circle_state != last_circle:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F), 1 if circle_state else 0)
            virtual_device.syn()
            last_circle = circle_state

        # Triangle -> E
        if triangle_state != last_triangle:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_E), 1 if triangle_state else 0)
            virtual_device.syn()
            last_triangle = triangle_state

        # Carré -> Backspace
        if square_state != last_square:
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_BACKSPACE), 1 if square_state else 0)
            virtual_device.syn()
            last_square = square_state

        # L3 -> Shift (toggle)
        if l3_state != last_l3 and l3_state == 1:
            l3_press_count += 1
            shift_state = 1 if l3_press_count % 2 == 1 else 0
            virtual_device.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTSHIFT), shift_state)
            virtual_device.syn()
            last_l3 = l3_state

        # Sortie avec PS + L3
        if ps_state == 1 and l3_state == 1:
            print("Bouton PS et L3 pressés : arrêt.")
            play_sound(EXIT_SOUND_CMD)
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
