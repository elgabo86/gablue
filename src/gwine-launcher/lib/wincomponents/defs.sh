#!/bin/bash

################################################################################
# defs.sh - Définitions des composants Windows
# URLs, checksums et liste des composants requis
################################################################################

WINCOMPONENTS_CACHE="${CACHE_DIR:-$HOME/.cache/gwine}/wincomponents"

ensure_dir -s "$WINCOMPONENTS_CACHE"

declare -A COMPONENT_URLS
declare -A COMPONENT_SHA256

COMPONENT_URLS[vcrun2010_x86]="https://download.microsoft.com/download/5/B/C/5BC5DBB3-652D-4DCE-B14A-475AB85EEF6E/vcredist_x86.exe"
COMPONENT_SHA256[vcrun2010_x86]="31d32fa39d52cac9a765a43660431f7a127eee784b54b2f5e2af3e2b763a1af8"
COMPONENT_URLS[vcrun2010_x64]="https://download.microsoft.com/download/A/8/0/A80747C3-41BD-45DF-B505-E9710D2744E0/vcredist_x64.exe"
COMPONENT_SHA256[vcrun2010_x64]="2fddbc3aaaab784c16bc673c3bae5f80929d5b372810dbc28649283566d33255"

COMPONENT_URLS[vcrun2012_x86]="https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x86.exe"
COMPONENT_SHA256[vcrun2012_x86]="b924ad8062eaf4e70437c8be50fa612162795ff0839479546ce907ffa8d6e386"
COMPONENT_URLS[vcrun2012_x64]="https://download.microsoft.com/download/1/6/B/16B06F60-3B20-4FF2-B699-5E9B7962F9AE/VSU_4/vcredist_x64.exe"
COMPONENT_SHA256[vcrun2012_x64]="681be3e5ba9fd3da02c09d7e565adfa078640ed66a0d58583efad2c1e3cc4064"

COMPONENT_URLS[vcrun2013_x86]="https://download.microsoft.com/download/0/5/6/056dcda9-d667-4e27-8001-8a0c6971d6b1/vcredist_x86.exe"
COMPONENT_SHA256[vcrun2013_x86]="89f4e593ea5541d1c53f983923124f9fd061a1c0c967339109e375c661573c17"
COMPONENT_URLS[vcrun2013_x64]="https://download.microsoft.com/download/0/5/6/056dcda9-d667-4e27-8001-8a0c6971d6b1/vcredist_x64.exe"
COMPONENT_SHA256[vcrun2013_x64]="20e2645b7cd5873b1fa3462b99a665ac8d6e14aae83ded9d875fea35ffdd7d7e"

COMPONENT_URLS[vcrun2022_x86]="https://aka.ms/vs/17/release/vc_redist.x86.exe"
COMPONENT_SHA256[vcrun2022_x86]="0c09f2611660441084ce0df425c51c11e147e6447963c3690f97e0b25c55ed64"
COMPONENT_URLS[vcrun2022_x64]="https://aka.ms/vs/17/release/vc_redist.x64.exe"
COMPONENT_SHA256[vcrun2022_x64]="cc0ff0eb1dc3f5188ae6300faef32bf5beeba4bdd6e8e445a9184072096b713b"

COMPONENT_URLS[dotnetdesktop6_x86]="https://download.visualstudio.microsoft.com/download/pr/cdc314df-4a4c-4709-868d-b974f336f77f/acd5ab7637e456c8a3aa667661324f6d/windowsdesktop-runtime-6.0.36-win-x86.exe"
COMPONENT_SHA256[dotnetdesktop6_x86]="4e77bd970df0a06528ee88d33e4a8c9fb85beedbdd7219b017083acf0c3aa39e"
COMPONENT_URLS[dotnetdesktop6_x64]="https://download.visualstudio.microsoft.com/download/pr/f6b6c5dc-e02d-4738-9559-296e938dabcb/b66d365729359df8e8ea131197715076/windowsdesktop-runtime-6.0.36-win-x64.exe"
COMPONENT_SHA256[dotnetdesktop6_x64]="0d20debb26fc8b2bc84f25fbd9d4596a6364af8517ebf012e8b871127b798941"

COMPONENT_URLS[dotnetdesktop7_x86]="https://download.visualstudio.microsoft.com/download/pr/b840017b-c69f-4724-a152-11020a0039e6/b74aa12e4ee765a3387a7dcd4ba56187/windowsdesktop-runtime-7.0.20-win-x86.exe"
COMPONENT_SHA256[dotnetdesktop7_x86]="58d32d9857bda5da99afc217669aedacdffb20aed61f15315718eeb3a455b273"
COMPONENT_URLS[dotnetdesktop7_x64]="https://download.visualstudio.microsoft.com/download/pr/08bbfe8f-812d-479f-803b-23ea0bffce47/c320e4b037f3e92ab7ea92c3d7ea3ca1/windowsdesktop-runtime-7.0.20-win-x64.exe"
COMPONENT_SHA256[dotnetdesktop7_x64]="57e7c16e7226c9a29dbc3faedd9e5876cec494c7660528052f52160521e7b714"

