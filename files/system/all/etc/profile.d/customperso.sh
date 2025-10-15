# Personnal alias

alias gablue-help="cat /usr/share/ublue-os/gablue/gablue-help.txt"

alias goarch="distrobox enter archgab"

alias dlv-mp3="yt-dlp -x --format bestaudio --audio-format mp3 --audio-quality 0 --embed-thumbnail --embed-metadata --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s'"
alias dlv-mp4="yt-dlp --recode-video mp4 --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"
alias dlv-mkv="yt-dlp --recode-video mkv --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"

alias miniserve="podman kill miniserve; sleep 2; podman run -v ./:/share:Z --network host --name miniserve --rm --rmi -it docker.io/svenstaro/miniserve /share -r"

alias ffsend="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload -y"
alias ffsendp="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload --password -y"

alias gogpt="tgpt --provider sky -i"

alias gogpt-web="tgpt --provider kimi -i"

alias wkill="pgrep -i '(.*\\.(exe|dll|msi|bat|com|lnk)$)|(.*(wine|proton).*)' | xargs -n1 kill -9 ; pgrep -f bottles | xargs -I {} sh -c 'ps -p {} -o comm= | grep -q \"^bwrap$\" && kill -9 {}'"

alias wrun="/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable"

alias wtricks="flatpak run --env=WINEPREFIX=/var/data/bottles/bottles/def --env=WINE=/var/data/bottles/runners/gwine-10.2.r9/bin/wine --command=winetricks com.usebottles.bottles"

alias wwayland='flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_CURRENT_USER\Software\Wine\Drivers" -v "Graphics" -d "wayland" -t "REG_SZ"'

alias wx11='flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_CURRENT_USER\Software\Wine\Drivers" -v "Graphics" -d "X11" -t "REG_SZ"'

alias neofetch="fastfetch --logo /usr/share/ublue-os/gablue/logoascii.ans -c /usr/share/ublue-os/gablue/fastfetch.jsonc"

alias ollama-start="podman start ollama open-webui && echo 'Ollama WebGui http://localhost:3000'"

alias ollama-restart="podman restart ollama open-webui"

alias ollama-stop="podman stop ollama open-webui"

alias open="xdg-open &>/dev/null"

alias battery-info="upower -i /org/freedesktop/UPower/devices/battery_BAT0"

alias sherlock="podman run --rm docker.io/sherlock/sherlock:latest"

alias fkill="ps aux | grep 'bwrap' | grep -v 'SyncThingy' | grep -v 'xdg-dbus-proxy' | grep -v 'grep' | awk '{print $2}' | xargs kill -9"

alias gablue-update="ujust gablue-update"

alias gablue-switch-main="rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-main:latest"

alias gablue-switch-nvidia-open="rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia-open:latest"

alias gablue-switch-nvidia="rpm-ostree rebase ostree-image-signed:docker://ghcr.io/elgabo86/gablue-nvidia:latest"

alias vrr-on="kscreen-doctor output.1.vrrpolicy.automatic"

alias vrr-off="kscreen-doctor output.1.vrrpolicy.never"

alias watchdir='function _watchdir() { local dir="${1:-$PWD}"; inotifywait -m "$dir" -r -e modify,create,delete,move --format "%w%f %e %T" --timefmt "%F %T" | tee -a /tmp/watchdir.log; }; _watchdir'

alias a13-up="adb shell wm user-rotation lock 0"
alias a13-down="adb shell wm user-rotation lock 2"
alias a13-left="adb shell wm user-rotation lock 3"
alias a13-right="adb shell wm user-rotation lock 1"

alias tp="trash-put"

# Ignore duplicate
HISTCONTROL=ignoredups

# Init pyenv
if [ -x ~/.pyenv/bin/pyenv ]; then
	export PYENV_ROOT="$HOME/.pyenv"
	command -v pyenv >/dev/null || export PATH="$PYENV_ROOT/bin:$PATH"
	eval "$(pyenv init -)"
fi
