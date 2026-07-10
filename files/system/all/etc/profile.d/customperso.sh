# Personal alias

alias gablue-help="cat /usr/share/ublue-os/gablue/gablue-help.txt"

alias goarch="distrobox enter archgab"



alias miniserve="podman kill miniserve 2>/dev/null; sleep 1; podman run -v ./:/share:Z -p 8080:8080 --name miniserve --rm --rmi docker.io/svenstaro/miniserve /share -r --header 'Cache-Control: no-store'"

ffsend() { podman run --rm --rmi -it -v "$(pwd):/data:Z" docker.io/timvisee/ffsend:latest upload -y "$@"; }
ffsendp() { podman run --rm --rmi -it -v "$(pwd):/data:Z" docker.io/timvisee/ffsend:latest upload --password -y "$@"; }

alias neofetch="fastfetch --logo /usr/share/ublue-os/gablue/logoascii.ans -c /usr/share/ublue-os/gablue/fastfetch.jsonc"

alias open="xdg-open &>/dev/null"

alias battery-info="upower -i /org/freedesktop/UPower/devices/battery_BAT0"

alias sherlock="podman run --rm docker.io/sherlock/sherlock:latest"

alias fkill="ps aux | grep 'bwrap' | grep -v 'SyncThingy' | grep -v 'xdg-dbus-proxy' | grep -v 'grep' | awk '{print $2}' | xargs kill -9"

alias gablue-update="ujust gablue-update"

alias vrr-on="kscreen-doctor output.1.vrrpolicy.automatic"

alias vrr-off="kscreen-doctor output.1.vrrpolicy.never"

watchdir() { local dir="${1:-$PWD}"; inotifywait -m "$dir" -r -e modify,create,delete,move --format "%w%f %e %T" --timefmt "%F %T" | tee -a /tmp/watchdir.log; }

alias tp="trash-put"

alias changefps="/usr/share/ublue-os/gablue/scripts/gamepadshortcuts/changefps"

alias opencode-install="curl -fsSL https://opencode.ai/install | bash"
alias hermes-install="curl -fsSL https://hermes-agent.nousresearch.com/install.sh | bash"

# Ignore duplicate
HISTCONTROL=ignoredups