COMPONENT_URLS[dotnetdesktop8_x86]="https://download.visualstudio.microsoft.com/download/pr/acf6e5d3-1e2f-4072-833c-fa84a10841c5/acd48342207247f404a5aaa58d1a1ea1/windowsdesktop-runtime-8.0.12-win-x86.exe"
COMPONENT_SHA256[dotnetdesktop8_x86]="340e30c8611af3800b74f0560f0b6f3feab82ee5cfa3fc0d115b84b08bd5456d"
COMPONENT_URLS[dotnetdesktop8_x64]="https://download.visualstudio.microsoft.com/download/pr/f1e7ffc8-c278-4339-b460-517420724524/f36bb75b2e86a52338c4d3a90f8dac9b/windowsdesktop-runtime-8.0.12-win-x64.exe"
COMPONENT_SHA256[dotnetdesktop8_x64]="cb51b559f343cb56e23cad2e5af8c4d1701e221a0a2a4116193a2a9375568814"

COMPONENT_URLS[directx_Jun2010]="https://files.holarse-linuxgaming.de/mirrors/microsoft/directx_Jun2010_redist.exe"
COMPONENT_SHA256[directx_Jun2010]="8746ee1a84a083a90e37899d71d50d5c7c015e69688a466aa80447f011780c0d"

COMPONENT_URLS[corefont_andale]="https://github.com/pushcx/corefonts/raw/master/andale32.exe"
COMPONENT_SHA256[corefont_andale]="0524fe42951adc3a7eb870e32f0920313c71f170c859b5f770d82b4ee111e970"
COMPONENT_URLS[corefont_arial]="https://github.com/pushcx/corefonts/raw/master/arial32.exe"
COMPONENT_SHA256[corefont_arial]="85297a4d146e9c87ac6f74822734bdee5f4b2a722d7eaa584b7f2cbf76f478f6"
COMPONENT_URLS[corefont_arialb]="https://github.com/pushcx/corefonts/raw/master/arialb32.exe"
COMPONENT_SHA256[corefont_arialb]="a425f0ffb6a1a5ede5b979ed6177f4f4f4fdef6ae7c302a7b7720ef332fec0a8"
COMPONENT_URLS[corefont_comic]="https://github.com/pushcx/corefonts/raw/master/comic32.exe"
COMPONENT_SHA256[corefont_comic]="9c6df3feefde26d4e41d4a4fe5db2a89f9123a772594d7f59afd062625cd204e"
COMPONENT_URLS[corefont_courier]="https://github.com/pushcx/corefonts/raw/master/courie32.exe"
COMPONENT_SHA256[corefont_courier]="bb511d861655dde879ae552eb86b134d6fae67cb58502e6ff73ec5d9151f3384"
COMPONENT_URLS[corefont_georgia]="https://github.com/pushcx/corefonts/raw/master/georgi32.exe"
COMPONENT_SHA256[corefont_georgia]="2c2c7dcda6606ea5cf08918fb7cd3f3359e9e84338dc690013f20cd42e930301"
COMPONENT_URLS[corefont_impact]="https://github.com/pushcx/corefonts/raw/master/impact32.exe"
COMPONENT_SHA256[corefont_impact]="6061ef3b7401d9642f5dfdb5f2b376aa14663f6275e60a51207ad4facf2fccfb"
COMPONENT_URLS[corefont_times]="https://github.com/pushcx/corefonts/raw/master/times32.exe"
COMPONENT_SHA256[corefont_times]="db56595ec6ef5d3de5c24994f001f03b2a13e37cee27bc25c58f6f43e8f807ab"
COMPONENT_URLS[corefont_trebuchet]="https://github.com/pushcx/corefonts/raw/master/trebuc32.exe"
COMPONENT_SHA256[corefont_trebuchet]="5a690d9bb8510be1b8b4fe49f1f2319651fe51bbe54775ddddd8ef0bd07fdac9"
COMPONENT_URLS[corefont_verdana]="https://github.com/pushcx/corefonts/raw/master/verdan32.exe"
COMPONENT_SHA256[corefont_verdana]="c1cb61255e363166794e47664e2f21af8e3a26cb6346eb8d2ae2fa85dd5aad96"
COMPONENT_URLS[corefont_webdings]="https://github.com/pushcx/corefonts/raw/master/webdin32.exe"
COMPONENT_SHA256[corefont_webdings]="64595b5abc1080fba8610c5c34fab5863408e806aafe84653ca8575bed17d75a"

