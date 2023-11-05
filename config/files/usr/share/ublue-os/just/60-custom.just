#!include /usr/share/ublue-os/just/100-bling.just

# Include some of your custom scripts here!

tailscale-up:
  sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
  sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
  sudo tailscale up --login-server https://headscale.gabserv.duckdns.org

distrobox-archgab:
  podman kill --all
  distrobox-rm -f archgab
  distrobox-create --pull latest --name archgab --home ~/.archgab --image ghcr.io/elgabo86/archgab:latest

distrobox-archgab-nvidia:
  podman kill --all
  distrobox-rm -f archgab
  distrobox-create --pull latest --name archgab --home ~/.archgab --nvidia --image ghcr.io/elgabo86/archgab:latest

gablue-rebase-main:
  rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

gablue-rebase-nvidia:
  rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

flatpak-custom-overrides:
  flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
  flatpak override --user --filesystem=xdg-config/MangoHud:ro
  flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
  flatpak override --user --env=OBS_VKCAPTURE=1
  mkdir ~/.local/share/applications
  flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
  flatpak override com.usebottles.bottles --user --filesystem=/run/media

amd-corectrl-set-kargs:
  echo 'Setting needed kargs for CoreCtrl...'
  rpm-ostree kargs --append="amdgpu.ppfeaturemask=0xffffffff"

