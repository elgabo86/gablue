#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
WGP Creator - Création de paquets Windows Game Packs
Interface graphique PySide6 (Qt) pour la création de paquets WGP

Usage: makewgp.py <dossier_du_jeu>
"""

import sys
import os
import subprocess
import tempfile
import shutil
import threading
import uuid
from pathlib import Path

from PySide6.QtWidgets import (
    QApplication, QMainWindow, QWidget, QVBoxLayout, QHBoxLayout,
    QLabel, QPushButton, QLineEdit, QListWidget, QListWidgetItem,
    QComboBox, QCheckBox, QProgressDialog, QMessageBox, QFileDialog,
    QGroupBox, QFrame, QScrollArea, QSizePolicy
)
from PySide6.QtCore import Qt, QThread, Signal, QTimer
from PySide6.QtGui import QPixmap, QIcon, QFont


class CreateWGPThread(QThread):
    """Thread pour créer le WGP sans bloquer l'interface"""
    progress = Signal(int, str)
    finished = Signal(bool, str)
    
    def __init__(self, game_dir, game_name, config, parent=None):
        super().__init__(parent)
        self.game_dir = game_dir
        self.game_name = game_name
        self.config = config
        self.cancelled = False
        
    def run(self):
        try:
            # Créer les fichiers de configuration
            self.progress.emit(10, "Création des fichiers de configuration...")
            self.create_config_files()
            
            if self.cancelled:
                self.cleanup()
                self.finished.emit(False, "Création annulée par l'utilisateur")
                return
            
            # Créer le squashfs avec progression temps réel
            self.progress.emit(30, "Préparation de l'archive WGP...")
            wgp_file = os.path.join(os.path.dirname(self.game_dir), f"{self.game_name}.wgp")
            
            result = self.create_squashfs(wgp_file)
            
            if self.cancelled:
                self.cleanup()
                if os.path.exists(wgp_file):
                    os.remove(wgp_file)
                self.finished.emit(False, "Création annulée par l'utilisateur")
                return
            
            if result.returncode == 0:
                self.progress.emit(100, "Terminé !")
                # Restaurer les fichiers originaux et supprimer les dossiers .save/.extra
                self.cleanup()
                self.finished.emit(True, wgp_file)
            else:
                # Restaurer les fichiers en cas d'échec de mksquashfs
                self.cleanup()
                self.finished.emit(False, f"Erreur lors de la création: {result.stderr}")
                
        except Exception as e:
            # Restaurer les fichiers et nettoyer en cas d'erreur
            self.cleanup()
            self.finished.emit(False, f"Erreur: {str(e)}")
    
    def create_config_files(self):
        """Crée les fichiers de configuration (.gamename, .launch, .args, .fix, etc.)"""
        # .gamename
        with open(os.path.join(self.game_dir, '.gamename'), 'w') as f:
            f.write(self.game_name)
        
        # .launch (contient le chemin relatif de l'exécutable)
        with open(os.path.join(self.game_dir, '.launch'), 'w') as f:
            f.write(self.config['exe'])
        
        # .args
        with open(os.path.join(self.game_dir, '.args'), 'w') as f:
            f.write(self.config['args'])
        
        # .fix (fichier vide si fix activé)
        if self.config['fix_controller']:
            open(os.path.join(self.game_dir, '.fix'), 'w').close()
        
        # .icon - Copier AVANT de traiter les saves/extras car l'icône
        # peut être dans un dossier qui sera déplacé
        if self.config['icon']:
            icon_dest = os.path.join(self.game_dir, '.icon.png')
            # Ne copier que si l'icône source est différente de la destination
            if os.path.abspath(self.config['icon']) != os.path.abspath(icon_dest):
                shutil.copy2(self.config['icon'], icon_dest)
        
        # Créer les symlinks et copier vers .save/.extra
        self._process_saves_and_extras()
    
    def _copy_dir_contents(self, source, target):
        """Copie le contenu d'un répertoire source vers target (sans copier le répertoire lui-même)"""
        os.makedirs(target, exist_ok=True)
        for item in os.listdir(source):
            s = os.path.join(source, item)
            d = os.path.join(target, item)
            if os.path.islink(s):
                # Préserver les symlinks
                if os.path.islink(d):
                    os.remove(d)
                elif os.path.exists(d):
                    if os.path.isdir(d):
                        shutil.rmtree(d)
                    else:
                        os.remove(d)
                os.symlink(os.readlink(s), d)
            elif os.path.isdir(s):
                if os.path.exists(d):
                    shutil.rmtree(d)
                # Préserver les symlinks lors de la copie récursive
                shutil.copytree(s, d, symlinks=True)
            else:
                shutil.copy2(s, d)

    def _process_saves_and_extras(self):
        """Crée les dossiers .save/.extra, copie les fichiers et crée les symlinks"""
        print(f"DEBUG: _process_saves_and_extras called")
        print(f"DEBUG: game_dir = {self.game_dir}")
        print(f"DEBUG: saves = {self.config.get('saves', [])}")
        print(f"DEBUG: extras = {self.config.get('extras', [])}")
        
        # Traiter les sauvegardes
        if self.config['saves']:
            saves_dir = os.path.join(self.game_dir, '.save')
            os.makedirs(saves_dir, exist_ok=True)
            print(f"DEBUG: Created saves_dir = {saves_dir}")

            with open(os.path.join(self.game_dir, '.savepath'), 'w') as f:
                for item_type, rel_path in self.config['saves']:
                    # Format compatible avec makewgp.sh (sans préfixe)
                    f.write(f"{rel_path}\n")

                    # Créer le symlink
                    source = os.path.join(self.game_dir, rel_path)
                    target = os.path.join(saves_dir, rel_path)
                    
                    print(f"DEBUG: Processing save: {rel_path} (type={item_type})")
                    print(f"DEBUG: source = {source}, exists={os.path.exists(source)}")
                    print(f"DEBUG: target = {target}")

                    if os.path.exists(source):
                        # Copier vers .save (contenu uniquement pour les dossiers)
                        os.makedirs(os.path.dirname(target), exist_ok=True)
                        if item_type == 'dir':
                            print(f"DEBUG: Copying dir contents from {source} to {target}")
                            self._copy_dir_contents(source, target)
                        else:
                            print(f"DEBUG: Copying file from {source} to {target}")
                            shutil.copy2(source, target)
                        
                        # Vérifier que la copie a fonctionné
                        if os.path.exists(target):
                            print(f"DEBUG: Copy successful, target exists")
                        else:
                            print(f"DEBUG: ERROR - Copy failed, target does not exist!")

                        # Créer le symlink
                        saves_base = f"/tmp/wgp-saves/{self.game_name}"
                        if os.path.islink(source):
                            os.remove(source)
                        elif os.path.isdir(source):
                            shutil.rmtree(source)
                        else:
                            os.remove(source)
                        os.makedirs(os.path.dirname(source), exist_ok=True)
                        os.symlink(os.path.join(saves_base, rel_path), source)
                        print(f"DEBUG: Created symlink {source} -> {os.path.join(saves_base, rel_path)}")
        else:
            print(f"DEBUG: No saves to process")

        # Traiter les extras
        if self.config['extras']:
            extras_dir = os.path.join(self.game_dir, '.extra')
            os.makedirs(extras_dir, exist_ok=True)
            print(f"DEBUG: Created extras_dir = {extras_dir}")

            with open(os.path.join(self.game_dir, '.extrapath'), 'w') as f:
                for item_type, rel_path in self.config['extras']:
                    # Format compatible avec makewgp.sh (sans préfixe)
                    f.write(f"{rel_path}\n")

                    # Créer le symlink
                    source = os.path.join(self.game_dir, rel_path)
                    target = os.path.join(extras_dir, rel_path)
                    
                    print(f"DEBUG: Processing extra: {rel_path} (type={item_type})")
                    print(f"DEBUG: source = {source}, exists={os.path.exists(source)}")
                    print(f"DEBUG: target = {target}")

                    if os.path.exists(source):
                        # Copier vers .extra (contenu uniquement pour les dossiers)
                        os.makedirs(os.path.dirname(target), exist_ok=True)
                        if item_type == 'dir':
                            print(f"DEBUG: Copying dir contents from {source} to {target}")
                            self._copy_dir_contents(source, target)
                        else:
                            print(f"DEBUG: Copying file from {source} to {target}")
                            shutil.copy2(source, target)
                        
                        # Vérifier que la copie a fonctionné
                        if os.path.exists(target):
                            print(f"DEBUG: Copy successful, target exists")
                        else:
                            print(f"DEBUG: ERROR - Copy failed, target does not exist!")

                        # Créer le symlink vers /tmp/wgp-extra (comme pour les saves)
                        extras_base = f"/tmp/wgp-extra/{self.game_name}"
                        if os.path.islink(source):
                            os.remove(source)
                        elif os.path.isdir(source):
                            shutil.rmtree(source)
                        else:
                            os.remove(source)
                        os.makedirs(os.path.dirname(source), exist_ok=True)
                        os.symlink(os.path.join(extras_base, rel_path), source)
                        print(f"DEBUG: Created symlink {source} -> {os.path.join(extras_base, rel_path)}")
        else:
            print(f"DEBUG: No extras to process")
    
    def create_squashfs(self, wgp_file):
        """Crée l'archive squashfs avec progression temps réel basée sur les fichiers traités"""
        import subprocess
        
        comp_level = self.config['compression']
        
        # Compter le nombre total de fichiers dans le dossier source AVANT de lancer mksquashfs
        total_files = 0
        for root, dirs, files in os.walk(self.game_dir):
            dirs[:] = [d for d in dirs if d not in ['.save', '.extra', '__pycache__']]
            total_files += len(files)
        
        if total_files == 0:
            total_files = 1  # Éviter division par zéro
        
        cmd = [
            'mksquashfs',
            self.game_dir,
            wgp_file,
            '-comp', 'zstd',
            '-Xcompression-level', str(comp_level),
            '-noappend',
            '-wildcards',
            '-info'  # Affiche chaque fichier traité
        ]
        
        # Exclure les fichiers temporaires (mais PAS .save et .extra qui contiennent les données)
        excludes = ['*.tmp', '*.log']
        for exclude in excludes:
            cmd.extend(['-e', exclude])
        
        # Lancer mksquashfs avec stderr redirigé pour capturer la progression
        process = subprocess.Popen(
            cmd,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            bufsize=1,
            universal_newlines=True
        )
        
        files_processed = 0
        last_progress = 0
        self.progress.emit(0, "Démarrage de la compression...")
        
        # Parser la sortie en temps réel
        while True:
            line = process.stdout.readline()
            if not line:
                break
            
            # Rechercher les lignes "file " qui indiquent un fichier traité
            if line.startswith('file '):
                files_processed += 1
                percentage = min(int((files_processed / total_files) * 100), 100)
                if percentage > last_progress:
                    last_progress = percentage
                    self.progress.emit(percentage, f"Compression en cours... {percentage}% ({files_processed}/{total_files} fichiers)")
            elif 'Creating' in line and 'filesystem' in line:
                self.progress.emit(0, "Initialisation de la compression...")
            
            # Vérifier si annulation demandée
            if self.cancelled:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except:
                    process.kill()
                return subprocess.CompletedProcess(cmd, -1, '', 'Annulé par l\'utilisateur')
        
        # Attendre la fin du processus
        process.wait()
        
        # Créer un objet CompletedProcess similaire à subprocess.run
        return subprocess.CompletedProcess(cmd, process.returncode, '', '')
    
    def cleanup(self):
        """Nettoie les fichiers temporaires et restaure les fichiers originaux"""
        self.restore_files()
        self.cleanup_dirs_only()
    
    def cleanup_dirs_only(self):
        """Supprime uniquement les dossiers .save et .extra sans restaurer les fichiers"""
        for f in ['.save', '.extra']:
            path = os.path.join(self.game_dir, f)
            if os.path.exists(path):
                shutil.rmtree(path)
    
    def restore_files(self):
        """Restaure les fichiers originaux depuis .save et .extra (compatible makewgp.sh)"""
        print(f"DEBUG: restore_files called")
        print(f"DEBUG: game_dir = {self.game_dir}")
        
        # Restaurer depuis .save en utilisant .savepath
        savepath_file = os.path.join(self.game_dir, '.savepath')
        saves_backup_dir = os.path.join(self.game_dir, '.save')
        print(f"DEBUG: savepath_file = {savepath_file}, exists={os.path.exists(savepath_file)}")
        print(f"DEBUG: saves_backup_dir = {saves_backup_dir}, exists={os.path.exists(saves_backup_dir)}")
        
        if os.path.exists(savepath_file) and os.path.exists(saves_backup_dir):
            print(f"DEBUG: Restoring saves...")
            with open(savepath_file, 'r') as f:
                for line in f:
                    rel_path = line.strip()
                    if not rel_path:
                        continue

                    backup_path = os.path.join(saves_backup_dir, rel_path)
                    original_path = os.path.join(self.game_dir, rel_path)
                    
                    print(f"DEBUG: Restoring save: {rel_path}")
                    print(f"DEBUG: backup_path = {backup_path}, exists={os.path.exists(backup_path)}")
                    print(f"DEBUG: original_path = {original_path}")

                    if not os.path.exists(backup_path):
                        print(f"DEBUG: WARNING - backup_path does not exist, skipping")
                        continue

                    # Supprimer le symlink existant
                    if os.path.islink(original_path):
                        print(f"DEBUG: Removing symlink {original_path}")
                        os.remove(original_path)
                    elif os.path.exists(original_path):
                        print(f"DEBUG: Removing existing {original_path}")
                        if os.path.isdir(original_path):
                            shutil.rmtree(original_path)
                        else:
                            os.remove(original_path)

                    # Restaurer depuis backup (contenu uniquement pour les dossiers)
                    os.makedirs(os.path.dirname(original_path), exist_ok=True)
                    if os.path.isdir(backup_path):
                        print(f"DEBUG: Copying dir contents from {backup_path} to {original_path}")
                        self._copy_dir_contents(backup_path, original_path)
                    else:
                        print(f"DEBUG: Copying file from {backup_path} to {original_path}")
                        shutil.copy2(backup_path, original_path)
                    
                    print(f"DEBUG: Restore complete for {rel_path}")

        # Restaurer depuis .extra en utilisant .extrapath
        extrapath_file = os.path.join(self.game_dir, '.extrapath')
        extras_backup_dir = os.path.join(self.game_dir, '.extra')
        if os.path.exists(extrapath_file) and os.path.exists(extras_backup_dir):
            with open(extrapath_file, 'r') as f:
                for line in f:
                    rel_path = line.strip()
                    if not rel_path:
                        continue

                    backup_path = os.path.join(extras_backup_dir, rel_path)
                    original_path = os.path.join(self.game_dir, rel_path)

                    if not os.path.exists(backup_path):
                        continue

                    # Supprimer le symlink existant
                    if os.path.islink(original_path):
                        os.remove(original_path)
                    elif os.path.exists(original_path):
                        if os.path.isdir(original_path):
                            shutil.rmtree(original_path)
                        else:
                            os.remove(original_path)

                    # Restaurer depuis backup (contenu uniquement pour les dossiers)
                    os.makedirs(os.path.dirname(original_path), exist_ok=True)
                    if os.path.isdir(backup_path):
                        self._copy_dir_contents(backup_path, original_path)
                    else:
                        shutil.copy2(backup_path, original_path)
    
    def cancel(self):
        self.cancelled = True


