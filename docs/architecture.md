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

The most likely direction is reusing or forking ZeroClaw, OpenClaw, or a
similar Rust-first high-performance runtime as the embedded Claw core inside
the OS.

That runtime should be treated as a deeply integrated system component, not as
an optional app bolted onto Linux later.

ClawPi should wrap that runtime with OS-owned behavior like:

- boot targets
- first-boot setup
- ownership and pairing later
- recovery and reset behavior
- device defaults
- hardware integration
- local browser handoff

ClawPi should prefer integration over reimplementation.

If ZeroClaw/OpenClaw already solves a runtime problem well, ClawPi should reuse
or adapt it before rebuilding that capability from scratch.

At the same time, ClawPi does not need to inherit the default ZeroClaw UX
unchanged.

The runtime can drift in presentation and flow so it feels native to a
dedicated ClawPi device instead of a generic agent product.

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

- a ZeroClaw/OpenClaw-style local agent daemon
- browser automation
- MCP support
- memory
- task execution
- scheduling
- shell and OS command execution
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
- keep the setup behavior OS-owned and headless-first

At the moment this looks like:

- `clawpi-mode.service` runs during boot on the current Pi
- `clawpi-init` chooses a target based on simple local state
- on the current DietPi proving ground, Wi-Fi client mode is owned by `ifup@wlan0.service`
- on that proving ground, ClawPi writes both `wpa_supplicant-wlan0.conf` and a compatibility `wpa_supplicant.conf` so the same setup contract can drive DietPi and the image path
- `clawpi-setup.target` starts `clawpi-setupd` and `clawpi-portald`
- `clawpi-setupd` seeds or validates `/etc/clawpi/config.toml`
- `clawpi-portald` opens a temporary setup network like `ClawPi Setup XXXX`, answers captive-portal probes, and serves a local setup page at `http://setup.clawpi/` with `http://192.168.64.1/` as the direct fallback
- on that DietPi proving ground, `clawpi-portald` has to stop and restore `ifup@wlan0.service` when taking over `wlan0` for setup mode
- when the user submits home Wi-Fi details, `clawpi-portald` writes them into `/etc/clawpi/config.toml`, syncs the local hostname, tears down the setup network, applies the `wpa_supplicant` config, and waits for the device to join the real network
- `clawpi-init` only enters normal mode when that config is valid and complete
- `clawpi-portald` marks setup complete only after the device has joined the submitted Wi-Fi network and then starts `clawpi.target`
- the mode targets are cleaned up after activation so setup mode can be entered again cleanly
- `clawpi.target` now starts `clawpi-sessiond`, which keeps a minimal runtime heartbeat under `/run/clawpi`
- `clawpi.target` now also starts `clawpi-agentd`, which listens on a local Unix socket under `/run/clawpi` and owns prompt execution for the proving ground
- the same `/etc/clawpi/config.toml` contract now also carries the first AI runtime fields: provider, model, and provider-specific auth
- `clawpi.target` also starts `clawpi-webd`, which serves the local browser control surface at `http://<device-name>.local/`
- if the AI fields are missing, `clawpi-webd` becomes a first-run setup shell that asks for the local Claw provider, model, and provider-specific auth
- once those AI fields are present, `clawpi-webd` turns into a narrow local Claw console with a single prompt surface and tucked-away settings
- today that local gateway is still intentionally narrow: it is more of an OS-owned proving-ground surface than a full agent runtime
- the current prompt path now crosses a local runtime boundary: `clawpi-webd` sends prompt requests to `clawpi-agentd`, and `clawpi-agentd` hands those requests into an upstream ZeroClaw-based runtime
- that proving-ground daemon now seeds a small ClawPi-owned workspace for ZeroClaw so the embedded runtime speaks and behaves like ClawPi instead of inheriting the stock upstream identity
- local shell and tool execution for prompt handling now comes from the reused ZeroClaw runtime instead of a ClawPi-owned OpenAI Responses loop
- the runtime target is still to reuse or fork ZeroClaw/OpenClaw or a similar Rust-based agent core and run it as a system-level Claw service inside normal mode
- that embedded runtime should own sessions, memory, tool use, shell access, and long-running task behavior, while `clawpi-webd` stays a front end
- auth is only one slice of that work; ClawPi may still support raw API keys, local runtimes, and account-based sign-in flows later
- `clawpi-recovery.target` now starts `clawpi-recoveryd`, which clears recovery state and redirects back into setup

This is a proving-ground path, not the final image design.

## Initial image path

ClawPi now has the beginning of a real `pi-gen` path.

The current shape is intentionally small:

- `scripts/build_image.sh` assembles a custom ClawPi stage under `target/pi-gen/stage-clawpi`
- that stage is built from the same install flow already proven on the current Pi
- the stage copies ClawPi binaries and units into the image rootfs
- the stage installs the runtime packages needed for headless setup onboarding
- the stage seeds `/etc/clawpi/config.toml` in pending setup mode
- the stage masks the distro `hostapd` and `dnsmasq` services so the setup network is owned by ClawPi
- the stage enables `clawpi-mode.service` for first boot
- the stage now uses a `prerun.sh` handoff so `pi-gen` copies the previous `stage2` rootfs into `stage-clawpi` before applying ClawPi files
- when given a `pi-gen` checkout, the script syncs `stage-clawpi` into that tree and writes a matching `config`
- `scripts/install_pi_gen_deps.sh` can prepare a Debian build host using either the checkout's `depends` file or the current upstream dependency set
- on a CM5-class arm64 build host with a `16k` page-size kernel, the image build should use the `pi-gen` `arm64` branch rather than the default `master` checkout
- the proving-ground CM5 can now complete the first arm64 image build with this stage
- the next proving-ground step is flashing that image to the CM5 eMMC and validating the cordless onboarding flow on real hardware

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
