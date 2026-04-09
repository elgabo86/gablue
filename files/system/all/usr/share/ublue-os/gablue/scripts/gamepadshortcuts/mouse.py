#!/usr/bin/python3

import evdev
import uinput
import os
import sys
import time
import subprocess

# Configuration du mouvement - Style FPS moderne
DEAD_ZONE = 0.08
BASE_SENSITIVITY = 2
MAX_SENSITIVITY = 12
UPDATE_RATE = 120

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
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_RIGHT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_UP),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_DOWN),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ENTER),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ESC),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTSHIFT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTALT),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_BACKSPACE),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_SPACE),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_TAB),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F4),
        (evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F11),
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
    mappings = {
        "south": evdev.ecodes.BTN_SOUTH,
        "east": evdev.ecodes.BTN_EAST,
        "north": evdev.ecodes.BTN_NORTH,
        "west": evdev.ecodes.BTN_WEST,
        "tr": evdev.ecodes.BTN_TR,
        "tl": evdev.ecodes.BTN_TL,
        "tr2": evdev.ecodes.BTN_TR2,
        "tl2": evdev.ecodes.BTN_TL2,
        "thumbl": evdev.ecodes.BTN_THUMBL,
        "thumbr": evdev.ecodes.BTN_THUMBR,
        "start": evdev.ecodes.BTN_START,
        "select": evdev.ecodes.BTN_SELECT,
        "mode": evdev.ecodes.BTN_MODE,
    }
    return mappings

def normalize_axis(value, min_val, max_val):
    normalized = (value - min_val) / (max_val - min_val)
    return (normalized * 2) - 1

def apply_deadzone(value, deadzone):
    if abs(value) < deadzone:
        return 0.0
    sign = 1.0 if value > 0 else -1.0
    return sign * ((abs(value) - deadzone) / (1.0 - deadzone))

def apply_easing_quadratic_curve(normalized_value):
    abs_val = abs(normalized_value)
    sign = 1.0 if normalized_value > 0 else -1.0
    
    if abs_val <= 0.35:
        multiplier = 0.2
    elif abs_val <= 0.75:
        t = (abs_val - 0.35) / 0.4
        multiplier = 0.2 + 0.5 * t
    else:
        t = (abs_val - 0.75) / 0.25
        eased = t * t
        multiplier = 0.7 + 0.3 * eased
    
    return sign * multiplier * abs_val

