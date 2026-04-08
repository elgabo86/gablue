extends Node

class_name GwinePlugin

const PLUGIN_ID = "gwine"
const PLUGIN_NAME = "Gwine WGP Games"

var library_provider: GwineLibraryProvider

func _ready():
    # Initialiser le provider de bibliothèque
    library_provider = GwineLibraryProvider.new()
    library_provider.name = "GwineProvider"
    add_child(library_provider)
    
    # Enregistrer le provider auprès du LibraryManager
    var library_manager = get_node_or_null("/root/Main/LibraryManager")
    if library_manager:
        library_manager.register_library_provider(library_provider)
        print("Gwine plugin: Provider enregistré avec succès")
    else:
        push_error("Gwine plugin: Impossible de trouver le LibraryManager")

func _exit_tree():
    # Désenregistrer le provider lors de la fermeture
    var library_manager = get_node_or_null("/root/Main/LibraryManager")
    if library_manager and library_provider:
        library_manager.unregister_library_provider(library_provider)
