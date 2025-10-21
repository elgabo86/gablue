# Fix hdmi/dp/usb audio drop
audio-fix-enable:
    #!/usr/bin/bash
    mkdir -p ~/.config/wireplumber/wireplumber.conf.d/ &&
    cp -f /usr/share/ublue-os/gablue/fixes/50-sound-drop-fix.conf ~/.config/wireplumber/wireplumber.conf.d/ &&
    systemctl restart --user pipewire.socket wireplumber.service &&
    echo "Audio fix is installed"

# Disable fix for hdmi/dp/usb audio drop
audio-fix-disable:
    #!/usr/bin/bash
    rm -f ~/.config/wireplumber/wireplumber.conf.d/50-sound-drop-fix.conf &&
    systemctl restart --user pipewire.socket wireplumber.service &&
    echo "Audio fix is removed"
