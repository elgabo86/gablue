"""Formatage lisible : tailles, dates, chemins."""

import shutil
import subprocess
import time


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
