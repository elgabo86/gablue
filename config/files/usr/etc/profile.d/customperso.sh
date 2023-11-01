# Personnal alias

alias goarch="distrobox enter arch"

alias goarch-root="distrobox enter --root arch"

alias gablue-arch-reset="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"

alias gablue-arch-reset-root="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"

alias gablue-arch-nvidia-reset="podman kill --all && distrobox-rm -f arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest && distrobox enter arch"

alias gablue-arch-nvidia-reset-root="sudo podman kill --all && distrobox-rm -f --root arch && distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest --root && distrobox enter --root arch"

alias gablue-flatpak-fix-overrides="flatpak override --user --filesystem=xdg-config/gtk-3.0:ro && flatpak override --user --filesystem=xdg-config/MangoHud:ro && flatpak override --user --filesystem=xdg-config/gtk-4.0:ro && flatpak override --user --env=OBS_VKCAPTURE=1"

alias gablue-nvidia-fix-kargs="rpm-ostree kargs --append=rd.driver.blacklist=nouveau --append=modprobe.blacklist=nouveau --append=nvidia-drm.modeset=1"

alias gablue-tailscale-init="sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent && sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent && sudo tailscale up --login-server https://headscale.gabserv.duckdns.org"

alias gablue-rebase-fix="rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest"

alias gablue-rebase-nvidia-fix="rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest"




# Ignore duplicate
HISTCONTROL=ignoredups

