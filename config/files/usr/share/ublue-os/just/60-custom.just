#!include /usr/share/ublue-os/just/100-bling.just

# Include some of your custom scripts here!

tailscale-init:
  sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
  sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
  sudo tailscale up --login-server https://headscale.gabserv.duckdns.org

arch-reset:
  podman kill --all
  distrobox-rm -f arch
  distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest
  distrobox enter arch

arch-reset-root:
  sudo podman kill --all
  distrobox-rm -f --root arch
  distrobox-create --pull latest --name arch --image ghcr.io/elgabo86/archgab:latest --root
  distrobox enter --root arch

arch-nvidia-reset:
  podman kill --all
  distrobox-rm -f arch
  distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest
  distrobox enter arch

arch-nvidia-reset-root:
  sudo podman kill --all
  distrobox-rm -f --root arch
  distrobox-create --pull latest --name arch --nvidia --image ghcr.io/elgabo86/archgab:latest --root
  distrobox enter --root arch

rebase-fix-main:
  rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

rebase-fix-nvidia:
  rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

flatpak-custom-overrides:
  flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
  flatpak override --user --filesystem=xdg-config/MangoHud:ro
  flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
  flatpak override --user --env=OBS_VKCAPTURE=1
