 
# fr_CH fix
export LANG=fr_CH.UTF-8

# Personnal alias
alias goarch="distrobox enter arch"
alias goarchroot="distrobox enter --root arch"
alias renewarch="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"
alias renewarchroot="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"
alias renewarchnvidia="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"
alias renewarchrootnvidia="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"

alias flatpak-fix-overrides="flatpak override --user --filesystem=xdg-config/gtk-3.0:ro && flatpak override --user --filesystem=xdg-config/MangoHud:ro && flatpak override --user --filesystem=xdg-config/gtk-4.0:ro && flatpak override --user --env=OBS_VKCAPTURE=1"

# Ignore duplicate
HISTCONTROL=ignoredups

# Pyenv control
if grep -q 'ID=arch' /etc/os-release; then
    export PYENV_ROOT="$HOME/.pyenv"
	command -v pyenv >/dev/null || export PATH="$PYENV_ROOT/bin:$PATH"
	eval "$(pyenv init -)"
fi
