# Personnal alias

alias goarch="distrobox enter arch"

alias goarch-root="distrobox enter --root arch"

alias gablue-tailscale-init="sudo firewall-cmd --add-masquerade --zone=FedoraWorkstation --permanent && sudo firewall-cmd --add-interface=tailscale0 --zone=trusted --permanent && sudo tailscale up --login-server https://headscale.gabserv.duckdns.org"

# Ignore duplicate
HISTCONTROL=ignoredups