def play_sound(command):
    try:
        subprocess.Popen(command, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    except Exception as e:
        print(f"Erreur lors du lancement du son : {e}")

class MouseAccumulator:
    def __init__(self):
        self.carry_x = 0.0
        self.carry_y = 0.0
    
    def add_movement(self, dx, dy):
        self.carry_x += dx
        self.carry_y += dy
    
    def get_int_movement(self):
        int_x = int(round(self.carry_x))
        int_y = int(round(self.carry_y))
        self.carry_x -= int_x
        self.carry_y -= int_y
        return int_x, int_y

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

    abs_info_rx = controller.absinfo(evdev.ecodes.ABS_RX)
    abs_info_ry = controller.absinfo(evdev.ecodes.ABS_RY)

    accumulator = MouseAccumulator()
    
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

    update_interval = 1.0 / UPDATE_RATE

    while True:
        loop_start = time.time()
        
        current_time = time.time()
        dt = current_time - last_time
        last_time = current_time
        
        if dt > 0.1:
            dt = 0.1

        raw_x = controller.absinfo(evdev.ecodes.ABS_RX).value
        raw_y = controller.absinfo(evdev.ecodes.ABS_RY).value

        norm_x = normalize_axis(raw_x, abs_info_rx.min, abs_info_rx.max)
        norm_y = normalize_axis(raw_y, abs_info_ry.min, abs_info_ry.max)

        raw_magnitude = (norm_x ** 2 + norm_y ** 2) ** 0.5
        
        if raw_magnitude < DEAD_ZONE:
            magnitude = 0
        else:
            if raw_magnitude > 0.001:
                dir_x = norm_x / raw_magnitude
                dir_y = norm_y / raw_magnitude
            else:
                dir_x = 0
                dir_y = 0
            magnitude = (raw_magnitude - DEAD_ZONE) / (1.0 - DEAD_ZONE)
        
        if magnitude > 0.001:
            curve_magnitude = apply_easing_quadratic_curve(magnitude)
            
            speed = BASE_SENSITIVITY + (MAX_SENSITIVITY - BASE_SENSITIVITY) * curve_magnitude
            
            move_x = dir_x * speed * dt * 60
            move_y = dir_y * speed * dt * 60
            
            accumulator.add_movement(move_x, move_y)
            
            int_x, int_y = accumulator.get_int_movement()
            
            if int_x != 0 or int_y != 0:
                if int_x != 0:
                    mouse.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_X), int_x)
                if int_y != 0:
                    mouse.emit((evdev.ecodes.EV_REL, evdev.ecodes.REL_Y), int_y)
                mouse.syn()

        left_state = controller.active_keys().count(button_mappings["tr"])
        right_state = controller.active_keys().count(button_mappings["tl"])
        mode_state = controller.active_keys().count(button_mappings["mode"])
        r3_state = controller.active_keys().count(button_mappings["thumbr"])
        l3_state = controller.active_keys().count(button_mappings["thumbl"])
        l2_state = controller.active_keys().count(button_mappings["tl2"])
        r2_state = controller.active_keys().count(button_mappings["tr2"])
        start_state = controller.active_keys().count(button_mappings["start"])
        select_state = controller.active_keys().count(button_mappings["select"])
        cross_state = controller.active_keys().count(button_mappings["south"])
        circle_state = controller.active_keys().count(button_mappings["east"])
        triangle_state = controller.active_keys().count(button_mappings["north"])
        square_state = controller.active_keys().count(button_mappings["west"])

        dpad_up_state = controller.absinfo(evdev.ecodes.ABS_HAT0Y).value == -1
        dpad_down_state = controller.absinfo(evdev.ecodes.ABS_HAT0Y).value == 1
        dpad_left_state = controller.absinfo(evdev.ecodes.ABS_HAT0X).value == -1
        dpad_right_state = controller.absinfo(evdev.ecodes.ABS_HAT0X).value == 1

        if left_state != last_left:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_LEFT), left_state)
            mouse.syn()
            last_left = left_state

        if right_state != last_right:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.BTN_RIGHT), right_state)
            mouse.syn()
            last_right = right_state

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

        if l3_state != last_l3:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ESC), 1 if l3_state else 0)
            mouse.syn()
            last_l3 = l3_state

        if l2_state != last_l2:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTALT), 1 if l2_state else 0)
            mouse.syn()
            last_l2 = l2_state

        if r2_state != last_r2:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_LEFTSHIFT), 1 if r2_state else 0)
            mouse.syn()
            last_r2 = r2_state

        if start_state != last_start:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_ENTER), 1 if start_state else 0)
            mouse.syn()
            last_start = start_state

        if select_state != last_select:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F11), 1 if select_state else 0)
            mouse.syn()
            last_select = select_state

        if square_state != last_square:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_BACKSPACE), 1 if square_state else 0)
            mouse.syn()
            last_square = square_state

        if cross_state != last_cross:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_SPACE), 1 if cross_state else 0)
            mouse.syn()
            last_cross = cross_state

        if circle_state != last_circle:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_TAB), 1 if circle_state else 0)
            mouse.syn()
            last_circle = circle_state

        if triangle_state != last_triangle:
            mouse.emit((evdev.ecodes.EV_KEY, evdev.ecodes.KEY_F4), 1 if triangle_state else 0)
            mouse.syn()
            last_triangle = triangle_state

        if mode_state == 1 and r3_state == 1:
            print("Bouton Home/Guide et R3 pressés : arrêt.")
            play_sound(EXIT_SOUND_CMD)
            print("Son de sortie lancé en arrière-plan.")
            time.sleep(0.7)
            sys.exit(0)

        elapsed = time.time() - loop_start
        if elapsed < update_interval:
            time.sleep(update_interval - elapsed)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\nArrêt par l'utilisateur.")
        sys.exit(0)
