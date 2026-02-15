#!/usr/bin/python3
# -*- coding: utf-8 -*-
"""
AppImage Icon Extractor
Extrait l'icône d'un fichier AppImage

Usage: appimageiconextract.py <appimage_path> <output_png>
"""

import sys
import os
import subprocess
import tempfile
import shutil
import xml.etree.ElementTree as ET


def extract_appimage_icon(appimage_path, output_path):
    """Extrait l'icône d'un AppImage"""
    
    if not os.path.exists(appimage_path):
        print(f"Erreur: AppImage non trouvé: {appimage_path}")
        return False
    
    # Créer un dossier temporaire pour le montage
    temp_mount = tempfile.mkdtemp(prefix='appimage_mount_')
    
    try:
        # Essayer de monter l'AppImage avec squashfuse
        mounted = False
        
        # Vérifier si c'est un fichier squashfs (AppImage type 2)
        with open(appimage_path, 'rb') as f:
            magic = f.read(4)
            if magic == b'hsqs':
                # C'est un squashfs, essayer de le monter
                try:
                    result = subprocess.run(
                        ['squashfuse', appimage_path, temp_mount],
                        capture_output=True,
                        timeout=10
                    )
                    if result.returncode == 0:
                        mounted = True
                except:
                    pass
        
        extract_dir = None
        if not mounted:
            # Essayer avec --appimage-extract
            try:
                extract_dir = tempfile.mkdtemp(prefix='appimage_extract_')
                result = subprocess.run(
                    [appimage_path, '--appimage-extract'],
                    cwd=extract_dir,
                    capture_output=True,
                    timeout=30
                )
                if result.returncode == 0:
                    temp_mount = os.path.join(extract_dir, 'squashfs-root')
                    mounted = True
            except:
                pass
        
        if not mounted:
            print("Erreur: Impossible de monter ou extraire l'AppImage")
            return False
        
        # Chercher le fichier .desktop pour trouver le nom de l'icône
        icon_name = None
        desktop_file = None
        
        # Chercher les fichiers .desktop
        for root, dirs, files in os.walk(temp_mount):
            for file in files:
                if file.endswith('.desktop'):
                    desktop_file = os.path.join(root, file)
                    break
            if desktop_file:
                break
        
        if desktop_file:
            # Lire le fichier .desktop
            with open(desktop_file, 'r', encoding='utf-8', errors='ignore') as f:
                for line in f:
                    if line.startswith('Icon='):
                        icon_name = line.strip()[5:]
                        break
        
        # Chercher l'icône
        icon_path = None
        
        if icon_name:
            # Chercher l'icône par son nom
            search_paths = [
                os.path.join(temp_mount, 'usr', 'share', 'icons'),
                os.path.join(temp_mount, 'usr', 'share', 'pixmaps'),
                temp_mount,
            ]
            
            for search_path in search_paths:
                if os.path.exists(search_path):
                    for root, dirs, files in os.walk(search_path):
                        for file in files:
                            file_name = os.path.splitext(file)[0]
                            if file_name == icon_name:
                                full_path = os.path.join(root, file)
                                # Vérifier que c'est une image
                                if file.lower().endswith(('.png', '.jpg', '.jpeg', '.svg', '.ico')):
                                    icon_path = full_path
                                    break
                        if icon_path:
                            break
                if icon_path:
                    break
        
        # Si pas d'icône trouvée par nom, chercher n'importe quelle icône
        if not icon_path:
            icon_dirs = [
                os.path.join(temp_mount, 'usr', 'share', 'icons'),
                os.path.join(temp_mount, 'usr', 'share', 'pixmaps'),
            ]
            
            for icon_dir in icon_dirs:
                if os.path.exists(icon_dir):
                    # Chercher des fichiers d'icônes
                    for root, dirs, files in os.walk(icon_dir):
                        for file in files:
                            if file.lower().endswith(('.png', '.svg')):
                                # Prendre la plus grande icône trouvée
                                full_path = os.path.join(root, file)
                                if not icon_path:
                                    icon_path = full_path
                                else:
                                    # Comparer les tailles si c'est un PNG
                                    if file.lower().endswith('.png'):
                                        try:
                                            # Essayer de trouver une taille dans le chemin (ex: 256x256)
                                            import re
                                            size_match = re.search(r'(\d+)x\d+', root)
                                            if size_match:
                                                size = int(size_match.group(1))
                                                current_size = 0
                                                current_match = re.search(r'(\d+)x\d+', os.path.dirname(icon_path))
                                                if current_match:
                                                    current_size = int(current_match.group(1))
                                                if size > current_size:
                                                    icon_path = full_path
                                        except:
                                            pass
                        if icon_path:
                            break
                if icon_path:
                    break
        
        # Convertir et sauvegarder l'icône
        if icon_path:
            if icon_path.lower().endswith('.svg'):
                # Convertir SVG en PNG
                try:
                    subprocess.run(
                        ['convert', '-background', 'none', icon_path, '-resize', '256x256', output_path],
                        check=True,
                        timeout=10
                    )
                    return True
                except:
                    # Essayer avec inkscape
                    try:
                        subprocess.run(
                            ['inkscape', icon_path, '--export-filename=' + output_path, '--export-width=256', '--export-height=256'],
                            check=True,
                            timeout=10
                        )
                        return True
                    except:
                        pass
            elif icon_path.lower().endswith('.ico'):
                # Extraire la plus grande icône du fichier .ico
                try:
                    subprocess.run(
                        ['convert', icon_path, '-resize', '256x256', output_path],
                        check=True,
                        timeout=10
                    )
                    return True
                except:
                    pass
            else:
                # Copier directement et redimensionner si nécessaire
                try:
                    subprocess.run(
                        ['convert', icon_path, '-resize', '256x256>', output_path],
                        check=True,
                        timeout=10
                    )
                    return True
                except:
                    # Si convert échoue, copier simplement
                    shutil.copy2(icon_path, output_path)
                    return True
        
        print("Aucune icône trouvée dans l'AppImage")
        return False
        
    finally:
        # Nettoyer
        try:
            if mounted:
                subprocess.run(['fusermount', '-u', temp_mount], capture_output=True)
            shutil.rmtree(temp_mount, ignore_errors=True)
            if extract_dir:
                shutil.rmtree(extract_dir, ignore_errors=True)
        except:
            pass


def main():
    if len(sys.argv) < 3:
        print("Usage: appimageiconextract.py <appimage_path> <output_png>")
        sys.exit(1)
    
    appimage_path = sys.argv[1]
    output_path = sys.argv[2]
    
    success = extract_appimage_icon(appimage_path, output_path)
    sys.exit(0 if success else 1)


if __name__ == '__main__':
    main()
