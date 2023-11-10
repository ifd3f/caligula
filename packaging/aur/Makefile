rootdir=$(shell pwd)

.PHONY: main
main: inputs dockerimage build caligula-bin
	docker run --rm -it -v $(rootdir)/inputs:/inputs:ro -v $(rootdir)/caligula-bin:/caligula-bin caligula-packaging

.PHONY: dockerimage
dockerimage: dockerenv
	docker build -t caligula-packaging $<

build:
	mkdir -p build
