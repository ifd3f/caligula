CC=clang
CFLAGS+=-O0 -g
CXXFLAGS+=-O0 -g

libcaliguladarwin.a: enumdisk.o
	ar r $@ $^

enumdisk.o: enumdisk.mm enumdisk.h
	$(CC) -c $< $(LDFLAGS) -F/Library/Frameworks -o $@

clean:
	rm -f *.o *.a
