extends RefCounted

class_name WGPGame

# Propriétés du jeu
var name: String = ""
var path: String = ""
var launch_command: String = ""

func _init(game_name: String, game_path: String):
    name = game_name
    path = game_path
    # La commande de lancement est simplement le script lui-même
    launch_command = game_path

func get_name() -> String:
    return name

func get_launch_command() -> String:
    return launch_command

func get_path() -> String:
    return path
