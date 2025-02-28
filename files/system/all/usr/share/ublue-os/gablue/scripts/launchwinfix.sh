#!/bin/bash
fullpath="$1"
waitwine flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus" -v "DisableHidraw" -d 0 -t "REG_DWORD" &&
/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable "$fullpath" ;
waitwine flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus" -v "DisableHidraw" -d 1 -t "REG_DWORD"
