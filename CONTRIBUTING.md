# Contributing to the Elysiae Launcher

The Elysiae Launcher is primarilly developed by two people new to the tools they are using, so if you identify issues with the source code, feel free to contribute! We will review your changes at our earliest convenience

## Setting up the development environment

Elyisae *must* run on a Linux system in order to successfully compile. If you are using windows, consider using [wsl](https://aka.ms/wsl).

### Minimum System Requirements

To compile Elysiae, you should have the following present on your system:

1. Kernel >= v6.14
2. Systemd (any recent version)
3. A Desktop environment running on wayland
4. A few gigabytes of disk space left for rust compiles
5. rustup (any recent version)
6. meson >= 1.11.0
7. ninja >= 1.13.0
8. gtk4 build libraries >= 4.22

### Installing libraries with mise