class WGPWindow(QMainWindow):
    def __init__(self, game_dir=None):
        super().__init__()
        self.game_dir = game_dir
        self.game_name = ""
        self.exe_files = []
        self.icon_path = None
        self.create_thread = None
        
        self.setWindowTitle("WGP Creator - Création de paquets Windows Game Packs")
        self.setMinimumSize(900, 500)
        self.resize(1100, 600)
        
        self.setup_ui()
        
        if game_dir:
            self.load_game_directory(game_dir)
    
    def setup_ui(self):
        """Configure l'interface utilisateur avec un layout dynamique"""
        # Widget central
        central_widget = QWidget()
        self.setCentralWidget(central_widget)
        
        # Layout principal avec un splitter pour redimensionnement flexible
        main_layout = QVBoxLayout(central_widget)
        main_layout.setSpacing(10)
        main_layout.setContentsMargins(15, 15, 15, 15)
        
        # Titre
        title_label = QLabel("Création du paquet WGP")
        title_font = QFont()
        title_font.setPointSize(16)
        title_font.setBold(True)
        title_label.setFont(title_font)
        title_label.setStyleSheet("color: #588CFF;")
        title_label.setAlignment(Qt.AlignCenter)
        main_layout.addWidget(title_label)
        
        # === SECTION HAUTE: Nom du paquet ===
        name_layout = QHBoxLayout()
        name_label = QLabel("Nom du paquet:")
        name_label.setFixedWidth(120)
        self.name_input = QLineEdit()
        self.name_input.setPlaceholderText("Nom du paquet WGP...")
        name_layout.addWidget(name_label)
        name_layout.addWidget(self.name_input)
        main_layout.addLayout(name_layout)
        
        # === SECTION MILIEU: Layout principal avec 2 colonnes ===
        middle_layout = QHBoxLayout()
        middle_layout.setSpacing(10)
        main_layout.addLayout(middle_layout, stretch=1)
        
        # --- COLONNE GAUCHE: Exécutable, Icônes, Arguments, Options ---
        left_layout = QVBoxLayout()
        left_layout.setSpacing(8)
        middle_layout.addLayout(left_layout, stretch=1)
        
        # Exécutable principal (compact)
        exe_group = QGroupBox("Exécutable principal")
        exe_layout = QVBoxLayout(exe_group)
        exe_layout.setContentsMargins(8, 12, 8, 8)
        self.exe_list = QListWidget()
        self.exe_list.setMaximumHeight(80)
        self.exe_list.setMinimumHeight(50)
        self.exe_list.currentRowChanged.connect(self.on_exe_selected)
        exe_layout.addWidget(self.exe_list)
        left_layout.addWidget(exe_group)
        
        # Icône du jeu (déplacée ici)
        icon_group = QGroupBox("Icône du jeu")
        icon_layout = QVBoxLayout(icon_group)
        icon_layout.setContentsMargins(8, 12, 8, 8)
        
        # Zone de défilement pour les icônes
        scroll_area = QScrollArea()
        scroll_area.setWidgetResizable(True)
        scroll_area.setHorizontalScrollBarPolicy(Qt.ScrollBarAsNeeded)
        scroll_area.setVerticalScrollBarPolicy(Qt.ScrollBarAlwaysOff)
        scroll_area.setFixedHeight(85)
        scroll_area.setStyleSheet("QScrollArea { border: none; background: transparent; }")
        
        # Widget conteneur pour la grille d'icônes
        self.icons_container = QWidget()
        self.icons_layout = QHBoxLayout(self.icons_container)
        self.icons_layout.setSpacing(6)
        self.icons_layout.setAlignment(Qt.AlignLeft)
        self.icons_layout.setContentsMargins(4, 4, 4, 4)
        
        scroll_area.setWidget(self.icons_container)
        icon_layout.addWidget(scroll_area)
        
        # Label d'info + Bouton ajouter sur la même ligne
        icon_bottom_layout = QHBoxLayout()
        self.icon_info_label = QLabel("Cliquez sur une icône")
        self.icon_info_label.setStyleSheet("color: #B4B4BE; font-size: 10px;")
        icon_bottom_layout.addWidget(self.icon_info_label)
        icon_bottom_layout.addStretch()
        add_icon_btn = QPushButton("+ Ajouter...")
        add_icon_btn.setMaximumWidth(90)
        add_icon_btn.clicked.connect(self.select_custom_icon)
        icon_bottom_layout.addWidget(add_icon_btn)
        icon_layout.addLayout(icon_bottom_layout)
        
        left_layout.addWidget(icon_group)
        
        # Arguments
        args_group = QGroupBox("Arguments")
        args_layout = QVBoxLayout(args_group)
        args_layout.setContentsMargins(8, 12, 8, 8)
        self.args_input = QLineEdit()
        self.args_input.setPlaceholderText("ex: --dx12 --fullscreen")
        args_layout.addWidget(self.args_input)
        left_layout.addWidget(args_group)
        
        # Fix manette et Compression sur la même ligne
        options_layout = QHBoxLayout()
        self.fix_checkbox = QCheckBox("Fix manette")
        options_layout.addWidget(self.fix_checkbox)
        
        options_layout.addSpacing(15)
        
        comp_label = QLabel("Compression:")
        comp_label.setFixedWidth(80)
        options_layout.addWidget(comp_label)
        
        self.comp_combo = QComboBox()
        self.comp_combo.addItems(["Non (0)", "5", "10", "15", "19"])
        self.comp_combo.setCurrentIndex(3)  # 15 par défaut
        self.comp_combo.setFixedWidth(90)
        options_layout.addWidget(self.comp_combo)
        options_layout.addStretch()
        
        left_layout.addLayout(options_layout)
        
        # --- COLONNE DROITE: Sauvegardes et Extras ---
        right_layout = QVBoxLayout()
        right_layout.setSpacing(8)
        middle_layout.addLayout(right_layout, stretch=1)
        
        # Sauvegardes persistantes
        saves_group = QGroupBox("Sauvegardes persistantes")
        saves_group.setToolTip("Stockées dans UserData (persistantes)")
        saves_layout = QVBoxLayout(saves_group)
        saves_layout.setContentsMargins(8, 12, 8, 8)
        
        self.saves_list = QListWidget()
        self.saves_list.setMinimumHeight(80)
        saves_layout.addWidget(self.saves_list)
        
        saves_btn_layout = QHBoxLayout()
        saves_btn_layout.setSpacing(5)
        add_save_file_btn = QPushButton("+ Fichier")
        add_save_file_btn.setMaximumWidth(80)
        add_save_file_btn.clicked.connect(lambda: self.add_item('save', 'file'))
        add_save_dir_btn = QPushButton("+ Dossier")
        add_save_dir_btn.setMaximumWidth(80)
        add_save_dir_btn.clicked.connect(lambda: self.add_item('save', 'dir'))
        remove_save_btn = QPushButton("- Suppr")
        remove_save_btn.setMaximumWidth(60)
        remove_save_btn.clicked.connect(lambda: self.remove_item('save'))
        
        saves_btn_layout.addWidget(add_save_file_btn)
        saves_btn_layout.addWidget(add_save_dir_btn)
        saves_btn_layout.addWidget(remove_save_btn)
        saves_btn_layout.addStretch()
        saves_layout.addLayout(saves_btn_layout)
        right_layout.addWidget(saves_group)
        
        # Fichiers temporaires
        extras_group = QGroupBox("Fichiers temporaires (extras)")
        extras_group.setToolTip("Stockées dans /tmp (non persistants)")
        extras_layout = QVBoxLayout(extras_group)
        extras_layout.setContentsMargins(8, 12, 8, 8)
        
        self.extras_list = QListWidget()
        self.extras_list.setMinimumHeight(80)
        extras_layout.addWidget(self.extras_list)
        
        extras_btn_layout = QHBoxLayout()
        extras_btn_layout.setSpacing(5)
        add_extra_file_btn = QPushButton("+ Fichier")
        add_extra_file_btn.setMaximumWidth(80)
        add_extra_file_btn.clicked.connect(lambda: self.add_item('extra', 'file'))
        add_extra_dir_btn = QPushButton("+ Dossier")
        add_extra_dir_btn.setMaximumWidth(80)
        add_extra_dir_btn.clicked.connect(lambda: self.add_item('extra', 'dir'))
        remove_extra_btn = QPushButton("- Suppr")
        remove_extra_btn.setMaximumWidth(60)
        remove_extra_btn.clicked.connect(lambda: self.remove_item('extra'))
        
        extras_btn_layout.addWidget(add_extra_file_btn)
        extras_btn_layout.addWidget(add_extra_dir_btn)
        extras_btn_layout.addWidget(remove_extra_btn)
        extras_btn_layout.addStretch()
        extras_layout.addLayout(extras_btn_layout)
        right_layout.addWidget(extras_group)
        
        # === BOUTONS PRINCIPAUX ===
        button_layout = QHBoxLayout()
        button_layout.setSpacing(10)
        main_layout.addLayout(button_layout)
        
        button_layout.addStretch()
        
        self.create_btn = QPushButton("Créer le WGP")
        self.create_btn.setMinimumWidth(140)
        self.create_btn.setMinimumHeight(35)
        self.create_btn.setStyleSheet("""
            QPushButton {
                background-color: #588CFF;
                color: white;
                border: none;
                border-radius: 6px;
                font-weight: bold;
                font-size: 13px;
            }
            QPushButton:hover {
                background-color: #6CA0FF;
            }
            QPushButton:pressed {
                background-color: #4A7AE8;
            }
        """)
        self.create_btn.clicked.connect(self.start_create_wgp)
        button_layout.addWidget(self.create_btn)
        
        cancel_btn = QPushButton("Annuler")
        cancel_btn.setMinimumWidth(100)
        cancel_btn.setMinimumHeight(35)
        cancel_btn.clicked.connect(self.close)
        button_layout.addWidget(cancel_btn)
        
        # Données
        self.saves = []
        self.extras = []
    
    def load_game_directory(self, game_dir):
        """Charge le dossier du jeu et les fichiers de configuration existants"""
        self.game_dir = os.path.abspath(game_dir)
        self.game_name = os.path.basename(self.game_dir)
        
        # Charger le nom du jeu depuis .gamename si existe
        gamename_file = os.path.join(self.game_dir, '.gamename')
        if os.path.exists(gamename_file):
            with open(gamename_file, 'r') as f:
                self.game_name = f.read().strip()
        self.name_input.setText(self.game_name)
        
        # Charger les arguments depuis .args si existe
        args_file = os.path.join(self.game_dir, '.args')
        if os.path.exists(args_file):
            with open(args_file, 'r') as f:
                self.args_input.setText(f.read().strip())
        
        # Charger le fix manette depuis .fix si existe
        fix_file = os.path.join(self.game_dir, '.fix')
        self.fix_checkbox.setChecked(os.path.exists(fix_file))
        
        # Charger les sauvegardes depuis .savepath si existe
        savepath_file = os.path.join(self.game_dir, '.savepath')
        if os.path.exists(savepath_file):
            self.saves = []
            with open(savepath_file, 'r') as f:
                for line in f:
                    line = line.strip()
                    if line:
                        if line.startswith('D:'):
                            self.saves.append(('dir', line[2:]))
                        elif line.startswith('F:'):
                            self.saves.append(('file', line[2:]))
                        else:
                            # Format ancien sans préfixe, deviner selon l'existence
                            full_path = os.path.join(self.game_dir, line)
                            if os.path.isdir(full_path):
                                self.saves.append(('dir', line))
                            else:
                                self.saves.append(('file', line))
            self.update_saves_list()
        
        # Charger les extras depuis .extrapath si existe
        extrapath_file = os.path.join(self.game_dir, '.extrapath')
        if os.path.exists(extrapath_file):
            self.extras = []
            with open(extrapath_file, 'r') as f:
                for line in f:
                    line = line.strip()
                    if line:
                        if line.startswith('D:'):
                            self.extras.append(('dir', line[2:]))
                        elif line.startswith('F:'):
                            self.extras.append(('file', line[2:]))
                        else:
                            # Format ancien sans préfixe, deviner selon l'existence
                            full_path = os.path.join(self.game_dir, line)
                            if os.path.isdir(full_path):
                                self.extras.append(('dir', line))
                            else:
                                self.extras.append(('file', line))
            self.update_extras_list()
        
        # Charger toutes les icônes disponibles
        self.load_available_icons()
        
        # Rechercher les .exe et .bat
        self.exe_files = []
        for root, dirs, files in os.walk(self.game_dir):
            # Ignorer les dossiers spéciaux
            dirs[:] = [d for d in dirs if d not in ['.save', '.extra', '__pycache__']]
            
            for file in files:
                if file.lower().endswith(('.exe', '.bat')):
                    rel_path = os.path.relpath(os.path.join(root, file), self.game_dir)
                    self.exe_files.append(rel_path)
        
        self.exe_files.sort()
        self.exe_list.clear()
        self.exe_list.addItems(self.exe_files)
        
        # Sélectionner l'exécutable depuis .launch si existe
        selected_exe = None
        launch_file = os.path.join(self.game_dir, '.launch')
        if os.path.exists(launch_file):
            with open(launch_file, 'r') as f:
                saved_exe = f.read().strip()
                if saved_exe in self.exe_files:
                    index = self.exe_files.index(saved_exe)
                    self.exe_list.setCurrentRow(index)
                    selected_exe = saved_exe
                elif self.exe_files:
                    self.exe_list.setCurrentRow(0)
                    selected_exe = self.exe_files[0]
        elif self.exe_files:
            self.exe_list.setCurrentRow(0)
            selected_exe = self.exe_files[0]
        
        # Sélectionner l'icône correspondant à l'exécutable sélectionné
        if selected_exe:
            self.select_icon_for_exe(selected_exe)
    
    def select_icon_for_exe(self, exe_path):
        """Sélectionne l'icône correspondant à l'exécutable donné"""
        exe_name = os.path.splitext(os.path.basename(exe_path))[0]
        
        # Priorité 1: Sélectionner l'icône existante si présente
        for idx, icon in enumerate(self.available_icons):
            if icon['source'] == 'existing':
                self.select_icon(idx)
                return
        
        # Priorité 2: Chercher une icône d'exe correspondante
        for idx, icon in enumerate(self.available_icons):
            if icon['source'] == 'exe':
                # Vérifier si c'est l'icône de cet exe
                if exe_name[:12] in icon['name'] or exe_name[:15] in icon['name']:
                    self.select_icon(idx)
                    return
        
        # Sinon, sélectionner la première icône disponible
        if self.available_icons:
            self.select_icon(0)
    
    def on_exe_selected(self, index):
        """Appelé quand un exécutable est sélectionné"""
        if index >= 0 and index < len(self.exe_files):
            exe_path = os.path.join(self.game_dir, self.exe_files[index])
            self.select_icon_for_exe(exe_path)
    
    def auto_extract_icon(self, exe_path):
        """Extrait l'icône depuis l'exe (utilisé si l'exe est sélectionné manuellement après chargement)"""
        # Les icônes sont déjà extraites au chargement, mais on peut en ajouter si besoin
        try:
            import tempfile
            
            # Vérifier si l'icône de cet exe est déjà dans la liste
            exe_name = os.path.splitext(os.path.basename(exe_path))[0]
            for icon in self.available_icons:
                if icon['source'] == 'exe' and exe_name[:12] in icon['name']:
                    # Déjà présent, juste la sélectionner
                    idx = self.available_icons.index(icon)
                    self.select_icon(idx)
                    return
            
            # Sinon, extraire et ajouter
            result = subprocess.run(
                ['wrestool', '-x', '-t', '14', exe_path],
                capture_output=True
            )
            
            if result.returncode == 0 and result.stdout:
                ico_file = os.path.join(tempfile.gettempdir(), f'wgp_icon_{uuid.uuid4().hex}.ico')
                with open(ico_file, 'wb') as f:
                    f.write(result.stdout)
                
                png_file = os.path.join(tempfile.gettempdir(), f'wgp_icon_{uuid.uuid4().hex}.png')
                subprocess.run(['icotool', '-x', '-o', png_file, ico_file], check=False)
                
                base_name = os.path.basename(png_file).replace('.png', '')
                temp_dir = tempfile.gettempdir()
                for f in os.listdir(temp_dir):
                    if f.startswith(base_name) and f.endswith('.png'):
                        full_path = os.path.join(temp_dir, f)
                        
                        icon_entry = {
                            'path': full_path,
                            'name': f"Exe: {exe_name[:15]}",
                            'source': 'exe'
                        }
                        
                        self.available_icons.append(icon_entry)
                        self.update_icons_display()
                        self.select_icon(len(self.available_icons) - 1)
                        break
                
                os.remove(ico_file)
                
        except Exception as e:
            print(f"Erreur extraction icône: {e}")
    
    def load_icon_preview(self, icon_path):
        """Charge l'aperçu de l'icône"""
        if os.path.exists(icon_path):
            pixmap = QPixmap(icon_path)
            if not pixmap.isNull():
                scaled = pixmap.scaled(64, 64, Qt.KeepAspectRatio, Qt.SmoothTransformation)
                self.icon_preview.setPixmap(scaled)
    
    def load_available_icons(self):
        """Charge toutes les icônes disponibles dans le dossier du jeu"""
        self.available_icons = []
        
        # 1. Chercher l'icône .icon.png existante (prioritaire)
        existing_icon = os.path.join(self.game_dir, '.icon.png')
        if os.path.exists(existing_icon):
            self.available_icons.append({
                'path': existing_icon,
                'name': 'Icône existante',
                'source': 'existing'
            })
        
        # 2. Chercher les fichiers .ico
        for root, dirs, files in os.walk(self.game_dir):
            # Ignorer certains dossiers
            dirs[:] = [d for d in dirs if d not in ['.save', '.extra', '__pycache__', 
                                                     'screenshots', 'textures', 'images', 
                                                     'data', 'assets', 'sounds', 'music']]
            for file in files:
                if file.lower().endswith('.ico'):
                    full_path = os.path.join(root, file)
                    self.available_icons.append({
                        'path': full_path,
                        'name': os.path.splitext(file)[0][:20],
                        'source': 'ico'
                    })
        
        # 3. Chercher les images qui semblent être des icônes (carrées et petite taille)
        for root, dirs, files in os.walk(self.game_dir):
            # Ignorer certains dossiers de données
            dirs[:] = [d for d in dirs if d not in ['.save', '.extra', '__pycache__', 
                                                     'screenshots', 'textures', 'images', 
                                                     'data', 'assets', 'sounds', 'music',
                                                     'saves', 'save', 'userdata']]
            for file in files:
                file_lower = file.lower()
                if file_lower.endswith(('.png', '.jpg', '.jpeg', '.bmp')):
                    full_path = os.path.join(root, file)
                    # Vérifier si c'est une icône (carrée et taille typique)
                    try:
                        pixmap = QPixmap(full_path)
                        if not pixmap.isNull():
                            width = pixmap.width()
                            height = pixmap.height()
                            # Doit être carré (ratio 1:1) et taille typique d'icône
                            is_square = abs(width - height) <= 2  # Tolérance de 2 pixels
                            is_icon_size = width in [16, 24, 32, 48, 64, 96, 128, 192, 256, 512]
                            
                            if is_square and is_icon_size:
                                self.available_icons.append({
                                    'path': full_path,
                                    'name': os.path.splitext(file)[0][:20],
                                    'source': 'image'
                                })
                    except:
                        pass
        
        # 4. Extraire les icônes de tous les .exe trouvés
        self.extract_icons_from_all_exes()
        
        # Limiter à 30 icônes max pour ne pas surcharger l'interface
        self.available_icons = self.available_icons[:30]
        
        # Mettre à jour l'affichage
        self.update_icons_display()
    
    def extract_icons_from_all_exes(self):
        """Extrait les icônes de tous les .exe du dossier"""
        import tempfile
        
        exe_files = []
        for root, dirs, files in os.walk(self.game_dir):
            dirs[:] = [d for d in dirs if d not in ['.save', '.extra', '__pycache__']]
            for file in files:
                if file.lower().endswith('.exe'):
                    exe_files.append(os.path.join(root, file))
        
        # Limiter à 10 exe max pour éviter d'être trop long
        for exe_path in exe_files[:10]:
            try:
                result = subprocess.run(
                    ['wrestool', '-x', '-t', '14', exe_path],
                    capture_output=True, timeout=10
                )
                
                if result.returncode == 0 and result.stdout:
                    # Sauvegarder l'icône
                    ico_file = os.path.join(tempfile.gettempdir(), f'wgp_icon_{uuid.uuid4().hex}.ico')
                    with open(ico_file, 'wb') as f:
                        f.write(result.stdout)
                    
                    # Convertir en PNG
                    png_file = os.path.join(tempfile.gettempdir(), f'wgp_icon_{uuid.uuid4().hex}.png')
                    subprocess.run(['icotool', '-x', '-o', png_file, ico_file], check=False)
                    
                    # Chercher le fichier PNG créé
                    base_name = os.path.basename(png_file).replace('.png', '')
                    temp_dir = tempfile.gettempdir()
                    for f in os.listdir(temp_dir):
                        if f.startswith(base_name) and f.endswith('.png'):
                            full_path = os.path.join(temp_dir, f)
                            exe_name = os.path.splitext(os.path.basename(exe_path))[0]
                            self.available_icons.append({
                                'path': full_path,
                                'name': f"Exe: {exe_name[:12]}",
                                'source': 'exe'
                            })
                            break
                    
                    # Nettoyer le fichier .ico
                    os.remove(ico_file)
                    
            except Exception as e:
                print(f"Erreur extraction icône de {exe_path}: {e}")
    
    def update_icons_display(self):
        """Met à jour l'affichage des icônes disponibles"""
        # Vider le layout existant
        while self.icons_layout.count():
            item = self.icons_layout.takeAt(0)
            if item.widget():
                item.widget().deleteLater()
        
        self.icon_widgets = []
        self.selected_icon_idx = -1
        
        if not self.available_icons:
            # Aucune icône trouvée
            no_icon_label = QLabel("Aucune icône trouvée\n(sélectionnez un .exe)")
            no_icon_label.setStyleSheet("color: #888; font-size: 10px;")
            no_icon_label.setAlignment(Qt.AlignCenter)
            self.icons_layout.addWidget(no_icon_label)
            return
        
        # Créer un widget pour chaque icône
        for idx, icon_info in enumerate(self.available_icons):
            icon_widget = self.create_icon_widget(icon_info, idx)
            self.icons_layout.addWidget(icon_widget)
            self.icon_widgets.append(icon_widget)
        
        # Sélectionner la première icône par défaut (icône existante si présente)
        if self.available_icons:
            self.select_icon(0)
    
    def create_icon_widget(self, icon_info, idx):
        """Crée un widget pour une icône cliquable"""
        from PySide6.QtWidgets import QFrame
        
        container = QFrame()
        container.setFixedSize(64, 64)
        container.setStyleSheet("""
            QFrame {
                border: 2px solid transparent;
                border-radius: 5px;
                background-color: #2D2D33;
            }
            QFrame:hover {
                border: 2px solid #588CFF;
            }
        """)
        container.setCursor(Qt.PointingHandCursor)
        container.mousePressEvent = lambda e, i=idx: self.select_icon(i)
        
        layout = QVBoxLayout(container)
        layout.setContentsMargins(3, 3, 3, 3)
        layout.setSpacing(1)
        
        # Image
        icon_label = QLabel()
        icon_label.setFixedSize(40, 40)
        icon_label.setAlignment(Qt.AlignCenter)
        
        pixmap = QPixmap(icon_info['path'])
        if not pixmap.isNull():
            scaled = pixmap.scaled(40, 40, Qt.KeepAspectRatio, Qt.SmoothTransformation)
            icon_label.setPixmap(scaled)
        
        layout.addWidget(icon_label, alignment=Qt.AlignCenter)
        
        # Nom tronqué
        name_label = QLabel(icon_info['name'][:8])
        name_label.setStyleSheet("font-size: 7px; color: #B4B4BE;")
        name_label.setAlignment(Qt.AlignCenter)
        layout.addWidget(name_label)
        
        return container
    
    def select_icon(self, idx):
        """Sélectionne une icône"""
        if idx < 0 or idx >= len(self.available_icons):
            return
        
        # Désélectionner l'ancienne
        if self.selected_icon_idx >= 0 and self.selected_icon_idx < len(self.icon_widgets):
            old_widget = self.icon_widgets[self.selected_icon_idx]
            old_widget.setStyleSheet("""
                QFrame {
                    border: 2px solid transparent;
                    border-radius: 6px;
                    background-color: #2D2D33;
                }
                QFrame:hover {
                    border: 2px solid #588CFF;
                }
            """)
        
        # Sélectionner la nouvelle
        self.selected_icon_idx = idx
        new_widget = self.icon_widgets[idx]
        new_widget.setStyleSheet("""
            QFrame {
                border: 2px solid #588CFF;
                border-radius: 6px;
                background-color: #3D4C5C;
            }
        """)
        
        # Mettre à jour le chemin de l'icône sélectionnée
        self.icon_path = self.available_icons[idx]['path']
        icon_name = self.available_icons[idx]['name']
        self.icon_info_label.setText(f"Sélectionnée: {icon_name}")
    
    def select_custom_icon(self):
        """Sélectionne une icône personnalisée externe"""
        file_path, _ = QFileDialog.getOpenFileName(
            self, "Sélectionner une icône", "",
            "Images (*.png *.ico *.jpg *.jpeg *.bmp)"
        )
        if file_path:
            # Ajouter à la liste et sélectionner
            self.available_icons.append({
                'path': file_path,
                'name': os.path.splitext(os.path.basename(file_path))[0][:20],
                'source': 'custom'
            })
            self.update_icons_display()
            # Sélectionner la dernière (celle qu'on vient d'ajouter)
            self.select_icon(len(self.available_icons) - 1)
    
    def _is_path_conflict(self, new_path, existing_list, existing_list_name):
        """Vérifie si le nouveau chemin entre en conflit avec les chemins existants
        
        Returns: (conflict: bool, message: str)
        """
        new_path = new_path.rstrip('/')
        
        for item_type, existing_path in existing_list:
            existing_path = existing_path.rstrip('/')
            
            # Vérifier si le nouveau chemin est DANS un dossier existant
            if new_path.startswith(existing_path + '/'):
                return True, f"'{new_path}' est contenu dans le dossier '{existing_path}' ({existing_list_name})"
            
            # Vérifier si un dossier existant est DANS le nouveau chemin
            if existing_path.startswith(new_path + '/'):
                return True, f"'{new_path}' contient déjà '{existing_path}' ({existing_list_name})"
        
        return False, ""
    
    def add_item(self, item_type, item_subtype):
        """Ajoute un fichier ou dossier à la liste"""
        if item_type == 'save':
            if item_subtype == 'file':
                file_path, _ = QFileDialog.getOpenFileName(self, "Sélectionner un fichier", self.game_dir)
                if file_path:
                    rel_path = os.path.relpath(file_path, self.game_dir)
                    # Vérifier si déjà présent dans saves
                    if ('file', rel_path) in self.saves:
                        QMessageBox.information(self, "Déjà présent", "Ce fichier est déjà dans la liste des sauvegardes.")
                        return
                    # Vérifier si présent dans extras
                    if ('file', rel_path) in self.extras:
                        QMessageBox.information(self, "Déjà présent", "Ce fichier est déjà dans la liste des extras.")
                        return
                    # Vérifier les conflits de chemins
                    conflict, msg = self._is_path_conflict(rel_path, self.saves, "sauvegardes")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    conflict, msg = self._is_path_conflict(rel_path, self.extras, "extras")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    self.saves.append(('file', rel_path))
            else:
                dir_path = QFileDialog.getExistingDirectory(self, "Sélectionner un dossier", self.game_dir)
                if dir_path:
                    rel_path = os.path.relpath(dir_path, self.game_dir)
                    # Vérifier si déjà présent dans saves
                    if ('dir', rel_path) in self.saves:
                        QMessageBox.information(self, "Déjà présent", "Ce dossier est déjà dans la liste des sauvegardes.")
                        return
                    # Vérifier si présent dans extras
                    if ('dir', rel_path) in self.extras:
                        QMessageBox.information(self, "Déjà présent", "Ce dossier est déjà dans la liste des extras.")
                        return
                    # Vérifier les conflits de chemins
                    conflict, msg = self._is_path_conflict(rel_path, self.saves, "sauvegardes")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    conflict, msg = self._is_path_conflict(rel_path, self.extras, "extras")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    self.saves.append(('dir', rel_path))
            self.update_saves_list()
        else:
            if item_subtype == 'file':
                file_path, _ = QFileDialog.getOpenFileName(self, "Sélectionner un fichier", self.game_dir)
                if file_path:
                    rel_path = os.path.relpath(file_path, self.game_dir)
                    # Vérifier si déjà présent dans extras
                    if ('file', rel_path) in self.extras:
                        QMessageBox.information(self, "Déjà présent", "Ce fichier est déjà dans la liste des extras.")
                        return
                    # Vérifier si présent dans saves
                    if ('file', rel_path) in self.saves:
                        QMessageBox.information(self, "Déjà présent", "Ce fichier est déjà dans la liste des sauvegardes.")
                        return
                    # Vérifier les conflits de chemins
                    conflict, msg = self._is_path_conflict(rel_path, self.extras, "extras")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    conflict, msg = self._is_path_conflict(rel_path, self.saves, "sauvegardes")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    self.extras.append(('file', rel_path))
            else:
                dir_path = QFileDialog.getExistingDirectory(self, "Sélectionner un dossier", self.game_dir)
                if dir_path:
                    rel_path = os.path.relpath(dir_path, self.game_dir)
                    # Vérifier si déjà présent dans extras
                    if ('dir', rel_path) in self.extras:
                        QMessageBox.information(self, "Déjà présent", "Ce dossier est déjà dans la liste des extras.")
                        return
                    # Vérifier si présent dans saves
                    if ('dir', rel_path) in self.saves:
                        QMessageBox.information(self, "Déjà présent", "Ce dossier est déjà dans la liste des sauvegardes.")
                        return
                    # Vérifier les conflits de chemins
                    conflict, msg = self._is_path_conflict(rel_path, self.extras, "extras")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    conflict, msg = self._is_path_conflict(rel_path, self.saves, "sauvegardes")
                    if conflict:
                        QMessageBox.warning(self, "Conflit de chemin", msg)
                        return
                    self.extras.append(('dir', rel_path))
            self.update_extras_list()
    
    def remove_item(self, item_type):
        """Supprime un élément de la liste"""
        if item_type == 'save':
            current = self.saves_list.currentRow()
            if current >= 0:
                del self.saves[current]
                self.update_saves_list()
        else:
            current = self.extras_list.currentRow()
            if current >= 0:
                del self.extras[current]
                self.update_extras_list()
    
    def update_saves_list(self):
        """Met à jour la liste des sauvegardes"""
        self.saves_list.clear()
        for item_type, path in self.saves:
            prefix = "[Dossier] " if item_type == 'dir' else "[Fichier] "
            self.saves_list.addItem(prefix + path)
    
    def update_extras_list(self):
        """Met à jour la liste des extras"""
        self.extras_list.clear()
        for item_type, path in self.extras:
            prefix = "[Dossier] " if item_type == 'dir' else "[Fichier] "
            self.extras_list.addItem(prefix + path)
    
    def start_create_wgp(self):
        """Démarre la création du WGP"""
        # Récupérer le nom du jeu
        game_name = self.name_input.text().strip()
        if not game_name:
            QMessageBox.warning(self, "Nom manquant", "Veuillez entrer un nom pour le paquet.")
            return
        
        # Vérifier l'exécutable
        if self.exe_list.currentRow() < 0:
            QMessageBox.warning(self, "Exécutable manquant", "Veuillez sélectionner un exécutable principal.")
            return
        
        # Configurer la création
        config = {
            'exe': self.exe_files[self.exe_list.currentRow()],
            'args': self.args_input.text(),
            'compression': self.comp_combo.currentIndex() * 5,  # 0, 5, 10, 15, 19
            'fix_controller': self.fix_checkbox.isChecked(),
            'saves': self.saves,
            'extras': self.extras,
            'icon': self.icon_path
        }
        
        wgp_file = os.path.join(os.path.dirname(self.game_dir), f"{game_name}.wgp")
        
        # Vérifier si le fichier existe
        if os.path.exists(wgp_file):
            reply = QMessageBox.question(
                self, "Fichier existant",
                f"{os.path.basename(wgp_file)} existe déjà. Voulez-vous l'écraser ?",
                QMessageBox.Yes | QMessageBox.No
            )
            if reply == QMessageBox.No:
                return
        
        # Lancer la création directement (pas de résumé)
        self.do_create_wgp(game_name, config)
    

        

    
    def do_create_wgp(self, game_name, config):
        """Effectue la création avec progression"""
        self.progress_dialog = QProgressDialog("Création du WGP...", "Annuler", 0, 100, self)
        self.progress_dialog.setWindowModality(Qt.WindowModal)
        self.progress_dialog.setMinimumDuration(0)
        self.progress_dialog.setValue(0)
        
        # Créer et lancer le thread
        self.create_thread = CreateWGPThread(self.game_dir, game_name, config)
        self.create_thread.progress.connect(self.on_progress)
        self.create_thread.finished.connect(self.on_finished)
        
        self.progress_dialog.canceled.connect(self.create_thread.cancel)
        
        self.create_thread.start()
    
    def on_progress(self, value, message):
        """Met à jour la progression"""
        self.progress_dialog.setValue(value)
        self.progress_dialog.setLabelText(message)
    
    def on_finished(self, success, message):
        """Appelé quand la création est terminée"""
        self.progress_dialog.close()
        
        if success:
            # Les fichiers ont déjà été restaurés par le thread via cleanup()
            # Pas besoin d'appeler restore_files() ici
            
            # Calculer les tailles et le ratio de compression
            wgp_file = message
            size_before = self.get_directory_size(self.game_dir)
            size_after = os.path.getsize(wgp_file)
            size_after_formatted = self.get_file_size(wgp_file)
            
            if size_before > 0:
                compression_ratio = (1 - size_after / size_before) * 100
                ratio_text = f"\nGain: {compression_ratio:.1f}%"
            else:
                ratio_text = ""
            
            QMessageBox.information(
                self, "Succès",
                f"WGP créé avec succès !\n\n"
                f"Fichier: {os.path.basename(wgp_file)}\n"
                f"Taille originale: {self.format_bytes(size_before)}\n"
                f"Taille compressée: {size_after_formatted}{ratio_text}"
            )
            self.close()
        else:
            QMessageBox.critical(self, "Erreur", message)
    
    def get_directory_size(self, path):
        """Calcule la taille totale d'un répertoire en octets"""
        total = 0
        for dirpath, dirnames, filenames in os.walk(path):
            # Ignorer les dossiers spéciaux
            dirnames[:] = [d for d in dirnames if d not in ['.save', '.extra', '__pycache__']]
            for f in filenames:
                fp = os.path.join(dirpath, f)
                if not os.path.islink(fp):
                    total += os.path.getsize(fp)
        return total
    
    def format_bytes(self, size):
        """Formate une taille en octets vers une chaîne lisible"""
        for unit in ['B', 'KB', 'MB', 'GB', 'TB']:
            if size < 1024.0:
                return f"{size:.2f} {unit}"
            size /= 1024.0
        return f"{size:.2f} PB"
    
    def get_file_size(self, filepath):
        """Retourne la taille du fichier formatée"""
        size = os.path.getsize(filepath)
        for unit in ['B', 'KB', 'MB', 'GB']:
            if size < 1024.0:
                return f"{size:.1f} {unit}"
            size /= 1024.0
        return f"{size:.1f} TB"


