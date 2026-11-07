CC = gcc
PKG_CONFIG = pkg-config
CFLAGS = -Wall -Wextra -O2 -std=c11 -D_GNU_SOURCE $(shell $(PKG_CONFIG) --cflags hidapi-hidraw)
LDLIBS = $(shell $(PKG_CONFIG) --libs hidapi-hidraw)
TARGET = gablue-dualsense-ctl

all: $(TARGET)

$(TARGET): dualsense-ctl.c
	$(CC) $(CFLAGS) -o $(TARGET) dualsense-ctl.c $(LDLIBS)

install: $(TARGET)
	install -d $(DESTDIR)/usr/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/bin/$(TARGET)

clean:
	rm -f $(TARGET)

.PHONY: all install clean
