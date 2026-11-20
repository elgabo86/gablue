CC      ?= cc
PKGS     = libavformat libavcodec libavfilter libavutil libavdevice libswresample libdrm libpulse-simple
CFLAGS  ?= -O2 -Wall -Wextra
LIBS     = -lcap -lpthread $(shell pkg-config --libs libpulse-simple)
DESTDIR ?=

gpad-rec: gpad-rec.c
	$(CC) $(CFLAGS) -o $@ $< $$(pkg-config --cflags --libs $(PKGS)) $(LIBS)

# La file cap n'est PAS posée ici : l'image la pose (voir AGENTS.md), le banc
# la pose une fois par compilation avec `pkexec setcap cap_sys_admin+p`.
install: gpad-rec
	install -Dm755 gpad-rec $(DESTDIR)/usr/bin/gpad-rec
	install -Dm755 grec $(DESTDIR)/usr/bin/grec

clean:
	rm -f gpad-rec

.PHONY: install clean
