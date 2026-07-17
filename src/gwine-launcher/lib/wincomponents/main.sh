#!/bin/bash

################################################################################
# main.sh - Installation de tous les composants Windows
################################################################################

install_all_wincomponents() {
    echo "Installation des composants Windows..."
    
    if [ "${OFFLINE_MODE:-false}" = "true" ]; then
        if ! check_wincomponents_cache; then
            echo "Erreur: Mode offline activé mais certains composants sont manquants"
            echo "Lancez 'gwine --init' sans l'option --offline pour télécharger les composants"
            return 1
        fi
    fi
    
    install_corefonts
    
    install_vcrun 2010 both
    install_vcrun 2012 both
    install_vcrun 2013 both
    install_vcrun 2022 both
    
    install_dotnet 6 both
    install_dotnet 7 both
    install_dotnet 8 both
    
    install_d3dx9
    
    install_d3dcompiler 42
    install_d3dcompiler 43
    install_d3dcompiler_47
    
    install_xact

    if [ "$WINEARCH" = "win64" ]; then
        install_xact_x64
    fi
    
    install_msls31
    
    install_vb6run
    
    install_vcrun6
    
    install_openal
    
    install_physx
    
    install_wmp9
    
    echo "✓ Installation des composants Windows terminée"
    return 0
}
