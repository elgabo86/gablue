#!/bin/bash

fullpath="$1"

onlypath=$(dirname "$fullpath")

onlyapp=$(basename "$fullpath" .exe)

echo "#!/bin/bash" > "$onlypath"/"$onlyapp".sh
echo "/usr/bin/flatpak run --branch=stable --arch=x86_64 --command=bottles-cli --file-forwarding com.usebottles.bottles run --bottle def --executable '$fullpath'" >> "$onlypath"/"$onlyapp".sh

chmod +x "$onlypath"/"$onlyapp".sh
