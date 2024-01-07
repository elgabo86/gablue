# Personnal alias

alias goarch="distrobox enter archgab"

alias dlv-mp3="yt-dlp -x --format bestaudio --audio-format mp3 --audio-quality 0 --embed-thumbnail --embed-metadata --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s'"
alias dlv-mp4="yt-dlp --recode-video mp4 --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"
alias dlv-mkv="yt-dlp --recode-video mkv --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"

alias miniserve="podman run -v ./:/share:Z -p 8080:8080 --rm --rmi -it docker.io/svenstaro/miniserve /share"

# Ignore duplicate
HISTCONTROL=ignoredups

