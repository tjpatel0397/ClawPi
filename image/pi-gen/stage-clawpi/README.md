# ClawPi Custom pi-gen Stage

This directory is a ClawPi-owned custom stage for `pi-gen`.

It is intentionally narrow:

- inherit the `stage2` rootfs with the normal `pi-gen` stage handoff
- install the current ClawPi binaries
- install the current ClawPi systemd units
- install the runtime packages needed for setup networking
- install the runtime packages needed for captive-style setup networking
- install the runtime package needed for `.local` browser discovery in normal mode
- install the first local Claw agent/runtime pair so AI credentials and simple prompts can be handled on-device after Wi-Fi onboarding
- seed `/etc/clawpi/config.toml` into pending setup mode
- mask distro-managed `hostapd` and `dnsmasq` units so ClawPi owns setup mode
- enable `clawpi-mode.service` for first boot

The generated payload for this stage is assembled by `scripts/build_image.sh`.

That keeps the image path aligned with the proving-ground install path instead of
creating a second installation story just for image builds.
