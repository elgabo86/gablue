#!/usr/bin/bash
# Infrastructure d'internationalisation Gablue
# Usage: source ce fichier puis utiliser $(gablue_tr "texte anglais")
# Détection automatique via $LANG, fallback = anglais

declare -A GABLUE_TR

# Langue détectée : fr si $LANG commence par "fr", sinon en
gablue_lang() {
    if [[ "${LANG:-en}" == fr* ]]; then
        echo "fr"
    else
        echo "en"
    fi
}

# Fonction de traduction : retourne la trad si dispo, sinon l'original
gablue_tr() {
    local key="$1"
    if [[ "$(gablue_lang)" == "fr" ]] && [[ -n "${GABLUE_TR[$key]+x}" ]]; then
        echo "${GABLUE_TR[$key]}"
    else
        echo "$key"
    fi
}

# Traductions françaises
# fmt: GABLUE_TR["English string"]="Chaîne française"
