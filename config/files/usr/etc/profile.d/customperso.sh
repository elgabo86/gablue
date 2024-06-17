# Personnal alias

alias goarch="distrobox enter archgab"

alias dlv-mp3="yt-dlp -x --format bestaudio --audio-format mp3 --audio-quality 0 --embed-thumbnail --embed-metadata --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s'"
alias dlv-mp4="yt-dlp --recode-video mp4 --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"
alias dlv-mkv="yt-dlp --recode-video mkv --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"

alias miniserve="podman run -v ./:/share:Z --network host --rm --rmi -it docker.io/svenstaro/miniserve /share -r"

alias ffsend="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload -y"
alias ffsendp="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload --password -y"

alias gogpt="tgpt --provider opengpts -i"
alias gogpt-phind="tgpt --provider phind -i"
alias gogpt-koboldai="tgpt --provider koboldai -i"
alias gogpt-llama2="tgpt --provider llama2 -i"
alias gogpt-blackboxai="tgpt --provider blackboxai -i"


alias wkill="pgrep -i '(.*\.(exe|dll|msi|bat|com|lnk)$)|(.*(wine|proton).*))' |xargs -n1 kill -9"

alias neofetch="fastfetch -c /usr/share/ublue-os/gablue/fastfetch.jsonc"

# Ignore duplicate
HISTCONTROL=ignoredups

# Init pyenv
if [ -x ~/.pyenv/bin/pyenv ]; then
	export PYENV_ROOT="$HOME/.pyenv"
	command -v pyenv >/dev/null || export PATH="$PYENV_ROOT/bin:$PATH"
	eval "$(pyenv init -)"
fi
