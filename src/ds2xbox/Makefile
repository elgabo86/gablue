CC = gcc
CFLAGS = -Wall -Wextra -O2 -std=c11
LDFLAGS = -lpthread
TARGET = ds2xbox

all: $(TARGET)

$(TARGET): ds2xbox.c
	$(CC) $(CFLAGS) -o $(TARGET) ds2xbox.c $(LDFLAGS)

install: $(TARGET)
	install -d $(DESTDIR)/usr/bin
	install -m 755 $(TARGET) $(DESTDIR)/usr/bin/$(TARGET)

clean:
	rm -f $(TARGET)

.PHONY: all install clean
