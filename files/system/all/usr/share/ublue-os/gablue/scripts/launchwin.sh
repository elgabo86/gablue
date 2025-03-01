#!/bin/bash
fullpath="$1"

if ! pgrep -x "wineserver" > /dev/null; then
    waitwine flatpak run --command=bash com.usebottles.bottles -c "bottles-cli reg add -b def -k 'HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus' -v 'DisableHidraw' -d 1 -t 'REG_DWORD'"
fi

/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$fullpath"
