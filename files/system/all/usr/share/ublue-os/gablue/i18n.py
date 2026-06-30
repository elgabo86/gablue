#!/usr/bin/python3
"""Infrastructure d'internationalisation Gablue
Usage: from gablue.i18n import _ ; print(_("English text"))
Détection automatique via $LANG, fallback = anglais
"""

import os

_TR = {}

def _lang():
    return "fr" if os.environ.get("LANG", "en").startswith("fr") else "en"

def _(text):
    if _lang() == "fr" and text in _TR:
        return _TR[text]
    return text

# Traductions françaises
_TR["Disconnect Bluetooth"] = "Déconnecter Bluetooth"
_TR["Suspend"] = "Mettre en veille"
_TR["Shut down"] = "Éteindre"
_TR["Reboot"] = "Redémarrer"
_TR["Confirm"] = "Confirmer"
_TR["Yes"] = "Oui"
_TR["No"] = "Non"
