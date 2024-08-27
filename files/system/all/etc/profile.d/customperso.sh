# Personnal alias

alias gablue-help="cat /usr/share/ublue-os/gablue/gablue-help.txt"

alias goarch="distrobox enter archgab"

alias dlv-mp3="yt-dlp -x --format bestaudio --audio-format mp3 --audio-quality 0 --embed-thumbnail --embed-metadata --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s'"
alias dlv-mp4="yt-dlp --recode-video mp4 --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"
alias dlv-mkv="yt-dlp --recode-video mkv --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"

alias miniserve="podman run -v ./:/share:Z --network host --rm --rmi -it docker.io/svenstaro/miniserve /share -r"

alias ffsend="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload -y"
alias ffsendp="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload --password -y"

alias gogpt="tgpt --provider duckduckgo -i"

alias gogpt-pro="tgpt --provider phind -i"

alias wkill="pgrep -i '(.*\.(exe|dll|msi|bat|com|lnk)$)|(.*(wine|proton).*))' |xargs -n1 kill -9 && pgrep -f bottles |xargs -n1 kill -9"

alias wrun="flatpak run --command=bottles-cli com.usebottles.bottles run -b def -e"

alias wtricks="flatpak run --env=WINEPREFIX=/var/data/bottles/bottles/def --env=WINE=/var/data/bottles/runners/soda-9.0-1/bin/wine --command=winetricks com.usebottles.bottles"

alias neofetch="fastfetch -c /usr/share/ublue-os/gablue/fastfetch.jsonc"

alias gablue-update="/usr/libexec/gablue-update"

alias ollama-start="podman start ollama open-webui && echo 'Ollama WebGui http://localhost:3000'"

alias ollama-restart="podman restart ollama open-webui"

alias ollama-stop="podman stop ollama open-webui"

alias open="xdg-open &>/dev/null"

alias battery-info="upower -i /org/freedesktop/UPower/devices/battery_BAT0"

alias sherlock="podman run --rm docker.io/sherlock/sherlock:latest"

# Ignore duplicate
HISTCONTROL=ignoredups

# Init pyenv
if [ -x ~/.pyenv/bin/pyenv ]; then
	export PYENV_ROOT="$HOME/.pyenv"
	command -v pyenv >/dev/null || export PATH="$PYENV_ROOT/bin:$PATH"
	eval "$(pyenv init -)"
fi
