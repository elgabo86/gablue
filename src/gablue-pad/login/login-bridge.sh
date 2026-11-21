#!/usr/bin/bash
# Écran de connexion Gablue : thème sombre et pont manette.
#
# Lancé dans la session du greeter (utilisateur « plasmalogin ») par deux
# unités utilisateur, toutes deux rattachées à plasma-login-wayland.target :
#
#   login-bridge.sh theme   gablue-login-theme.service, Type=oneshot, ordonné
#                           AVANT plasma-login-kwin_wayland.service : le
#                           `kdeglobals` du greeter doit exister quand KWin lit
#                           ses couleurs, sinon l'écran reste clair un tour
#   login-bridge.sh pad     gablue-login-greeter.service, après KWin : clavier
#                           visuel + pointeur manette
#   login-bridge.sh         les deux (banc, débogage)
#
# AUCUN privilège : le script n'écrit que dans le dossier de configuration du
# greeter, ce qui évite tout franchissement de domaine SELinux.
#
# Réglages lus dans /etc/gablue/login.conf (écrit par Gablue Manager via
# /usr/libexec/gablue-login) :
#   pad=on|off    pont manette (pointeur + clavier visuel) — défaut on
#   dark=on|off   écran de connexion sombre            — défaut off
#
# Le clavier visuel ne s'ouvre QU'au premier mouvement d'une manette (stick
# hors zone morte, D-pad ou appui de touche) : une manette simplement branchée
# ne l'affiche pas. Il se referme après 10 s sans mouvement (ou quand la
# manette part) et le geste suivant le rouvre ; c'est gpad-login — le seul à
# voir les événements — qui prévient, par SIGUSR1.
set -u

mode=${1:-all}
case $mode in
theme|pad|all) ;;
*)
    echo "usage : $0 [theme|pad]" >&2
    exit 2
    ;;
esac

CONF=/etc/gablue/login.conf
DARK_SRC=/usr/share/gablue/plasmalogin-kdeglobals-dark
CONFDIR=$HOME/.config
KG=$CONFDIR/kdeglobals
MARK=$CONFDIR/.gablue-login-dark
BAK=$CONFDIR/.gablue-login-kdeglobals.orig

log() { echo "$(date -Is) $*"; }

# Un seul pont par session : deux exemplaires (unité héritée d'un test + celle
# de l'image) donneraient deux pointeurs virtuels, donc un mouvement doublé.
if [ "$mode" != theme ]; then
    LOCK=${XDG_RUNTIME_DIR:-/tmp}/gablue-login-bridge.lock
    if exec 9>"$LOCK"; then
        flock -n 9 || { log "un autre pont tourne déjà — abandon"; exit 0; }
    else
        log "verrou $LOCK impossible — continuation sans verrou"
    fi
fi

# Réglage : clé absente ou vide -> défaut
conf_get() {
    local v
    v=$(sed -n "s/^$1=//p" "$CONF" 2>/dev/null | tail -1)
    [ -n "$v" ] && printf '%s' "$v" || printf '%s' "$2"
}

# Garde : uniquement la session du greeter. Comparaison par UID (le nom
# s'écrit « plasmalogin » ; il a déjà été mal orthographié une fois).
greeter_uid=$(stat -c %u /var/lib/plasmalogin 2>/dev/null || echo "")
if [ -z "$greeter_uid" ] || [ "$(id -u)" != "$greeter_uid" ]; then
    log "uid $(id -u) != greeter (${greeter_uid:-?}) — abandon"
    exit 0
fi

# --- Thème de l'écran de connexion -----------------------------------------
theme_apply() {
    if [ "$(conf_get dark off)" = on ]; then
        if [ ! -r "$DARK_SRC" ]; then
            log "thème : $DARK_SRC absent — laissé tel quel"
            return
        fi
        cmp -s "$DARK_SRC" "$KG" 2>/dev/null && return
        mkdir -p "$CONFDIR"
        # Ce que le greeter avait avant nous (personnalisation faite à la
        # main) est sauvegardé UNE fois : le retour au clair doit rendre son
        # fichier au greeter, pas le supprimer.
        if [ ! -f "$MARK" ] && [ -f "$KG" ]; then
            cp -a "$KG" "$BAK" 2>/dev/null || true
        fi
        # Écriture EN PLACE (pas de rm puis création) : conserve le label
        # SELinux du fichier. Un `kdeglobals` posé à la main par root n'est
        # pas réécrivable par le greeter : on le dit et on laisse le fichier
        # tel quel plutôt que de réessayer à chaque démarrage.
        if [ ! -e "$KG" ] || [ -w "$KG" ]; then
            cat "$DARK_SRC" >"$KG"
            : >"$MARK"
            log "thème : sombre appliqué"
        else
            log "thème : $KG non inscriptible (propriétaire ?) — laissé tel quel"
        fi
    else
        [ -f "$MARK" ] || return
        if [ -f "$BAK" ]; then
            # La sauvegarde ne disparaît qu'une fois restituée : un échec doit
            # pouvoir être retenté au démarrage suivant.
            if [ -w "$KG" ]; then
                cat "$BAK" >"$KG"
                rm -f "$BAK" "$MARK"
                log "thème : ancien fichier restitué (clair)"
            else
                log "thème : restitution de $KG impossible — sauvegarde conservée"
            fi
        elif rm -f "$KG" 2>/dev/null; then
            rm -f "$MARK"
            log "thème : retour au clair"
        else
            log "thème : $KG non supprimable — laissé tel quel"
        fi
    fi
}

