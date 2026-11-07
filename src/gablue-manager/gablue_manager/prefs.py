"""Préférences de Gablue Manager (petit JSON, écriture atomique).

Deux familles de réglages, toutes deux propres à la session de l'utilisateur :

- l'**icône de barre d'état** (`tray`), lue au démarrage de session par
  `gablue-manager --tray` (autostart) et par la fenêtre principale ;
- la **vérification des mises à jour** : interrupteur général (`check_auto`),
  domaines surveillés (`check_image`, `check_flatpak`, `check_brew`,
  `check_gwine`, `check_apps`) et fréquence (`check_interval`, en minutes).

Une préférence absente ou illisible vaut la valeur par défaut : la barre d'état
et les vérifications sont actives, comme le veut l'image Gablue.
"""

import json
import os
import tempfile

from .constants import CONFIG_DIR

PREFS_FILE = CONFIG_DIR / "preferences.json"

DEFAULTS = {
    "tray": True,
    "check_auto": True,
    "check_image": True,
    "check_flatpak": True,
    "check_brew": True,
    "check_gwine": True,
    "check_apps": True,
    "check_interval": 120,          # minutes, 2 h
}

# Bornes des préférences numériques : 5 min (pas de martèlement réseau) à
# 24 h (une fois par jour).
INTERVAL_RANGE = (5, 1440)

CHECK_DOMAINS = ("image", "flatpak", "brew", "gwine", "apps")

def load_prefs() -> dict:
    """Préférences relues à chaque appel ; absent ou illisible = défauts."""
    try:
        data = json.loads(PREFS_FILE.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return dict(DEFAULTS)
    if not isinstance(data, dict):
        return dict(DEFAULTS)
    values = dict(DEFAULTS)
    for key, default in DEFAULTS.items():
        value = data.get(key)
        if isinstance(default, bool):
            if isinstance(value, bool):
                values[key] = value
        elif isinstance(value, int) and not isinstance(value, bool):
            low, high = INTERVAL_RANGE
            values[key] = min(high, max(low, value))
    return values

def save_prefs(values: dict) -> None:
    """Écrit les préférences connues (les clés inconnues sont ignorées)."""
    merged = dict(DEFAULTS)
    merged.update({key: value for key, value in values.items()
                   if key in DEFAULTS})
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

def check_domains(values: dict | None = None) -> dict[str, bool]:
    """Domaines surveillés, indépendamment de l'interrupteur général.

    Sert à « Vérifier les mises à jour » : un clic explicite reste utile même
    quand la surveillance automatique est coupée.
    """
    values = load_prefs() if values is None else values
    return {domain: bool(values.get(f"check_{domain}", True))
            for domain in CHECK_DOMAINS}

def auto_check_domains(values: dict | None = None) -> dict[str, bool]:
    """Domaines à vérifier tout seuls ; `check_auto` coupe tout."""
    values = load_prefs() if values is None else values
    if not values.get("check_auto", True):
        return {domain: False for domain in CHECK_DOMAINS}
    return check_domains(values)

def check_interval_seconds(values: dict | None = None) -> int:
    values = load_prefs() if values is None else values
    minutes = values.get("check_interval", DEFAULTS["check_interval"])
    if not isinstance(minutes, int) or isinstance(minutes, bool):
        minutes = DEFAULTS["check_interval"]
    low, high = INTERVAL_RANGE
    return min(high, max(low, minutes)) * 60

def checks_enabled(values: dict | None = None) -> bool:
    """Au moins un domaine est-il surveillé automatiquement ?"""
    return any(auto_check_domains(values).values())