def main():
    app = QApplication(sys.argv)
    
    # Style sombre
    app.setStyleSheet("""
        QMainWindow {
            background-color: #1E1F26;
        }
        QWidget {
            background-color: #1E1F26;
            color: #FFFFFF;
        }
        QGroupBox {
            border: 1px solid #3C3C46;
            border-radius: 6px;
            margin-top: 10px;
            padding-top: 10px;
            font-weight: bold;
            color: #588CFF;
        }
        QGroupBox::title {
            subcontrol-origin: margin;
            left: 10px;
            padding: 0 5px;
        }
        QLineEdit, QListWidget, QComboBox {
            background-color: #2D2D33;
            border: 1px solid #3C3C46;
            border-radius: 4px;
            padding: 5px;
            color: #FFFFFF;
        }
        QLineEdit:focus, QListWidget:focus {
            border: 1px solid #588CFF;
        }
        QListWidget::item:selected {
            background-color: #588CFF;
        }
        QPushButton {
            background-color: #3C3C46;
            border: 1px solid #3C3C46;
            border-radius: 4px;
            padding: 5px 10px;
            color: #FFFFFF;
        }
        QPushButton:hover {
            background-color: #4A4A55;
            border-color: #588CFF;
        }
        QCheckBox {
            color: #B4B4BE;
        }
        QCheckBox::indicator {
            width: 18px;
            height: 18px;
            background-color: #2D2D33;
            border: 1px solid #3C3C46;
            border-radius: 3px;
        }
        QCheckBox::indicator:checked {
            background-color: #588CFF;
        }
        QLabel {
            color: #FFFFFF;
        }
    """)
    
    # Récupérer le dossier du jeu depuis les arguments
    game_dir = sys.argv[1] if len(sys.argv) > 1 else None
    
    window = WGPWindow(game_dir)
    
    # Définir l'icône de la fenêtre (applications-games)
    app_icon = QIcon.fromTheme("applications-games")
    if app_icon.isNull():
        # Fallback si l'icône système n'est pas trouvée
        app_icon = QIcon.fromTheme("applications-other")
    app.setWindowIcon(app_icon)
    window.setWindowIcon(app_icon)
    
    window.show()
    
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
