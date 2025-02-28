#!/bin/bash

fullpath="$1"

onlypath=$(dirname "$fullpath")

onlyapp=$(basename "$fullpath" .exe)

echo "#!/bin/bash" > "$onlypath"/"$onlyapp".sh
echo 'flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus" -v "DisableHidraw" -d 0 -t "REG_DWORD" &&'   >> "$onlypath"/"$onlyapp".sh
echo "sleep 0.5 &&" >> "$onlypath"/"$onlyapp".sh
echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable '$fullpath' ;" >> "$onlypath"/"$onlyapp".sh
echo 'flatpak run --command=bottles-cli com.usebottles.bottles reg add -b def -k "HKEY_LOCAL_MACHINE\System\CurrentControlSet\Services\winebus" -v "DisableHidraw" -d 1 -t "REG_DWORD"'  >> "$onlypath"/"$onlyapp".sh

chmod +x "$onlypath"/"$onlyapp".sh
