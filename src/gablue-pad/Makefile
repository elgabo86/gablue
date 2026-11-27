# Makefile - binaires C et installation de gablue-pad.
#
# Le Rust (gpad) et le BPF (gpad-gate) ne se compilent PAS ici : ils ont chacun
# leur étape dans le Containerfile (toolchains lourdes, jamais dans l'image
# finale). `make install` installe l'arborescence de données et les binaires C.
CC      = gcc
CFLAGS  = -Wall -Wextra -O2 -std=c11 -D_GNU_SOURCE
PREFIX ?= /usr
BINDIR  = $(DESTDIR)$(PREFIX)/bin
LIBEXEC = $(DESTDIR)$(PREFIX)/libexec/gablue-pad

# arborescence copiée telle quelle : les scripts résolvent `sounds/`, `scripts/`
# et `kwin/` par rapport à leur propre dossier, comme au banc.
DATA = recorder.sh screenshot.sh replay.sh vrr.sh mangohud.sh booster.sh \
       session-start.sh scripts sounds kwin

BINS = gpad-trigger gpad-mouse gpad-inhibit gpad-rumble gpad-login \
       gpad-shot

all: $(BINS)

gpad-trigger: trigger/gpad-trigger.c
	$(CC) $(CFLAGS) -o $@ $<

gpad-mouse: mouse/gpad-mouse.c
	$(CC) $(CFLAGS) -o $@ $< -lm

gpad-inhibit: inhibit/gpad-inhibit.c
	$(CC) $(CFLAGS) -o $@ $< $(shell pkg-config --cflags --libs dbus-1 2>/dev/null)

gpad-rumble: rumble/gpad-rumble.c
	$(CC) $(CFLAGS) -o $@ $<

# Capture d'écran : appel direct à org.kde.KWin.ScreenShot2 + écriture PNG
# (libdbus comme gpad-inhibit, zlib pour le deflate ; pas de Qt).
gpad-shot: shot/gpad-shot.c
	$(CC) $(CFLAGS) -o $@ $< $(shell pkg-config --cflags --libs dbus-1 zlib libjpeg) -lpthread

# Écran de connexion : pointeur manette (même courbe que gpad-mouse, les deux
# sticks, X/R1 = clic, Carré = retour arrière, Start = Entrée).
gpad-login: login/gpad-login.c
	$(CC) $(CFLAGS) -o $@ $< -lm

install: all
	install -d $(BINDIR) $(LIBEXEC) $(LIBEXEC)/gate
	install -m 755 $(BINS) $(BINDIR)/
	cp -a $(DATA) $(LIBEXEC)/
	install -m 755 login/login-bridge.sh $(LIBEXEC)/login-bridge.sh
	# clavier virtuel à la demande (launcher : identification, date) : Python
	# du poste, aucun binaire à lier
	install -m 755 clavier/gpad-clavier $(BINDIR)/
	install -m 755 gate/padnodes.sh $(LIBEXEC)/gate/
	chmod 755 $(LIBEXEC)/*.sh $(LIBEXEC)/gate/padnodes.sh
	chmod 755 $(LIBEXEC)/scripts/*
	rm -f $(LIBEXEC)/kwin/AGENTS.md
	chmod 644 $(LIBEXEC)/sounds/* $(LIBEXEC)/kwin/*.js

clean:
	rm -f $(BINS)

.PHONY: all install clean
