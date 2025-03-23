#!/bin/bash

fullpath="$1"

onlypath=$(dirname "$fullpath")

onlyapp=$(basename "$fullpath" .exe)

echo "#!/bin/bash" > "$onlypath"/"$onlyapp".sh
echo "sed -i 's/\"DisableHidraw\"=dword:00000001/\"DisableHidraw\"=dword:00000000/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$onlypath/$onlyapp.sh"
echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable '$fullpath' ;" >> "$onlypath"/"$onlyapp".sh
echo "sed -i 's/\"DisableHidraw\"=dword:00000000/\"DisableHidraw\"=dword:00000001/' ~/.var/app/com.usebottles.bottles/data/bottles/bottles/def/system.reg" >> "$onlypath/$onlyapp.sh"

chmod +x "$onlypath"/"$onlyapp".sh
