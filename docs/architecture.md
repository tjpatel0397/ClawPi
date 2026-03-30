# ClawPi Architecture

## Overview

ClawPi is an Agentic OS for Raspberry Pi devices.

The goal is not just to run an agent on top of Linux.

The goal is to shape the operating system itself around agent behavior.

That means ClawPi should eventually feel less like “Linux plus some extra software” and more like a system that was designed from the start to understand context, take action, remember, and help proactively.

## The basic idea

Most operating systems were designed around apps.

ClawPi is trying to explore what happens if the starting point is the agent instead.

That changes how we think about the system.

Instead of starting with desktop assumptions, ClawPi starts with questions like:

- how should the device boot?
- how should setup work without a monitor?
- what should be built into the system from day one?
- how should the system take action on behalf of the user?
- what should memory look like at the OS level?
- how should browser use, tools, and automation feel like part of the system?

## What belongs to ClawPi

ClawPi is responsible for the operating system experience.

That includes things like:

- image construction
- boot flow
- first-boot setup
- device defaults
- system-level configuration
- recovery behavior
- network setup behavior
- runtime wiring
- hardware-specific integration
- packaging the system into something flashable

## What the runtime stack is for

ClawPi will rely on an agent runtime stack, but the runtime stack is not the whole project.

The runtime stack exists to help ClawPi become an agentic operating system.

That means ClawPi should use runtime capabilities where they already exist, but the repo itself should stay focused on the OS-level experience.

## System shape

ClawPi can be thought of as having three layers.

### 1. Base OS layer

This is the Raspberry Pi OS / Linux foundation that ClawPi builds on top of.

This layer gives us:

- kernel
- bootloader and boot config
- package base
- Linux userspace
- service management
- device drivers
- networking stack

### 2. ClawPi layer

This is the layer that gives ClawPi its identity.

This layer should define things like:

- setup mode
- normal mode
- recovery mode
- first-boot behavior
- system defaults
- config layout
- boot targets
- setup tools
- device control tools
- image overlays
- recovery/reset behavior later

This is where most ClawPi-owned logic belongs.

### 3. Agent/runtime layer

This layer provides the agent-facing behavior that helps ClawPi feel like an agentic system.

Examples of things that may live here or connect here include:

- browser automation
- MCP support
- memory
- task execution
- scheduling
- pairing
- dashboards or management surfaces

ClawPi should use this layer where it helps, but not let it define the whole philosophy of the project.

## Boot modes

ClawPi should eventually have clear system modes.

### Setup mode

This is the mode for first boot or incomplete setup.

It should be responsible for things like:

- detecting whether the system has been configured
- starting the setup path
- allowing network setup
- preparing the system for normal use

### Normal mode

This is the main operating mode.

It should be responsible for things like:

- bringing the system into its normal runtime state
- loading the expected agent/runtime behavior
- making the device ready for everyday use

### Recovery mode

This comes later.

It should be responsible for things like:

- recovering from bad config
- restoring access to setup
- helping the user repair or reset the device

## Current proving-ground shape

Before ClawPi has its own image, the current CM5 running DietPi is the place to
prove the early system behavior.

The current proving-ground path is intentionally small:

- install ClawPi binaries onto the current Pi
- install systemd units and targets onto the current Pi
- use a small mode-selection step at boot to choose setup, normal, or recovery
- keep setup behavior minimal until the first real setup flow is designed

At the moment this looks like:

- `clawpi-mode.service` runs during boot on the current Pi
- `clawpi-init` chooses a target based on simple local state
- `clawpi-setup.target` starts `clawpi-setupd`
- `clawpi-setupd` seeds or validates `/etc/clawpi/config.toml`
- `clawpi-wifid` can turn stored setup-mode Wi-Fi settings into a `wpa_supplicant` config on the current Pi and prefers reconfiguring an existing supplicant over forcing a restart
- `clawpi-init` only enters normal mode when that config is valid and complete
- the mode targets are cleaned up after activation so setup mode can be entered again cleanly
- `clawpi.target` now starts `clawpi-sessiond`, which keeps a minimal runtime heartbeat under `/run/clawpi`
- `clawpi-recovery.target` now starts `clawpi-recoveryd`, which clears recovery state and redirects back into setup

This is a proving-ground path, not the final image design.

## Initial image path

ClawPi now has the beginning of a real `pi-gen` path.

The current shape is intentionally small:

- `scripts/build_image.sh` assembles a custom ClawPi stage under `target/pi-gen/stage-clawpi`
- that stage is built from the same install flow already proven on the current Pi
- the stage copies ClawPi binaries and units into the image rootfs
- the stage seeds `/etc/clawpi/config.toml` in pending setup mode
- the stage enables `clawpi-mode.service` for first boot
- when given a `pi-gen` checkout, the script syncs `stage-clawpi` into that tree and writes a matching `config`
- `scripts/install_pi_gen_deps.sh` can prepare a Debian build host using either the checkout's `depends` file or the current upstream dependency set
- on a CM5-class arm64 build host with a `16k` page-size kernel, the image build should use the `pi-gen` `arm64` branch rather than the default `master` checkout

This is not the full image pipeline yet.

It is the first point where the repo can produce a real ClawPi-owned image layer instead of only modifying a live device.

## Device philosophy

ClawPi should be designed with the assumption that the device may not have:

- a monitor
- a keyboard
- a mouse

That means setup, recovery, and control paths should not depend on normal desktop assumptions.

## Development philosophy

ClawPi is still early.

So the current job is not to build every feature at once.

The current job is to shape the foundations clearly enough that the long-term direction stays intact.

That means focusing first on:

- repo structure
- image structure
- boot and setup flow
- runtime wiring
- real-device testing
- a clean path toward a flashable OS image

## Language direction

ClawPi-owned runtime code should default to Rust wherever it makes sense.

But the project should not force Rust into every file type.

Use the right tool for the right layer.

Examples of places where non-Rust files still make sense:

- systemd units and targets
- shell scripts for build/install glue
- pi-gen files
- config files

The real goal is not “Rust everywhere.”

The real goal is “a clean, efficient, durable system.”
