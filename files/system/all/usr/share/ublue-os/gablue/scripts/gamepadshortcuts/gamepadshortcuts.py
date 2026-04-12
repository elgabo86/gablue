#!/usr/bin/python3

import evdev
import select
import subprocess
import time
import os
import dbus
from evdev import InputDevice, categorize, ecodes

# État global
mouse_script_running = False
menuvsr_script_running = False
mouse_process = None
menuvsr_process = None
last_volume_time = 0
volume_cooldown = 0.2
last_hat_state = (0, 0)
screensaver_cookie = None
DEBUG = True

# Mapping boutons générique (peut varier selon le contrôleur)
# Valeurs standard Linux evdev
BTN_MAPPING = {
    "home": ecodes.BTN_MODE,
    "select": ecodes.BTN_SELECT,
    "start": ecodes.BTN_START,
    "triangle": ecodes.BTN_C,
    "square": ecodes.BTN_X,
    "circle": ecodes.BTN_B,
    "cross": ecodes.BTN_A,
    "l3": ecodes.BTN_THUMBL,
    "r3": ecodes.BTN_THUMBR,
    "l1": ecodes.BTN_TL,
    "r1": ecodes.BTN_TR,
}

# Axes
ABS_HAT0X = ecodes.ABS_HAT0X
ABS_HAT0Y = ecodes.ABS_HAT0Y
ABS_Y = ecodes.ABS_Y

def inhibit_screensaver():
    """Inhibe l'extinction d'écran et le verrouillage via DBus."""
    global screensaver_cookie
    try:
        bus = dbus.SessionBus()
        saver = bus.get_object('org.freedesktop.ScreenSaver', '/ScreenSaver')
        saver_interface = dbus.Interface(saver, dbus_interface='org.freedesktop.ScreenSaver')
        screensaver_cookie = saver_interface.Inhibit("gablue-gamepadshortcuts", "Manette connectée")
        if DEBUG:
            print(f"Inhibition écran activée (cookie: {screensaver_cookie})")
    except Exception as e:
        print(f"Erreur inhibition écran: {e}")
        screensaver_cookie = None

def uninhibit_screensaver():
    """Rétablit l'extinction d'écran et le verrouillage via DBus."""
    global screensaver_cookie
    if screensaver_cookie is not None:
        try:
            bus = dbus.SessionBus()
            saver = bus.get_object('org.freedesktop.ScreenSaver', '/ScreenSaver')
            saver_interface = dbus.Interface(saver, dbus_interface='org.freedesktop.ScreenSaver')
            saver_interface.UnInhibit(screensaver_cookie)
            if DEBUG:
                print("Inhibition écran désactivée")
        except Exception as e:
            print(f"Erreur désactivation inhibition écran: {e}")
        finally:
            screensaver_cookie = None

def find_gamepad():
    """Trouve le premier gamepad connecté."""
    devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
    for device in devices:
        capabilities = device.capabilities()
        if ecodes.EV_KEY in capabilities:
            keys = capabilities[ecodes.EV_KEY]
            if ecodes.BTN_A in keys or ecodes.BTN_SELECT in keys:
                if DEBUG:
                    print(f"Manette trouvée: {device.name} ({device.path})")
                return device
    return None

def get_button_state(device, button_code):
    """Retourne l'état actuel d'un bouton."""
    return device.active_keys() if button_code in device.active_keys() else 0

