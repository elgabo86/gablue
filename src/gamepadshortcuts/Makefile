CC = gcc
CFLAGS = -Wall -Wextra -O2 -std=c11 $(shell pkg-config --cflags dbus-1 2>/dev/null)
LDFLAGS = $(shell pkg-config --libs dbus-1 2>/dev/null)
MOUSE_LDFLAGS = -lm
TARGET = gamepadshortcuts
MOUSE_TARGET = gamepadshortcuts-mouse

all: $(TARGET) $(MOUSE_TARGET)

$(TARGET): gamepadshortcuts.c
	$(CC) $(CFLAGS) -o $(TARGET) gamepadshortcuts.c $(LDFLAGS)

$(MOUSE_TARGET): mouse.c
	$(CC) -Wall -Wextra -O2 -std=c11 -D_GNU_SOURCE -o $(MOUSE_TARGET) mouse.c $(MOUSE_LDFLAGS)

install: $(TARGET) $(MOUSE_TARGET)
	install -d $(DESTDIR)/usr/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/bin/$(TARGET)
	install -m 755 $(MOUSE_TARGET) $(DESTDIR)/usr/bin/$(MOUSE_TARGET)

clean:
	rm -f $(TARGET) $(MOUSE_TARGET)

.PHONY: all install clean
