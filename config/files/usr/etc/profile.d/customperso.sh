# Personnal alias
alias goarch="distrobox enter arch"
alias goarchroot="distrobox enter --root arch"
alias renewarch="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"
alias renewarchroot="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"
alias renewarchnvidia="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"
alias renewarchrootnvidia="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"

alias flatpak-fix-overrides="flatpak override --user --filesystem=xdg-config/gtk-3.0:ro && flatpak override --user --filesystem=xdg-config/MangoHud:ro && flatpak override --user --filesystem=xdg-config/gtk-4.0:ro && flatpak override --user --env=OBS_VKCAPTURE=1"

alias nvidia-fix-kargs="rpm-ostree kargs --append=rd.driver.blacklist=nouveau --append=modprobe.blacklist=nouveau --append=nvidia-drm.modeset=1"

alias tailscale-init="sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent && sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent && sudo systemctl enable --now tailscaled.service"

alias tailscale-gab="sudo tailscale up  --login-server https://headscale.gabserv.duckdns.org"

# Ignore duplicate
HISTCONTROL=ignoredups

