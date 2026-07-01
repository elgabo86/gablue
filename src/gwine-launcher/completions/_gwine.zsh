#compdef gwine
# Autocomplétion Zsh pour Gwine

gwine_commands() {
    local -a commands
    commands=(
        '--help:Afficher l\'aide'
        '-h:Afficher l\'aide'
        '--fix:Activer le mode fix manette'
        '--xbox:Émulation manettes Sony en Xbox 360 via ds2xbox'
        '--xbox-ds4:Émulation DualShock 4 uniquement en Xbox 360'
        '--xbox-dualsense:Émulation DualSense uniquement en Xbox 360'
        '--xbox-on:Activer le mode xbox par défaut pour tous les lancements'
        '--xbox-off:Désactiver le mode xbox par défaut'
        '--reset:Réinitialiser toutes les options par défaut'
        '--nofix:Ignorer le fichier .fix d'"'"'un jeu'
        '--exewgp:Mode interactif pour choisir l\'exécutable dans un WGP'
        '--init:Initialiser/réinitialiser le préfixe Wine'
        '--offline:Mode offline pour --init'
        '--update:Mettre à jour les composants'
        '--kdialog:Activer les dialogues graphiques kdialog'
        '--cmd:Ouvrir un terminal Wine ou exécuter une commande (cmd /C)'
        '--regedit:Lancer l\'éditeur de registre'
        '--reg:Installer un fichier de registre'
        '--winecfg:Lancer la configuration Wine'
        '--winetricks:Lancer winetricks'
        '--winetrick:Lancer winetricks'
        '--download-components:Télécharger tous les composants pour le mode offline'
        '--cachepack:Créer un pack du cache pour installation offline'
        '--nosandbox:Désactiver le sandboxing'
        '--joytest:Lancer le test de joypad (joy.cpl)'
        '--use-ln-mounts:Utiliser des liens symboliques au lieu de bind mounts'
        '--x11:Utiliser X11/XWayland comme mode d\'affichage'
        '--wayland:Utiliser Wayland natif comme mode d\'affichage'
        '--env:Passer une variable d\'environnement'
        '--args:Arguments à passer au jeu'
        '--gameid:ID du jeu'
        '--kill:Forcer l\'arrêt de tous les processus Gwine en cours'
        '--dir:Gérer les répertoires bindés dans le sandbox'
        '--wine:Configurer Wine comme runner par défaut'
        '--proton:Configurer Proton comme runner par défaut'
        '--dxvk:Utiliser DXVK standard'
        '--dxvk-async:Utiliser DXVK-GPLAsync'
    )
    _describe -t commands 'gwine commands' commands "$@"
}

gwine_files() {
    _files -g "*.wgp *.exe"
}

gwine_dir_commands() {
    local -a dir_cmds
    dir_cmds=(
        'add:Ajouter un répertoire aux bind mounts'
        'del:Supprimer un répertoire des bind mounts'
        'list:Lister tous les répertoires bindés'
        'reset:Réinitialiser la liste (supprime tout)'
    )
    _describe -t dir_cmds 'dir subcommands' dir_cmds "$@"
}

gwine_dir_path() {
    _directories
}

_arguments -C \
    '(-h --help)'{-h,--help}'[Afficher cette aide]' \
    '--fix[Activer le mode fix manette]' \
    '--xbox[Émulation manettes Sony en Xbox 360 via ds2xbox]' \
    '--xbox-ds4[Émulation DualShock 4 uniquement en Xbox 360]' \
    '--xbox-dualsense[Émulation DualSense uniquement en Xbox 360]' \
    '--xbox-on[Activer le mode xbox par défaut pour tous les lancements]' \
    '--xbox-off[Désactiver le mode xbox par défaut]' \
    '--reset[Réinitialiser toutes les options par défaut]' \
    '--nofix[Ignorer le fichier .fix d'"'"'un jeu]' \
    '--exewgp[Mode interactif pour choisir l\'exécutable dans un WGP]' \
    '--init[Initialiser/réinitialiser le préfixe Wine]' \
    '--offline[Mode offline pour --init]' \
    '--update[Mettre à jour les composants]' \
    '--kdialog[Activer les dialogues graphiques kdialog]' \
    '--cmd[Ouvrir un terminal Wine ou exécuter une commande (cmd /C)]:commande:_files -g "*.bat *.exe *.cmd *.msi"' \
    '--regedit[Lancer l\'éditeur de registre]' \
    '--reg[Installer un fichier de registre]:fichier reg:_files -g "*.reg"' \
    '--winecfg[Lancer la configuration Wine]' \
    '--winetricks[Lancer winetricks]' \
    '--winetrick[Lancer winetricks]' \
    '--download-components[Télécharger tous les composants pour le mode offline]' \
    '--cachepack[Créer un pack du cache pour installation offline]' \
    '--nosandbox[Désactiver le sandboxing]' \
    '--joytest[Lancer le test de joypad (joy.cpl)]' \
    '--use-ln-mounts[Utiliser des liens symboliques au lieu de bind mounts]' \
    '--x11[Utiliser X11/XWayland comme mode d\'affichage]' \
    '--wayland[Utiliser Wayland natif comme mode d\'affichage]' \
    '--env[Passer une variable d\'environnement]:variable:' \
    '--args[Arguments à passer au jeu]:arguments:' \
    '--gameid[ID du jeu]:id:' \
    '--kill[Forcer l\'arrêt de tous les processus Gwine en cours]' \
    '--dir[Gérer les répertoires bindés]:subcommand:gwine_dir_commands' \
    '--wine[Configurer Wine comme runner par défaut]' \
    '--proton[Configurer Proton comme runner par défaut]' \
    '--dxvk[Utiliser DXVK standard]' \
    '--dxvk-async[Utiliser DXVK-GPLAsync]' \
    '*:fichier:gwine_files'