[ "$mode" = pad ] || theme_apply
[ "$mode" = theme ] && exit 0

# --- Pont manette ----------------------------------------------------------
# KWin du greeter : présent avant nous (After=), on insiste un peu.
for _ in $(seq 1 100); do
    busctl --user status org.kde.KWin >/dev/null 2>&1 && break
    sleep 0.1
done

vkm() { busctl --user set-property org.kde.KWin /VirtualKeyboard \
            org.kde.kwin.VirtualKeyboard mode i "$1"; }
# Fermeture franche : Never(0) puis Touch-and-Tablet(1) ; sans le 0, un
# panneau déjà mappé reste visible (même mécanique que l'overlay manette).
hide_osk() { vkm 0; vkm 1; }
show_osk() { vkm 2 && busctl --user call org.kde.KWin /VirtualKeyboard \
                            org.kde.kwin.VirtualKeyboard forceActivate; }
pad_present() {
    for d in /dev/input/event*; do
        udevadm info -q property -n "$d" 2>/dev/null \
            | grep -q '^ID_INPUT_JOYSTICK=1' && return 0
    done
    return 1
}

hide_osk
if [ "$(conf_get pad on)" != on ]; then
    log "pont manette désactivé (pad=off) — clavier visuel fermé"
    exit 0
fi

# Notification « la manette est utilisée » : gpad-login envoie SIGUSR1 à chaque
# activité (appui, stick hors zone morte, pointeur qui avance). Il ne le fait
# que si GPAD_LOGIN_NOTIFY_PID vaut bien le PID de son parent : lancé à la
# main, il ne signale personne. Ce signal ouvre le clavier et, plus tard,
# repousse sa fermeture pour inactivité.
moved=0
trap 'moved=1' USR1
export GPAD_LOGIN_NOTIFY_PID=$$

gpid=
cleanup() {
    [ -n "$gpid" ] && kill "$gpid" 2>/dev/null
}
trap cleanup EXIT
trap 'exit 0' TERM INT

log "pont manette actif — clavier visuel fermé jusqu'au premier mouvement de la manette"

state=hidden
last=0                 # $SECONDS du dernier mouvement vu (SECONDS = 1 s)
OSK_IDLE=10            # clavier refermé après 10 s sans mouvement
while :; do
    if pad_present; then
        # Le pointeur suit la manette dès qu'elle est là (il ne fait rien
        # tant qu'on n'y touche pas) ; c'est lui qui notifie le mouvement.
        if [ -n "$gpid" ] && ! kill -0 "$gpid" 2>/dev/null; then
            wait "$gpid" 2>/dev/null
            gpid=
        fi
        if [ -z "$gpid" ]; then
            /usr/bin/gpad-login &
            gpid=$!
            log "manette vue → pointeur manette (clavier visuel au premier mouvement)"
        fi

        # SIGUSR1 interrompt le sleep ci-dessous : le clavier s'ouvre aussitôt.
        # gpad-login renotifie à chaque activité (appui, stick, pointeur qui
        # avance), donc ce même signal rouvre un clavier refermé.
        if [ "$moved" = 1 ]; then
            moved=0
            last=$SECONDS
            if [ "$state" = hidden ]; then
                show_osk
                state=shown
                log "mouvement manette → clavier visuel affiché"
                log "  état : $(busctl --user get-property org.kde.KWin /VirtualKeyboard \
                    org.kde.kwin.VirtualKeyboard mode available active visible 2>&1 \
                    | tr '\n' ' ')"
            fi
        fi
    else
        # Un mouvement signalé juste avant le débranchement ne doit pas
        # ouvrir le clavier sur un écran sans manette.
        moved=0
        if [ -n "$gpid" ]; then
            kill "$gpid" 2>/dev/null
            wait "$gpid" 2>/dev/null
            gpid=
        fi
    fi

    if [ "$state" = shown ]; then
        if [ -z "$gpid" ]; then
            hide_osk
            state=hidden
            log "plus de manette → clavier visuel fermé"
        elif [ "$((SECONDS - last))" -ge "$OSK_IDLE" ]; then
            hide_osk
            state=hidden
            log "$OSK_IDLE s sans mouvement → clavier visuel fermé"
        fi
    fi
    sleep 1
done
