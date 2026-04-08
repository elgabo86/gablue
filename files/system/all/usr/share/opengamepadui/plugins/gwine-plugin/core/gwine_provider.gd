extends Node

class_name GwineLibraryProvider

signal library_updated

const WGP_DIR = "/home/gab/Roms/windows"
const SUPPORTED_EXTENSIONS = ["sh", "wgp"]

var games: Array[WGPGame] = []

func _ready():
    scan_games()

func scan_games() -> void:
    games.clear()
    
    var dir = DirAccess.open(WGP_DIR)
    if dir == null:
        push_error("Gwine provider: Impossible d'ouvrir le répertoire " + WGP_DIR)
        return
    
    dir.list_dir_begin()
    var file_name = dir.get_next()
    
    while file_name != "":
        if not dir.current_is_dir():
            var extension = file_name.get_extension().to_lower()
            if extension in SUPPORTED_EXTENSIONS:
                var full_path = WGP_DIR.path_join(file_name)
                var game_name = file_name.get_basename()
                
                # Créer l'objet jeu
                var game = WGPGame.new(game_name, full_path)
                games.append(game)
                
                print("Gwine provider: Jeu trouvé - " + game_name)
        
        file_name = dir.get_next()
    
    dir.list_dir_end()
    emit_signal("library_updated")

func get_games() -> Array[WGPGame]:
    return games

func get_game_count() -> int:
    return games.size()

func refresh_library() -> void:
    scan_games()
