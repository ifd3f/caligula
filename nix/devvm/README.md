# Developer VM

This command runs a VM in your terminal and mounts your PWD to `/tmp/shared` inside the VM. 

Generally, this should be run in the root of this project.

```sh
SHARED_DIR=$PWD nix run .#devvm-aarch64-linux 
```

You may have to cross-compile using a remote machine. In that case, you can put builders in like so ([see Nix docs for more info](https://nix.dev/manual/nix/2.18/advanced-topics/distributed-builds)):

```sh
SHARED_DIR=$PWD nix run --builders 'ssh://astrid@your-host.example aarch64-linux' .#devvm-aarch64-linux 
```

Note that you may or may not have to `sudo` cargo commands inside the VM.

## USB Hotplug

There is a helper script for adding and removing USBs from the VM.

```sh
nix run .#devvm-usbhotplug -- add foo 10M
```

```sh
nix run .#devvm-usbhotplug -- rm foo
```