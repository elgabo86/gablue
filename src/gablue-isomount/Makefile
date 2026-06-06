CC = gcc
CFLAGS = -Wall -Wextra -O2 -std=c11 $(shell pkg-config --cflags dbus-1 2>/dev/null)
LDFLAGS = $(shell pkg-config --libs dbus-1 2>/dev/null)
TARGET = gablue-isomount

all: $(TARGET)

$(TARGET): gablue-isomount.c
	$(CC) $(CFLAGS) -o $(TARGET) gablue-isomount.c $(LDFLAGS)

install: $(TARGET)
	install -d $(DESTDIR)/usr/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/bin/$(TARGET)

clean:
	rm -f $(TARGET)

.PHONY: all install clean
