CC = gcc
CFLAGS = -Wall -Wextra -O2 -std=c11
TARGET = gamepadshortcuts

all: $(TARGET)

$(TARGET): gamepadshortcuts.c
	$(CC) $(CFLAGS) -o $(TARGET) gamepadshortcuts.c

install: $(TARGET)
	install -d $(DESTDIR)/usr/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/bin/$(TARGET)

clean:
	rm -f $(TARGET)

.PHONY: all install clean
