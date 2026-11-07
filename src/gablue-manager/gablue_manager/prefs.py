"""Préférences de Gablue Manager (petit JSON, écriture atomique).

Une seule préférence pour l'instant : l'icône de barre d'état. Elle est lue
au démarrage de session par `gablue-manager --tray` (autostart) et par la
fenêtre principale. Une préférence absente vaut « activée » : c'est le défaut
de l'image Gablue, où le tray est lancé à chaque ouverture de session.
"""

import json
import os
import tempfile

from .constants import CONFIG_DIR

PREFS_FILE = CONFIG_DIR / "preferences.json"

DEFAULTS = {"tray": True}

def load_prefs() -> dict:
    """Préférences relues à chaque appel ; absent ou illisible = défauts."""
    try:
        data = json.loads(PREFS_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return dict(DEFAULTS)
    if not isinstance(data, dict):
        return dict(DEFAULTS)
    values = dict(DEFAULTS)
    for key in DEFAULTS:
        if isinstance(data.get(key), bool):
            values[key] = data[key]
    return values

def save_prefs(values: dict) -> None:
    """Écrit les préférences connues (les clés inconnues sont ignorées)."""
    merged = dict(DEFAULTS)
    merged.update({key: value for key, value in values.items() if key in DEFAULTS})
    CONFIG_DIR.mkdir(parents=True, exist_ok=True)
    try:
        os.chmod(CONFIG_DIR, 0o700)
    except OSError:
        pass
    handle, temporary = tempfile.mkstemp(
        dir=CONFIG_DIR, prefix=".preferences-", suffix=".json",
    )
    try:
        with os.fdopen(handle, "w", encoding="utf-8") as stream:
            json.dump(merged, stream, indent=2, sort_keys=True)
            stream.write("\n")
        os.chmod(temporary, 0o600)
        os.replace(temporary, PREFS_FILE)
    except BaseException:
        try:
            os.unlink(temporary)
        except OSError:
            pass
        raise

def tray_enabled() -> bool:
    """L'icône de barre d'état doit-elle tourner dans cette session ?"""
    return bool(load_prefs()["tray"])

def set_tray_enabled(enabled: bool) -> None:
    values = load_prefs()
    values["tray"] = bool(enabled)
    save_prefs(values)