COMPONENT_URLS[d3dcompiler_47_x86]="https://raw.githubusercontent.com/mozilla/fxc2/master/dll/d3dcompiler_47_32.dll"
COMPONENT_SHA256[d3dcompiler_47_x86]="2ad0d4987fc4624566b190e747c9d95038443956ed816abfd1e2d389b5ec0851"
COMPONENT_URLS[d3dcompiler_47_x64]="https://raw.githubusercontent.com/mozilla/fxc2/master/dll/d3dcompiler_47.dll"
COMPONENT_SHA256[d3dcompiler_47_x64]="4432bbd1a390874f3f0a503d45cc48d346abc3a8c0213c289f4b615bf0ee84f3"

COMPONENT_URLS[msls31]="https://web.archive.org/web/20160710055851if_/http://download.microsoft.com/download/WindowsInstaller/Install/2.0/NT45/EN-US/InstMsiW.exe"
COMPONENT_SHA256[msls31]="4c3516c0b5c2b76b88209b22e3bf1cb82d8e2de7116125e97e128952372eed6b"

COMPONENT_URLS[vb6run]="https://web.archive.org/web/20070204154430/https://download.microsoft.com/download/5/a/d/5ad868a0-8ecd-4bb0-a882-fe53eb7ef348/VB6.0-KB290887-X86.exe"
COMPONENT_SHA256[vb6run]="467b5a10c369865f2021d379fc0933cb382146b702bbca4bcb703fc86f4322bb"

COMPONENT_URLS[vcrun6]="https://download.microsoft.com/download/vc60pro/Update/2/W9XNT4/EN-US/VC6RedistSetup_deu.exe"
COMPONENT_SHA256[vcrun6]="c2eb91d9c4448d50e46a32fecbcc3b418706d002beab9b5f4981de552098cee7"

COMPONENT_URLS[tahoma_cab]="https://downloads.sourceforge.net/corefonts/OldFiles/IELPKTH.CAB"
COMPONENT_SHA256[tahoma_cab]="c1be3fb8f0042570be76ec6daa03a99142c88367c1bc810240b85827c715961a"

COMPONENT_URLS[openal]="https://www.openal.org/downloads/oalinst.zip"
COMPONENT_SHA256[openal]="d165bcb7628fd950d14847585468cc11943b2a1da92a59a839d397c68f9d4b06"

COMPONENT_URLS[physx]="https://us.download.nvidia.com/Windows/9.23.1019/PhysX_9.23.1019_SystemSoftware.exe"
COMPONENT_SHA256[physx]="9b42b84e881769d681e09f62a1b51532616b2e6a2d5d99d0ccae6eb5fbbc208c"

COMPONENT_URLS[wsh57]="https://download.microsoft.com/download/4/4/d/44de8a9e-630d-4c10-9f17-b9b34d3f6417/scripten.exe"
COMPONENT_SHA256[wsh57]="63c781b9e50bfd55f10700eb70b5c571a9bedfd8d35af29f6a22a77550df5e7b"

COMPONENT_URLS[wmp9]="https://web.archive.org/web/20180404022333if_/download.microsoft.com/download/1/b/c/1bc0b1a3-c839-4b36-8f3c-19847ba09299/MPSetup.exe"
COMPONENT_SHA256[wmp9]="678c102847c18a92abf13c3fae404c3473a0770c871a046b45efe623c9938fc0"

COMPONENT_URLS[wm9codecs]="https://am.net/lib/tools/Microsoft/MPlayer/WM9Codecs9x.exe"
COMPONENT_SHA256[wm9codecs]="f25adf6529745a772c4fdd955505e7fcdc598b8a031bb0ce7e5856da5e5fcc95"

WINCOMPONENTS_REQUIRED=(
    "vcrun2010_x86" "vcrun2010_x64"
    "vcrun2012_x86" "vcrun2012_x64"
    "vcrun2013_x86" "vcrun2013_x64"
    "vcrun2022_x86" "vcrun2022_x64"
    "dotnetdesktop6_x86" "dotnetdesktop6_x64"
    "dotnetdesktop7_x86" "dotnetdesktop7_x64"
    "dotnetdesktop8_x86" "dotnetdesktop8_x64"
    "directx_Jun2010"
    "corefont_andale" "corefont_arial" "corefont_arialb"
    "corefont_comic" "corefont_courier" "corefont_georgia"
    "corefont_impact" "corefont_times" "corefont_trebuchet"
    "corefont_verdana" "corefont_webdings"
    "tahoma_cab"
    "d3dcompiler_47_x86" "d3dcompiler_47_x64"
    "msls31" "vb6run" "vcrun6"
    "openal" "physx"
    "wsh57" "wmp9" "wm9codecs"
)
