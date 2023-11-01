#!include /usr/share/ublue-os/just/100-bling.just

# Include some of your custom scripts here!

tailscale-up:
  sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent
  sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent
  sudo tailscale up --login-server https://headscale.gabserv.duckdns.org

distrobox-archgab:
  podman kill --all
  distrobox-rm -f archgab
  distrobox-create --pull latest --name archgab --image ghcr.io/elgabo86/archgab:latest
  distrobox enter archgab

distrobox-archgab-nvidia:
  podman kill --all
  distrobox-rm -f archgab
  distrobox-create --pull latest --name archgab --nvidia --image ghcr.io/elgabo86/archgab:latest
  distrobox enter archgab

gablue-rebase-main:
  rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest

gablue-rebase-nvidia:
  rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest

flatpak-custom-overrides:
  flatpak override --user --filesystem=xdg-config/gtk-3.0:ro
  flatpak override --user --filesystem=xdg-config/MangoHud:ro
  flatpak override --user --filesystem=xdg-config/gtk-4.0:ro
  flatpak override --user --env=OBS_VKCAPTURE=1

amd-corectrl-set-kargs:
  echo 'Setting needed kargs for CoreCtrl...'
  rpm-ostree kargs --append="amdgpu.ppfeaturemask=0xffffffff"

waydroid-reset:
  bash -c 'sudo rm -rf /var/lib/waydroid /home/.waydroid ~/waydroid ~/.share/waydroid ~/.local/share/applications/*aydroid* ~/.local/share/waydroid'

waydroid-init:
  #!/usr/bin/env bash
  sudo waydroid init -c 'https://ota.waydro.id/system' -v 'https://ota.waydro.id/vendor'

waydroid-enable:
  sudo systemctl enable --now waydroid-container.service

waydroid-disable:
  sudo systemctl disable --now waydroid-container.service

waydroid-configure:
  #!/usr/bin/env bash
  git clone https://github.com/casualsnek/waydroid_script.git --depth 1 /tmp/waydroid_script
  python -m venv /tmp/waydroid_script/venv
  source /tmp/waydroid_script/venv/bin/activate
  sudo pip install -r /tmp/waydroid_script/requirements.txt
  sudo /tmp/waydroid_script/main.py
  deactivate
  sudo rm -rf /tmp/waydroid_script

waydroid-multiwindows-enable:
  waydroid prop set persist.waydroid.multi_windows true

waydroid-multiwindows-disable:
  waydroid prop set persist.waydroid.multi_windows false



