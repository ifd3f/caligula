FROM archlinux

RUN pacman -Syu --noconfirm \
        python3 \
        base-devel \
        rust \
        sudo

RUN useradd -m -G wheel user
RUN echo '%wheel ALL=(ALL) NOPASSWD: ALL' >> /etc/sudoers

COPY tools /tools

ENTRYPOINT [ "/tools/entrypoint.py" ]
