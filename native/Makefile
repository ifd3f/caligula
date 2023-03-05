CC=clang
CFLAGS+=-O0 -g
CXXFLAGS+=-O0 -g

enumdisk.o: darwin/enumdisk.mm enumdisk.h
	$(CC) -c $< $(LDFLAGS) -F/Library/Frameworks -o $@
