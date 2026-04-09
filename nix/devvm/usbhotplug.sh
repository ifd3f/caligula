#!/usr/bin/env bash

validate_variable() {
    if [ -z "${2:-}" ]; then
        echo "Error: $1 not provided"
        print_help
        exit 1
    fi
}

print_help() {
    echo "Caligula DevVM USB hotplugger"
    echo "Usage:"
    printf "\t%s add <name> <size>\n" "$0"
    printf "\t%s rm <name>\n" "$0"
}

to_qemu() {
    nc -U /tmp/caligula-devvm-monitor.sock
}

add_usb() {
    validate_variable "name" "${1:-}"
    validate_variable "size" "${2:-}"

    name="$1"
    size="$2"
    file="/tmp/caliguladev_usb_drive_$name"

    set -euxo pipefail

    truncate -s "$size" "$file"
    echo "drive_add 0 if=none,id=drive-$name,format=raw,file=$file" | to_qemu
    echo "device_add usb-storage,bus=xhci.0,drive=drive-$name,removable=on,id=device-$name" | to_qemu
}

rm_usb() {
    validate_variable "name" "${1:-}"

    name="$1"
    file="/tmp/caliguladev_usb_drive_$name"

    set -euxo pipefail

    echo "device_del device-$name" | to_qemu
    echo "drive_del drive-$name" | to_qemu
    rm "$file"
}

case "${1:-}" in
    add)
        shift 1
        add_usb "$@"
        ;;
    rm)
        shift 1
        rm_usb "$@"
        ;;
    *)
        print_help
        exit 1
        ;;
esac
