#!/usr/bin/bash
# session-start.sh — (re)lance l'overlay manette dans la session Plasma qui
# démarre.
#
# Pourquoi un redémarrage et pas un simple `start` : `gablue-pad.service` vit
# dans le gestionnaire UTILISATEUR, partagé par toutes les sessions du compte.
# En revenant de la session console vers Plasma par le login, l'unité tourne
# encore — avec l'environnement de la session console (`WAYLAND_DISPLAY=
# gablue-gaming`) — et `WantedBy=graphical-session.target` ne redémarre JAMAIS
# une unité déjà active. Résultat constaté le 27/11/2026 sur gen1 : Home
# ouvrait un panneau dessiné sur un compositeur mort (rien à l'écran), et
# `kscreen-doctor` lancé par le script VRR héritait du vieil environnement,
# tombait sur le greffon Qt « xcb » absent et laissait une notification de
# crash (drkonqi).
#
# L'aller desktop → console est déjà couvert par `gablue-gaming-session`
# (`restart` aussi) ; c'est le retour qui manquait.
set -u

# `gablue-pad off` (masque utilisateur) : ne rien relancer.
systemctl --user is-enabled --quiet gablue-pad.service || exit 0

# Le gestionnaire utilisateur ne connaît le nouveau compositeur qu'une fois
# `WAYLAND_DISPLAY` importé par Plasma : attendre que le socket existe, borné à
# 5 s (au-delà, on redémarre quand même — l'unité se relancera d'elle-même au
# prochain login).
RT=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
for _ in $(seq 1 50); do
    wd=$(systemctl --user show-environment 2>/dev/null | sed -n 's/^WAYLAND_DISPLAY=//p')
    [[ -n "$wd" && -S "$RT/$wd" ]] && break
    sleep 0.1
done

systemctl --user restart gablue-pad.service 2>/dev/null || true
