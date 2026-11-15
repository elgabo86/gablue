"""Formatage lisible : tailles, dates, chemins."""

import shutil
import subprocess
import time
from pathlib import Path

from .constants import COMPOSEFS_FIX_SO


def human_size(size: int | float) -> str:
    value = float(size)
    for unit in ("o", "Kio", "Mio", "Gio", "Tio"):
        if value < 1024 or unit == "Tio":
            if unit == "o":
                return f"{int(value)} o"
            return f"{value:.1f} {unit}".replace(".0 ", " ")
        value /= 1024
    return f"{value:.1f} Tio"


def human_eta(seconds: float) -> str:
    seconds = int(max(0, seconds))
    if seconds < 60:
        return f"{seconds} s"
    minutes, rest = divmod(seconds, 60)
    if minutes < 60:
        return f"{minutes} min"
    hours, minutes = divmod(minutes, 60)
    return f"{hours} h {minutes:02d}"


def notify(title: str, body: str) -> None:
    """Notification de bureau : silencieuse si `notify-send` n'est pas là."""
    if not shutil.which("notify-send"):
        return
    try:
        subprocess.Popen(
            ["notify-send", "--app-name=Gablue Share", "--icon=gablue-share", title, body],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        pass


def human_date(timestamp: int | float) -> str:
    if not timestamp:
        return "—"
    moment = time.localtime(timestamp)
    now = time.localtime()
    if (moment.tm_year, moment.tm_yday) == (now.tm_year, now.tm_yday):
        return time.strftime("%H:%M", moment)
    if moment.tm_year == now.tm_year:
        return time.strftime("%d %b", moment)
    return time.strftime("%d %b %Y", moment)


def human_speed(bytes_per_second: float) -> str:
    return f"{human_size(bytes_per_second)}/s"


def human_duration(seconds: float) -> str:
    seconds = max(0, int(seconds))
    if seconds < 60:
        return f"{seconds} s"
    minutes, rest = divmod(seconds, 60)
    if minutes < 60:
        return f"{minutes} min"
    hours, minutes = divmod(minutes, 60)
    return f"{hours} h {minutes:02d}"


def dolphin_command(target: str, select: bool = False) -> list[str] | None:
    """Commande Dolphin avec le hook composefs de l'image, ou `None` sans Dolphin.

    `select` : Dolphin ouvre le **dossier parent** et y met l'élément en
    évidence — c'est ce qu'on veut après un téléchargement (le fichier arrive
    dans un dossier qui peut en contenir des centaines). Le hook `LD_PRELOAD`
    est réinjecté : un Dolphin lancé depuis l'application ne passe pas par le
    `.desktop` patché et afficherait 0 o d'espace libre sur l'overlay composefs
    de `/` (même correctif que gablue-manager, `dolphin_command`).
    """
    binary = shutil.which("dolphin")
    if binary is None:
        return None
    arguments = [binary]
    arguments += ["--select", target] if select else [target]
    if COMPOSEFS_FIX_SO.exists():
        return ["env", f"LD_PRELOAD={COMPOSEFS_FIX_SO}", *arguments]
    return arguments


def reveal(path: Path) -> bool:
    """Montre `path` dans Dolphin : le dossier ouvert, le fichier sélectionné.

    Faux quand Dolphin n'est pas installé — l'appelant décide alors du repli
    (`xdg-open`, qui passe par le `.desktop` patché).
    """
    path = Path(path)
    folder = path if path.is_dir() else path.parent
    command = None
    if path.exists():
        command = dolphin_command(str(path), select=True)
    if command is None and folder.is_dir():
        command = dolphin_command(str(folder))
    if command is None:
        return False
    try:
        subprocess.Popen(
            command,
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            start_new_session=True,
        )
    except OSError:
        return False
    return True
