 
# fr_CH fix
export LANG=fr_CH.UTF-8

# Personnal alias
alias goarch="distrobox enter arch"
alias goarchroot="distrobox enter --root arch"
alias renewarch="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"
alias renewarchroot="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"
alias renewarchnvidia="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"
alias renewarchrootnvidia="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"

alias flatpak-fix-overrides="sudo flatpak override --filesystem=xdg-config/gtk-3.0:ro && sudo flatpak override --filesystem=xdg-config/MangoHud:ro && sudo flatpak override --filesystem=xdg-config/gtk-4.0:ro && sudo flatpak override --env=OBS_VKCAPTURE=1"

# Ignore duplicate
HISTCONTROL=ignoredups
