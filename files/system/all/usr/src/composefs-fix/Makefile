.PHONY: install clean

TARGET = /usr/lib64/gablue-composefs-fix.so

install: $(TARGET)

$(TARGET): composefs-fix.c
	gcc -O2 -fPIC -shared -ldl -o $(TARGET) composefs-fix.c
	chmod 755 $(TARGET)

clean:
	rm -f $(TARGET)
