#!/usr/bin/bash

set -eoux pipefail

#Tools-cli
rpm-ostree install \
fswatch \
iotop-c \
wol \
duperemove \
atuin \
speedtest-cli \
yt-dlp \
unrar \
btop \
tailscale \
intel-undervolt \
ncdu \
mc \
lm_sensors \
fastfetch \
stress-ng \
powertop \
android-tools \
byobu \
cabextract \
ripgrep \
testdisk \
gh \
ryzenadj \
yq

#Goodies cli
rpm-ostree install \
bash-color-prompt \
asciiquarium \
figlet \
toilet \
cmatrix \
nyancat

#Tools
rpm-ostree install \
meld \
sunshine \
cpu-x \
corectrl \
solaar \
gnome-disk-utility \
skanpage \
x2goclient \
goverlay \
mangohud \
cool-retro-term \
gamescope \
cage \
wlr-randr \
scx-scheds \
joystickwake \
earlyoom

#Kde addons
if [ "$SOURCE_IMAGE" == "kinoite" ]; then
    rpm-ostree install \
    okular \
    gwenview \
    kcalc \
    yakuake \
    papirus-icon-theme \
    langpacks-fr \
    kdenetwork-filesharing \
    libadwaita-qt5 \
    libadwaita-qt6 \
    kde-runtime-docs \
    kdeplasma-addons \
    kde-material-you-colors \
    krdp \
    nerd-fonts \
    adobe-source-code-pro-fonts \
    google-droid-sans-mono-fonts \
    google-go-mono-fonts \
    breeze-gtk
fi

#Drivers
rpm-ostree install \
epson-inkjet-printer-escpr \
epson-inkjet-printer-escpr2 \
hplip \
printer-driver-brlaser \
ifuse \
libimobiledevice \

#Dev dep
rpm-ostree install \
gcc-c++ \
libffi-devel \
readline-devel \
zlib-devel \
bzip2-devel \
openssl-devel \
sqlite-devel \
xz-devel \
tk-devel \
patch \
bzip2 \
sqlite \
libuuid-devel \
gdbm-libs \
libnsl2

#Remove firefox
rpm-ostree override remove \
firefox \
firefox-langpacks

if [ "$GABLUE_VARIANT" == "main" ]; then
    rpm-ostree install \
    radeontop \
    waydroid
fi

if [ "$SOURCE_IMAGE" == "cosmic" ]; then
    rpm-ostree install \
    xdg-user-dirs
fi
