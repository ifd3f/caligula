FROM archlinux

RUN pacman -Syu --noconfirm \
        base-devel \
        rust \
        sudo

RUN useradd -m user

COPY entrypoint.sh /entrypoint.sh

ENTRYPOINT [ "/entrypoint.sh" ]