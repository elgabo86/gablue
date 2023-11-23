# !include 100-bling.just
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

mitigations-off:
  rpm-ostree kargs --append-if-missing=mitigations=off

gablue-setup:
  flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
  flatpak override --user --filesystem=xdg-config/MangoHud:ro
  flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
  flatpak override --user --env=OBS_VKCAPTURE=1
  mkdir ~/.local/share/applications
  flatpak override com.usebottles.bottles --user --filesystem=xdg-data/applications
  flatpak override com.usebottles.bottles --user --filesystem=/run/media
  rpm-ostree kargs --append-if-missing=mitigations=off

get-sunshine:
  wget \
    $(curl -s https://api.github.com/repos/LizardByte/Sunshine/releases/latest | \
    jq -r ".assets[] | select(.name | test(\"sunshine_x86_64.flatpak\")) | .browser_download_url") \
    -O /tmp/sunshine.flatpak
  sudo flatpak install --system /tmp/sunshine.flatpak -y
  sudo mkdir -p /var/usrlocal/systemd/system
  echo -e \
  "[Unit]\n\
  Description=Sunshine is a self-hosted game stream host for Moonlight.\n\
  StartLimitIntervalSec=500\n\
  StartLimitBurst=5\n\
  \n\
  [Service]\n\
  Environment=PULSE_SERVER=unix:/run/user/1000/pulse/native\n\
  ExecStart=flatpak run dev.lizardbyte.sunshine\n\
  ExecStop=flatpak kill dev.lizardbyte.sunshine\n\
  Restart=on-failure\n\
  RestartSec=5s\n\
  \n\
  [Install]\n\
  WantedBy=graphical.target" | \
  sudo tee /etc/systemd/system/sunshine.service
  sudo systemctl enable sunshine --now
  echo "Sunshine is installed!"

remove-sunshine:
  sudo systemctl disable sunshine.service
  sudo systemctl stop sunshine.service
  sudo flatpak remove --system dev.lizardbyte.sunshine -y
  sudo rm /etc/systemd/system/sunshine.service
  echo "Sunshine successfully deinstalled."
