# Install ble and atuin, for history bash sync and more
atuin-setup:
    #!/usr/bin/bash
    curl -L -o ble-nightly.tar.xz https://github.com/akinomyoga/ble.sh/releases/download/nightly/ble-nightly.tar.xz &&
    tar xJf ble-nightly.tar.xz &&
    bash ble-nightly/ble.sh --install ~/.local/share &&
    rm -rf ble-nightly &&
    rm -f ble-nightly.tar.xz &&
    echo 'source ~/.local/share/blesh/ble.sh' >> ~/.bashrc &&
    echo 'eval "$(atuin init bash --disable-up-arrow)"' >> ~/.bashrc &&
    echo "Atuin is ready!" ||
    echo "Error..."

# Install pyenv
pyenv-setup:
    #!/usr/bin/bash
    git clone https://github.com/pyenv/pyenv.git ~/.pyenv &&
    cd ~/.pyenv && src/configure && make -C src &&
    echo "ALL DONE!!!"

# Update pyenv
pyenv-update:
    #!/usr/bin/bash
    git -C ~/.pyenv pull &&
    echo "ALL DONE!!!"

# Remove pyenv
pyenv-remove:
    #!/usr/bin/bash
    rm -rf ~/.pyenv &&
    echo "ALL DONE!!!"

# Reset brew installation
brew-reset:
    #!/usr/bin/bash
    if [[ -d "${HOME}/cache/Homebrew/" ]]; then
        echo "Removing '$HOME/cache/Homebrew/' directory"
        rm -r "${HOME}/cache/Homebrew/"
    else
        echo "'${HOME}/cache/Homebrew/' directory is already removed"
    fi
    if [[ -d "/var/lib/homebrew/" ]]; then
        echo "Removing '/var/lib/homebrew/' directory"
        sudo rm -rf "/var/lib/homebrew/"
    else
        echo "'/var/lib/homebrew/' directory is already removed"
    fi
    if [[ -d "/var/cache/homebrew/" ]]; then
        echo "Removing '/var/cache/homebrew/' directory"
        sudo rm -rf "/var/cache/homebrew/"
    else
        echo "'/var/cache/homebrew/' directory is already removed"
    fi
    ## This is the main directory where brew is located
    if [[ -d "/var/home/linuxbrew/" ]]; then
        echo "Removing '/home/linuxbrew/' directory"
        sudo rm -rf "/var/home/linuxbrew/"
    else
        echo "'/home/linuxbrew/' directory is already removed"
    fi
    if [[ -f "/etc/.linuxbrew" ]]; then
        echo "Removing empty '/etc/.linuxbrew' file"
        sudo rm -f "/etc/.linuxbrew"
    else
        echo "'/etc/.linuxbrew' file is already removed"
    fi
    echo "ALL DONE!!!"
    echo "Restart computer to reuse brew"

# Install Qwen
qwen-install:
    #!/usr/bin/bash
    qwen-install
    echo "Qwen has been installed!"

# Uninstall Qwen
qwen-uninstall:
    #!/usr/bin/bash
    echo "Removing qwen-code via Homebrew..."
    brew uninstall qwen-code
    echo "Removing qwen.desktop from applications folder..."
    rm -f ~/.local/share/applications/qwen.desktop
    echo "Update desktop database..."
    update-desktop-database ~/.local/share/applications
    echo "Qwen has been uninstalled!"

# Install VSCodium
vscodium-install:
    #!/usr/bin/bash
    echo "Installing VSCodium via Homebrew..."
    brew tap ublue-os/tap
    brew install ublue-os/tap/vscodium-linux --cask
    echo "VSCodium has been installed!"

# Uninstall VSCodium
vscodium-uninstall:
    #!/usr/bin/bash
    echo "Removing VSCodium via Homebrew..."
    brew uninstall ublue-os/tap/vscodium-linux --cask
    echo "VSCodium has been uninstalled!"

# Install VSCode
vscode-install:
    #!/usr/bin/bash
    echo "Installing VSCodium via Homebrew..."
    brew tap ublue-os/tap
    brew install ublue-os/tap/visual-studio-code-linux --cask
    echo "VSCode has been installed!"

# Uninstall VSCode
vscode-uninstall:
    #!/usr/bin/bash
    echo "Removing VSCodium via Homebrew..."
    brew uninstall ublue-os/tap/visual-studio-code-linux --cask
    echo "VSCode has been uninstalled!"

# Install LMStudio
lmstudio-install:
    #!/usr/bin/bash
    echo "Installing LMStudio via Homebrew..."
    brew tap ublue-os/tap
    brew install ublue-os/tap/lm-studio-linux --cask
    echo "LMStudio has been installed!"

# Uninstall LMStudio
lmstudio-uninstall:
    #!/usr/bin/bash
    echo "Removing LMStudio via Homebrew..."
    brew uninstall ublue-os/tap/lm-studio-linux --cask
    echo "LMStudio has been uninstalled!"

# Install Jetbrain Toolbox
jetbrain-install:
    #!/usr/bin/bash
    echo "Installing Jetbrain Toolbox via Homebrew..."
    brew tap ublue-os/tap
    brew install ublue-os/tap/jetbrains-toolbox-linux --cask
    echo "Jetbrain Toolbox has been installed!"

# Uninstall Jetbrain Toolbox
jetbrain-uninstall:
    #!/usr/bin/bash
    echo "Removing Jetbrain Toolbox via Homebrew..."
    brew uninstall ublue-os/tap/jetbrains-toolbox-linux --cask
    echo "Jetbrain Toolbox has been uninstalled!"
