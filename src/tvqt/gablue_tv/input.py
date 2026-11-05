"""Manettes de jeu : lecture evdev, accélération et actions D-pad.

TOUTES les manettes connectées pilotent l'application ; l'état est tenu par
device et les fronts de boutons sont détectés par device.
"""

import select
import sys
import time

import evdev
from evdev import ecodes

from .constants import BASE, COOLDOWN_MS, SUFFIX

# Boutons manette (noms DualSense ; évite de semer les ecodes partout)
BTN_CROSS = ecodes.BTN_A
BTN_CIRCLE = ecodes.BTN_B
BTN_L1 = ecodes.BTN_TL
BTN_R1 = ecodes.BTN_TR
BTN_SELECT = ecodes.BTN_SELECT
BTN_START = ecodes.BTN_START


class InputMixin:
    """Entrées manettes : evdev multi-devices, accélération, actions."""
    # =========================================================================
    # ACCÉLÉRATION NAVIGATION
    # =========================================================================

    def _accel_delay(self, hold_ms):
        """Délai entre répétitions selon la durée de maintien (ms).

        La navigation accélère progressivement quand on maintient une direction :
        - 0-300ms   : 180ms (délai initial avant 1ère répétition)
        - 300-600ms : 120ms
        - 600ms-1s  : 70ms
        - 1-2s      : 40ms
        - >2s       : 25ms (très rapide)
        """
        if hold_ms < 300:
            return COOLDOWN_MS
        elif hold_ms < 600:
            return 120
        elif hold_ms < 1000:
            return 70
        elif hold_ms < 2000:
            return 40
        else:
            return 25

    def _try_move_cursor(self, direction):
        """Tente de déplacer le curseur dans la direction donnée.

        Retourne True si le curseur a été déplacé avec succès.
        """
        moved = False
        if direction == "up":
            new = self._cursor - self._cols
            if new >= 0:
                self._cursor = new
                moved = True
        elif direction == "down":
            new = self._cursor + self._cols
            if new < len(self._buttons):
                self._cursor = new
                moved = True
        elif direction == "left":
            if self._cursor % self._cols > 0:
                self._cursor -= 1
                moved = True
        elif direction == "right":
            if (self._cursor % self._cols < self._cols - 1
                    and self._cursor + 1 < len(self._buttons)):
                self._cursor += 1
                moved = True
        if moved:
            self._update_card_focus()
            self._ensure_visible()
        return moved

    def _nav_step(self, direction):
        """Un pas de navigation D-pad selon la vue courante (grille ou guide)."""
        if self._stack.currentIndex() == 2:
            self._guide_step(direction)
        else:
            self._try_move_cursor(direction)

    # =========================================================================
    # MANETTES DE JEU (toutes les manettes pilotent l'application)
    # =========================================================================

    @staticmethod
    def _normalize_axis(value, vmin, vmax):
        """Normalise un axe brut vers [-1.0, 1.0] selon la plage réelle du
        device (0.0 au repos). Les pads virtuels ds2xbox sont en
        -32768..32767 avec repos à 0 : sans normalisation par device, leur
        repos serait lu comme -1.008 (collé haut/gauche, volume qui monte
        en boucle)."""
        if vmax <= vmin:
            return 0.0
        norm = 2.0 * (value - vmin) / (vmax - vmin) - 1.0
        return max(-1.0, min(1.0, norm))

    def _find_gamepads(self):
        """Détecte les manettes connectées (toutes, pas une seule).

        Re-scan incrémental : ouvre uniquement les nouveaux devices, garde
        les déjà suivis (leurs états hat/boutons persistent)."""
        try:
            paths = evdev.list_devices()
        except (FileNotFoundError, PermissionError, OSError):
            paths = []
        for path in paths:
            if path in self._gamepads:
                continue
            try:
                device = evdev.InputDevice(path)
                caps = device.capabilities()
            except (OSError, IOError):
                continue
            if ecodes.EV_KEY not in caps:
                device.close()
                continue
            keys = caps[ecodes.EV_KEY]
            if ecodes.BTN_A not in keys and ecodes.BTN_SELECT not in keys:
                device.close()
                continue
            state = {
                "hat_x": 0, "hat_y": 0, "phat_x": 0, "phat_y": 0,
                "axis_x": 0.0, "axis_y": 0.0,
                "xr": (0, 255), "yr": (0, 255),
                "cross": False, "circle": False,
                "l1": False, "r1": False, "select": False, "start": False,
            }
            # Plages réelles des sticks pour la normalisation
            try:
                ax = device.absinfo(ecodes.ABS_X)
                ay = device.absinfo(ecodes.ABS_Y)
                state["xr"] = (ax.min, ax.max)
                state["yr"] = (ay.min, ay.max)
            except (OSError, IOError, KeyError, IndexError, AttributeError):
                pass
            self._gamepads[path] = device
            self._pad_states[path] = state
            print(f"[INFO] Manette ajoutée : {device.name} ({path})",
                  file=sys.stderr)
        self._update_osd()

    def _drop_gamepad(self, path):
        """Retire une manette déconnectée (fermeture propre)."""
        device = self._gamepads.pop(path, None)
        self._pad_states.pop(path, None)
        if device:
            try:
                device.close()
            except (OSError, IOError):
                pass
        self._update_osd()

    @staticmethod
    def _device_direction(state):
        """Direction d'un device : hat prioritaire sur stick, priorité
        up > down > left > right (sémantique historique mono-pad)."""
        if state["hat_y"] == -1 or state["axis_y"] < -0.5:
            return "up"
        if state["hat_y"] == 1 or state["axis_y"] > 0.5:
            return "down"
        if state["hat_x"] == -1 or state["axis_x"] < -0.5:
            return "left"
        if state["hat_x"] == 1 or state["axis_x"] > 0.5:
            return "right"
        return None

    def _merged_direction(self):
        """Direction courante toutes manettes confondues.

        Même ordre d'évaluation qu'en mono-pad (up > down > left > right),
        étendu à tous les devices."""
        for direction in ("up", "down", "left", "right"):
            for state in self._pad_states.values():
                if self._device_direction(state) == direction:
                    return direction
        return None

    def _update_osd(self):
        """Met à jour texte et visibilité de la barre d'aide manette."""
        connected = bool(self._gamepads)
        self._had_gamepad = connected
        idx = self._stack.currentIndex()
        texts = {
            0: "[A] Lancer    [B] Quitter    [Start] Guide TV    [Select] Plein écran    "
               "[L1/R1] Pays    [D-Pad] Naviguer",
            2: "[A] Regarder    [B] Retour    [Start] Chaînes    [Select] Plein écran    "
               "[L1/R1] Pays    [D-Pad] Naviguer le guide",
        }
        self._osd_label.setText(texts.get(idx, ""))
        self._osd_bar.setVisible(connected and idx != 1)

    def _poll_gamepad(self):
        """Boucle principale de lecture manettes (appelée toutes les ~16ms).

        TOUTES les manettes connectées pilotent l'application : état tenu
        par device, détection de fronts par device. L'accélération
        fonctionne même sans nouveaux événements : l'état du stick/hat
        est mémorisé entre les appels. Re-scan incrémental toutes les
        2 s pour suivre les branchements à chaud.

        Les manettes sont ignorées quand tvqt n'est pas la fenêtre active
        (focus Wayland géré par changeEvent / ActivationChange).
        """
        if not self._window_active:
            return

        now = time.time() * 1000

        # Re-scan incrémental toutes les 2 s (hotplug, reconnexion BT)
        if now - self._last_scan >= 2000:
            self._last_scan = now
            self._find_gamepads()

        if not self._gamepads:
            return

        watching = self._stack.currentIndex() == 1

        # Lecture des événements de toutes les manettes
        try:
            ready, _, _ = select.select(list(self._gamepads.values()), [], [], 0)
        except (OSError, IOError, ValueError):
            # fd invalide (device disparu) : repartir d'un état propre
            for path in list(self._gamepads):
                self._drop_gamepad(path)
            return

        actions = []  # boutons pressés (fronts détectés) ce poll
        for device in ready:
            state = self._pad_states.get(device.path)
            if state is None:
                continue
            try:
                events = device.read()
            except (OSError, IOError, BlockingIOError):
                self._drop_gamepad(device.path)
                continue

            for ev in events:
                if ev.type == ecodes.EV_ABS:
                    if ev.code == ecodes.ABS_HAT0X:
                        state["hat_x"] = ev.value
                    elif ev.code == ecodes.ABS_HAT0Y:
                        state["hat_y"] = ev.value
                    elif ev.code == ecodes.ABS_X:
                        state["axis_x"] = self._normalize_axis(ev.value, *state["xr"])
                    elif ev.code == ecodes.ABS_Y:
                        state["axis_y"] = self._normalize_axis(ev.value, *state["yr"])
                elif ev.type == ecodes.EV_KEY:
                    pressed = ev.value == 1
                    if ev.code == BTN_CROSS:
                        if pressed and not state["cross"]:
                            actions.append("cross")
                        state["cross"] = pressed
                    elif ev.code == BTN_CIRCLE:
                        if pressed and not state["circle"]:
                            actions.append("circle")
                        state["circle"] = pressed
                    elif ev.code == BTN_L1:
                        if pressed and not state["l1"]:
                            actions.append("l1")
                        state["l1"] = pressed
                    elif ev.code == BTN_R1:
                        if pressed and not state["r1"]:
                            actions.append("r1")
                        state["r1"] = pressed
                    elif ev.code == BTN_SELECT:
                        if pressed and not state["select"]:
                            actions.append("select")
                        state["select"] = pressed
                    elif ev.code == BTN_START:
                        if pressed and not state["start"]:
                            actions.append("start")
                        state["start"] = pressed

        # --- Direction actuelle (toutes manettes) ---
        new_dir = self._merged_direction()

        # --- Mode grille/guide : accélération progressive au maintien ---
        if not watching:
            if new_dir is None:
                self._hold_dir = None
            elif new_dir == self._hold_dir:
                hold_ms = now - self._hold_start
                if now - self._last_repeat >= self._accel_delay(hold_ms):
                    self._last_repeat = now
                    self._nav_step(new_dir)
            else:
                self._hold_dir = new_dir
                self._hold_start = now
                self._last_repeat = now
                self._nav_step(new_dir)

        # --- Boutons (fronts par device, tir unique, pas d'accélération) ---
        if now - self._last_input >= COOLDOWN_MS:
            if "select" in actions:
                self._last_input = now
                self._toggle_fullscreen()

            elif "cross" in actions:
                self._last_input = now
                if watching:
                    self._pause_toggle()
                elif self._stack.currentIndex() == 2:
                    self._guide_play_current()
                elif self._buttons and self._cursor < len(self._filtered):
                    _, (ch_id, label, _) = self._filtered[self._cursor]
                    self._play(f"{BASE}/{ch_id}{SUFFIX}/{ch_id}.m3u8", label)
                self._hold_dir = None

            elif "circle" in actions:
                self._last_input = now
                if watching:
                    self._stop_video()
                elif self._stack.currentIndex() == 2:
                    self._show_view(0)
                else:
                    self.close()
                return

            elif "start" in actions:
                self._last_input = now
                self._toggle_guide()
                self._hold_dir = None

            elif "l1" in actions:
                self._last_input = now
                if not watching:
                    self._country_idx = (self._country_idx - 1) % (len(self._countries_current) + 1)
                    self._select_country(self._country_idx)
                    self._scroll_to_top()
                self._hold_dir = None

            elif "r1" in actions:
                self._last_input = now
                if not watching:
                    self._country_idx = (self._country_idx + 1) % (len(self._countries_current) + 1)
                    self._select_country(self._country_idx)
                    self._scroll_to_top()
                self._hold_dir = None

            # --- Mode lecture : hat = front par device (un cran = une
            #     action), stick = répétition au rythme du cooldown.
            #     Seul le device qui PRODUIT la direction est évalué :
            #     le garde phat d'une manette au repos (0) ne doit pas
            #     faire répéter la direction tenue par une autre. ---
            elif watching and new_dir is not None:
                for state in self._pad_states.values():
                    if self._device_direction(state) != new_dir:
                        continue
                    if new_dir == "up" and state["phat_y"] != -1:
                        self._last_input = now
                        self._volume_up()
                    elif new_dir == "down" and state["phat_y"] != 1:
                        self._last_input = now
                        self._volume_down()
                    elif new_dir == "left" and state["phat_x"] != -1:
                        self._last_input = now
                        self._seek(-10)
                    elif new_dir == "right" and state["phat_x"] != 1:
                        self._last_input = now
                        self._seek(10)
                    break

        # Fronts hat mémorisés par device pour le mode lecture
        for state in self._pad_states.values():
            state["phat_x"] = state["hat_x"]
            state["phat_y"] = state["hat_y"]
