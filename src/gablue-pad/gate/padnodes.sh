#!/usr/bin/bash
# padnodes.sh - inventaire des nodes d'une manette, PAR PÉRIPHÉRIQUE PHYSIQUE.
#
#   ./padnodes.sh              toutes les manettes détectées
#   ./padnodes.sh <motif>      seulement celles dont le nom contient <motif>
#
# Sortie (lisible par script) :
#   PAD <nom> uniq=<adresse> transport=<usb|bt> pilote=<hid-playstation|xpad|…>
#   NODE /dev/input/jsN         joydev : le trou que EVIOCGRAB ne couvre pas
#   NODE /dev/hidrawN           HID brut (SDL hidapi, libusb, Steam)
#   USBFS /dev/bus/usb/BBB/DDD  accès USB direct — USB seulement, optionnel
#   END
#
# Pourquoi grouper par périphérique PHYSIQUE et pas par périphérique HID :
#   - une manette peut exposer plusieurs interfaces HID (Xbox en Bluetooth),
#     donc plusieurs `hidraw` et plusieurs `js` ;
#   - DualSense : 2 `js` (pad + capteurs) ; Switch Pro : 2 `js` (pad + IMU) ;
#   - xpad (Xbox en USB) est un pilote USB : aucun `hidraw`, la fuite n'est
#     que joydev + evdev.
# Une manette est reconnue par la présence d'un handler joydev (jsN) : les
# claviers et souris n'en ont jamais, quel que soit le pilote.
set -uo pipefail

HERE="$(dirname "$(readlink -f "$0")")"
MOTIF=${1:-}

# dossier du device USB parent (contient busnum/devnum), vide si Bluetooth
usb_dir() {
    local d=$1
    while [[ "$d" != "/" && "$d" != "/sys" ]]; do
        if [[ -f "$d/busnum" && -f "$d/devnum" ]]; then
            printf '%s\n' "$d"; return 0
        fi
        d=$(dirname "$d")
    done
    return 1
}

# nom du pilote qui a lié le périphérique HID (hid-playstation, xpad, hid-generic…)
pilote() {
    local d=$1
    if [[ -e "$d/driver" ]]; then
        basename "$(readlink -f "$d/driver")"
    else
        printf '?\n'
    fi
}

declare -A NOM UNIQ HIDS JS PIL HANDLERS ORDRE
declare -a ORDRE_LIST

while IFS=$'\t' read -r hid name uniq handlers; do
    [[ -z "$hid" ]] && continue

    # identité physique : adresse BT pour uhid, device USB sinon
    if [[ "$hid" == /sys/devices/virtual/misc/uhid/* ]]; then
        phys="bt:${uniq:-$hid}"
    else
        if u=$(usb_dir "$hid" 2>/dev/null); then
            phys="$u"
        else
            phys="$hid"
        fi
    fi

    if [[ -z "${ORDRE[$phys]+x}" ]]; then
        ORDRE[$phys]=1
        ORDRE_LIST+=("$phys")
        UNIQ[$phys]="$uniq"
        PIL[$phys]="$(pilote "$hid")"
        NOM[$phys]="$name"
        HIDS[$phys]="$hid"
        HANDLERS[$phys]="$handlers"
        JS[$phys]=0
    else
        [[ " ${HIDS[$phys]} " == *" $hid "* ]] || HIDS[$phys]="${HIDS[$phys]} $hid"
        HANDLERS[$phys]="${HANDLERS[$phys]} $handlers"
    fi

    if [[ " $handlers " == *" js"* ]]; then
        JS[$phys]=1
        # nom principal : entrée avec joydev, sans suffixe annexe
        if [[ "$name" != *Motion* && "$name" != *Touchpad* && "$name" != *IMU* \
              && "$name" != *Keyboard* && "$name" != *Mouse* ]]; then
            NOM[$phys]="$name"
        fi
    fi
done < <(awk 'BEGIN{RS="";FS="\n"}
{
    name=""; uniq=""; sysfs=""; handlers=""
    for (i=1;i<=NF;i++) {
        if ($i ~ /^N: Name=/)     { name=$i;     sub(/^N: Name="/,"",name); sub(/"$/,"",name) }
        if ($i ~ /^U: Uniq=/)     { uniq=$i;     sub(/^U: Uniq=/,"",uniq) }
        if ($i ~ /^S: Sysfs=/)    { sysfs=$i;    sub(/^S: Sysfs=/,"",sysfs) }
        if ($i ~ /^H: Handlers=/) { handlers=$i; sub(/^H: Handlers=/,"",handlers) }
    }
    if (sysfs == "") next
    hid = sysfs
    sub(/\/input\/input[0-9]+$/, "", hid)
    print "/sys" hid "\t" name "\t" uniq "\t" handlers
}' /proc/bus/input/devices)

for phys in "${ORDRE_LIST[@]}"; do
    [[ ${JS[$phys]} -eq 1 ]] || continue
    nom="${NOM[$phys]}"
    [[ -n "$MOTIF" && "$nom" != *"$MOTIF"* ]] && continue

    transport=bt
    [[ -f "$phys/busnum" ]] && transport=usb

    echo "PAD $nom uniq=${UNIQ[$phys]} transport=$transport pilote=${PIL[$phys]}"

    for h in ${HANDLERS[$phys]}; do
        [[ "$h" == js* ]] && echo "NODE /dev/input/$h"
    done

    # tous les hidraw des périphériques HID de ce périphérique physique
    for hd in ${HIDS[$phys]}; do
        for n in /sys/class/hidraw/hidraw*; do
            [[ "$(readlink -f "$n/device")" == "$hd" ]] && echo "NODE /dev/$(basename "$n")"
        done
    done

    if [[ $transport == usb ]]; then
        printf 'USBFS /dev/bus/usb/%03d/%03d\n' "$(cat "$phys/busnum")" "$(cat "$phys/devnum")"
    fi
    echo "END"
done
