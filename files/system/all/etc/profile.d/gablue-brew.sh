# shellcheck shell=bash
# Gablue : brew visible aussi dans les shells non-interactifs
# Le /etc/profile.d/brew.sh uBlue est réservé aux shells interactifs
# ($- == *i*) — la session Plasma (sourcing non-interactif au login),
# les scripts `bash -lc` et OpenCode (lancé en GUI, tool shells en
# `bash -c` qui ne sourcent rien et héritent) n'ont donc jamais brew
# dans le PATH. Ce fichier comble le trou en appliquant la même
# logique sans la garde interactive.
# Ajout en FIN de PATH : les binaires système restent prioritaires
# (le python/git de brew ne shadow jamais /usr/bin/python3, etc.)

if [ -z "${HOMEBREW_PREFIX:-}" ] && [ -d /home/linuxbrew/.linuxbrew ]; then
    case ":${PATH}:" in
        *":/home/linuxbrew/.linuxbrew/bin:"*) ;;
        *)
            HOMEBREW_PREFIX="/home/linuxbrew/.linuxbrew"
            export HOMEBREW_PREFIX
            HOMEBREW_CELLAR="${HOMEBREW_PREFIX}/Cellar"
            export HOMEBREW_CELLAR
            HOMEBREW_REPOSITORY="${HOMEBREW_PREFIX}"
            export HOMEBREW_REPOSITORY
            MANPATH="${HOMEBREW_PREFIX}/share/man${MANPATH+:$MANPATH}"
            export MANPATH
            INFOPATH="${HOMEBREW_PREFIX}/share/info${INFOPATH+:$INFOPATH}"
            export INFOPATH
            export PATH="${PATH}:${HOMEBREW_PREFIX}/bin:${HOMEBREW_PREFIX}/sbin"
            ;;
    esac
fi
