#!/usr/bin/env bash
# Script pour charger l'environnement brew dans les shells non interactifs
if [ -d "/home/linuxbrew/.linuxbrew" ]; then
    eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"
fi