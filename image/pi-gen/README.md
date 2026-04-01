# ClawPi Image Build

This folder is for the part of ClawPi that turns the project into a real operating system image.

The long-term goal is simple:

**someone should be able to flash ClawPi onto a Raspberry Pi device and boot into an Agentic OS experience.**

## Why this folder exists

ClawPi is not just a runtime idea.

It is meant to become a real flashable operating system for Raspberry Pi devices.

That means we need a place in the repo for the image-building side of the project:

- base OS customization
- package selection
- overlays
- first-boot behavior
- boot targets
- ClawPi defaults
- runtime wiring
- flashable image generation

This folder is where that side of the project will live.

## Why pi-gen

For the first version of the image pipeline, ClawPi is using `pi-gen` as the image build path.

The reason is simple:

- it is a known way to build Raspberry Pi OS images
- it gives us a practical starting point
- it helps us move toward a real flashable ClawPi image without inventing the whole image pipeline from scratch

This does not mean pi-gen is the forever choice.

It just means it is the right early path while ClawPi is still taking shape.

## What this part of the project is trying to do

This image layer should eventually make it possible to:

- build a ClawPi image from source
- boot into ClawPi setup mode on first boot
- transition into normal ClawPi mode after setup
- include ClawPi defaults and configuration
- include the runtime pieces the system needs
- shape the device so it feels like ClawPi from the first boot

## Current stage

This part of the project is still very early.

Right now the focus is not on a polished final build pipeline.

Right now the focus is on:

- creating the right folder structure
- defining how ClawPi should fit into an image build
- keeping a clean path toward a flashable OS image
- making sure the build story matches the bigger direction of the project

## What will likely live here

Over time, this area of the repo may include things like:

- ClawPi-specific pi-gen stages
- package and dependency setup
- image overlays
- first-boot marker behavior
- setup-mode defaults
- normal-mode defaults
- config placement
- service and target wiring
- image build scripts
- build notes for the Debian build server

## Philosophy

This folder should follow the same philosophy as the rest of ClawPi:

- keep the project OS-first
- keep the project agent-first
- keep the long-term flashable image goal clear
- avoid unnecessary complexity too early
- build in a way that is understandable and repeatable

The goal is not to create a messy pile of image hacks.

The goal is to create a clean path to a real ClawPi operating system image.

## Not the goal

This folder is not meant to become:

- a random collection of one-off scripts
- a place for unrelated experiments
- a second project separate from ClawPi
- a polished production image system on day one

It should stay focused on one thing:

**helping ClawPi become a real flashable Agentic OS.**

## Status

Work in progress.

The current practical path is `scripts/install_dev_on_pi.sh`, which installs
the early ClawPi binaries and systemd units onto the DietPi-based proving-ground
CM5 while the flashable image path is still taking shape.

The repo now also includes an initial custom `pi-gen` stage template under
`image/pi-gen/stage-clawpi`, and `scripts/build_image.sh` can assemble the
current ClawPi payload into a stage bundle under `target/pi-gen/stage-clawpi`.

If you pass `--pi-gen-dir`, the script will also sync `stage-clawpi` into that
checkout and write a matching `config` file before running `build.sh`.

That custom stage now includes a `prerun.sh` handoff, so `pi-gen` copies the
previous `stage2` rootfs into `stage-clawpi` before the ClawPi payload is
applied.

The stage also installs `hostapd` and `dnsmasq`, masks their distro-managed
systemd units, and enables the ClawPi-owned onboarding service that opens a
temporary setup network and local setup page on first boot, including
captive-portal hints and a direct `http://192.168.64.1/` fallback when local
DNS does not resolve `setup.clawpi`.

The same image payload now also installs `avahi-daemon` and a small
`clawpi-webd` landing service so a successful phone setup can hand off to
`http://<device-name>.local/` instead of requiring SSH.

If the build host is missing `pi-gen` prerequisites, use
`scripts/install_pi_gen_deps.sh --pi-gen-dir /path/to/pi-gen` on a Debian-based
machine before running the build.

On CM5-class arm64 hosts running a `16k` page-size kernel, a default `pi-gen`
`master` checkout is the wrong target because it builds the `armhf` path. Use
the `arm64` branch for those hosts before running `scripts/build_image.sh`.

The proving-ground CM5 can now complete the arm64 image build with this stage.
The next step is flashing that image onto the CM5 eMMC and validating the
cordless first-boot onboarding path.
