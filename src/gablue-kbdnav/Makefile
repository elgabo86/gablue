CC = gcc
CFLAGS = -Wall -Wextra -O2 -std=c11 -D_GNU_SOURCE
TARGET = gablue-kbdnav

all: $(TARGET)

$(TARGET): gablue-kbdnav.c
	$(CC) $(CFLAGS) -o $(TARGET) gablue-kbdnav.c

install: $(TARGET)
	install -d $(DESTDIR)/usr/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/bin/$(TARGET)

clean:
	rm -f $(TARGET)

.PHONY: all install clean
