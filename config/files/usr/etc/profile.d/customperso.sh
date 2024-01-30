# Personnal alias

alias goarch="distrobox enter archgab"

alias dlv-mp3="yt-dlp -x --format bestaudio --audio-format mp3 --audio-quality 0 --embed-thumbnail --embed-metadata --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s'"
alias dlv-mp4="yt-dlp --recode-video mp4 --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"
alias dlv-mkv="yt-dlp --recode-video mkv --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"

alias miniserve="podman run -v ./:/share:Z --network host --rm --rmi -it docker.io/svenstaro/miniserve /share"

alias ffsend="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload -y"
alias ffsendp="podman run --rm --rmi -it -v $(pwd):/data:Z docker.io/timvisee/ffsend:latest upload --password -y"

alias gogpt="tgpt -i"

# Ignore duplicate
HISTCONTROL=ignoredups

