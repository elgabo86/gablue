#!/usr/bin/bash

set -eoux pipefail

#Disable multilib repo to avoid i686 packages
sed -i 's@enabled=1@enabled=0@g' /etc/yum.repos.d/_copr_kylegospo-bazzite-multilib.repo

#Tools-cli
rpm-ostree install \
fswatch \
iotop-c \
wol \
duperemove \
atuin \
speedtest-cli \
yt-dlp \
rar \
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
yq \
trash-cli

#Goodies cli
rpm-ostree install \
bash-color-prompt \
asciiquarium \
figlet \
toilet \
cmatrix

#Tools
rpm-ostree install \
meld \
sunshine \
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
    papirus-icon-theme-dark \
    langpacks-fr \
    kdenetwork-filesharing \
    kde-runtime-docs \
    kdeplasma-addons
fi

#Fonts
rpm-ostree install \
nerd-fonts \
adobe-source-code-pro-fonts \
google-droid-sans-mono-fonts \
google-go-mono-fonts

#Drivers
rpm-ostree install \
epson-inkjet-printer-escpr \
epson-inkjet-printer-escpr2 \
hplip \
printer-driver-brlaser \
ifuse \
libimobiledevice

#Dev dep
rpm-ostree install \
gcc \
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

#Python
rpm-ostree install \
python3-pygame \
python3-typing-extensions


#Remove firefox
rpm-ostree override remove \
firefox \
firefox-langpacks

if [ "$GABLUE_VARIANT" == "main" ]; then
    rpm-ostree install \
    radeontop
fi

#Fix power-profile-daemon
if [ "$OS_VERSION" == "40" ]; then
    rpm-ostree override replace \
    --experimental \
    https://kojipkgs.fedoraproject.org//packages/power-profiles-daemon/0.21/3.fc40/x86_64/power-profiles-daemon-0.21-3.fc40.x86_64.rpm
fi
