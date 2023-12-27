# Personnal alias

alias goarch="distrobox enter archgab"

alias dlv-mp3="yt-dlp -x --format bestaudio --audio-format mp3 --embed-thumbnail --embed-metadata --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s'"
alias dlv-mp4="yt-dlp --format best --recode-video mp4 --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"
alias dlv-mkv="yt-dlp --format best --recode-video mkv --paths $(xdg-user-dir DOWNLOAD) -o '%(title)s.%(ext)s' --embed-subs --sub-langs all --embed-thumbnail"

# Ignore duplicate
HISTCONTROL=ignoredups