def main():
    global mouse_script_running, menuvsr_script_running
    global mouse_process, menuvsr_process
    global last_volume_time, last_hat_state

    gamepad = find_gamepad()
    if gamepad:
        inhibit_screensaver()
    else:
        print("Aucune manette détectée au démarrage.")

    # Boutons actuels maintenus
    home_pressed = False
    select_pressed = False
    start_pressed = False
    triangle_pressed = False
    square_pressed = False
    circle_pressed = False
    l3_pressed = False
    r3_pressed = False

    hat_x = 0
    hat_y = 0
    axis_1 = 0.0

    try:
        while True:
            if not gamepad:
                time.sleep(1)
                gamepad = find_gamepad()
                if gamepad:
                    inhibit_screensaver()
                continue

            try:
                r, w, x = select.select([gamepad], [], [], 0.1)
            except (OSError, IOError):
                print("Manette déconnectée (erreur IO).")
                uninhibit_screensaver()
                gamepad = find_gamepad()
                if gamepad:
                    inhibit_screensaver()
                continue

            if gamepad in r:
                try:
                    for event in gamepad.read():
                        if event.type == ecodes.EV_KEY:
                            # Événements boutons
                            if event.code == BTN_MAPPING["home"]:
                                home_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["select"]:
                                select_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["start"]:
                                start_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["triangle"]:
                                triangle_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["square"]:
                                square_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["circle"]:
                                circle_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["l3"]:
                                l3_pressed = event.value == 1
                            elif event.code == BTN_MAPPING["r3"]:
                                r3_pressed = event.value == 1

                            # Traitement des combinaisons au press
                            if event.value == 1 and home_pressed:
                                if select_pressed:
                                    print("KILL")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/killthemall &")
                                elif start_pressed:
                                    print("ES")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/openes &")
                                elif r3_pressed and not mouse_script_running:
                                    print("MOUSE")
                                    mouse_process = subprocess.Popen(
                                        ["/usr/bin/python3", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/mouse.py"]
                                    )
                                    mouse_script_running = True
                                elif l3_pressed:
                                    print("MUTE")
                                    os.system("pactl set-sink-mute @DEFAULT_SINK@ toggle")
                                elif triangle_pressed:
                                    print("LAUNCHYT")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/launchyt &")
                                    time.sleep(1)
                                elif circle_pressed and not menuvsr_script_running:
                                    print("MENUVR")
                                    menuvsr_process = subprocess.Popen(
                                        ["/usr/bin/python3", "/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/menuvsr.py"]
                                    )
                                    menuvsr_script_running = True

                        elif event.type == ecodes.EV_ABS:
                            # D-Pad (hat)
                            if event.code == ABS_HAT0X:
                                hat_x = event.value
                            elif event.code == ABS_HAT0Y:
                                hat_y = event.value
                            # Stick gauche Y (axe 1) - plage 0-255, centre à 128
                            elif event.code == ABS_Y:
                                axis_1 = (event.value - 128) / 127.0

                            # Traitement hat
                            hat_value = (hat_x, hat_y)
                            if home_pressed and hat_value != last_hat_state:
                                if hat_value == (-1, 0):
                                    print("SCREEN")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/takescreenshot &")
                                    time.sleep(0.1)
                                elif hat_value == (1, 0):
                                    print("RECORD")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/startstoprecord &")
                                    time.sleep(2)
                                elif hat_value == (0, 1):
                                    print("FPS")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/changefps")
                                    time.sleep(0.1)
                                elif hat_value == (0, -1):
                                    print("MANGO")
                                    os.system("/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/showhidemango")
                                    time.sleep(0.1)
                                last_hat_state = hat_value

                except (OSError, IOError, BlockingIOError):
                    print("Erreur lecture manette, reconnexion...")
                    uninhibit_screensaver()
                    gamepad = find_gamepad()
                    if gamepad:
                        inhibit_screensaver()
                    continue

            # Traitement volume (en continu, pas seulement sur événement)
            current_time = time.time()
            if home_pressed and current_time - last_volume_time > volume_cooldown:
                if axis_1 < -0.5:
                    print("VOLUME UP")
                    os.system("pactl set-sink-volume @DEFAULT_SINK@ +10%")
                    last_volume_time = current_time
                elif axis_1 > 0.5:
                    print("VOLUME DOWN")
                    os.system("pactl set-sink-volume @DEFAULT_SINK@ -10%")
                    last_volume_time = current_time

            # Vérifier si les processus se sont terminés
            if mouse_script_running and mouse_process and mouse_process.poll() is not None:
                mouse_script_running = False
                mouse_process = None
                print("mouse.py terminé.")

            if menuvsr_script_running and menuvsr_process and menuvsr_process.poll() is not None:
                menuvsr_script_running = False
                menuvsr_process = None
                print("menuvsr.py terminé.")

    except KeyboardInterrupt:
        print("Arrêt du script.")
    finally:
        uninhibit_screensaver()
        if mouse_process:
            mouse_process.terminate()
        if menuvsr_process:
            menuvsr_process.terminate()
        if gamepad:
            gamepad.close()

if __name__ == "__main__":
    main()
