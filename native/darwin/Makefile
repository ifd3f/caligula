CC=clang
CFLAGS+=-Wall -O0 -g

libcaliguladarwin.a: enumdisk.mm.o REDiskList.m.o
	ar r $@ $^

%.m.o: %.m enumdisk.h REDiskList.h
	$(CC) -c $< $(CFLAGS) -F/Library/Frameworks -o $@

%.mm.o: %.mm enumdisk.h REDiskList.h
	$(CC) -c $< $(CFLAGS) -F/Library/Frameworks -o $@

clean:
	rm -f *.o *.a
