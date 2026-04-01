# ClawPi Roadmap

## What this roadmap is for

ClawPi has a big long-term goal.

If we try to build the whole vision at once, the project will get messy very fast.

This roadmap exists to keep the work in the right order.

The idea is simple:

build the foundations first, then build the real OS experience on top of them.

## Phase 0 — lock the direction

Goal:

- define ClawPi as an Agentic OS project
- keep the project OS-first
- keep the project agent-first
- keep the long-term vision clear

Outputs:

- README
- architecture doc
- roadmap
- ADR
- agent instructions

## Phase 1 — shape the repo

Goal:

- create a repo structure that fits an OS project

Outputs:

- docs
- Rust workspace
- image folder
- overlays folder
- systemd folder
- scripts folder

This phase is about giving the project a home that matches what it is trying to become.

## Phase 2 — prove the basic runtime on the current Pi

Goal:

- use the current CM5 device as a proving ground
- test ClawPi logic on real hardware
- prove early boot and runtime ideas before building the full image

Outputs:

- install path for the current device
- early ClawPi tools or helpers
- basic boot/runtime flow on real hardware

Current status:

- the repo includes an early install path for the current DietPi-based CM5
- the repo includes minimal mode selection and setup target wiring for proving-ground tests

This is not the final product.
This is the stage where we prove the early direction on the hardware we already have.

## Phase 3 — create the first setup flow

Goal:

- define what first boot should feel like

Outputs:

- first-boot detection
- setup path
- config-writing path
- handoff into normal mode

Current status:

- the repo now uses `/etc/clawpi/config.toml` as the first setup contract
- `clawpi-setupd` seeds that config when it is missing
- normal mode now depends on the config being valid and marked complete

This is where ClawPi starts to feel like a system instead of just a codebase.

## Phase 4 — connect the runtime layer

Goal:

- wire ClawPi into the agent/runtime layer in a clean way

Outputs:

- runtime wiring
- browser/tool integration path
- memory/task direction
- example system behavior

Current status:

- the repo includes a minimal normal-mode daemon started from `clawpi.target`
- that daemon writes runtime status under `/run/clawpi`
- this is only the first runtime foothold, not the full agent/runtime layer

This phase should focus on integration, not on rebuilding everything from scratch.

## Phase 5 — make setup resilient

Goal:

- make it practical to get a fresh system online

Outputs:

- Wi-Fi setup path
- setup fallback behavior
- clearer recovery into setup mode

Current status:

- the repo now includes a recovery service that redirects recovery mode back into setup mode
- setup fallback behavior is starting to take shape on the proving-ground Pi
- the repo now includes a first OS-owned setup-network path for the current Pi

This is the point where a fresh flash starts feeling much more usable.

## Phase 6 — build the first flashable image

Goal:

- turn the working system direction into a repeatable image build

Outputs:

- image build flow
- ClawPi image layer
- documented build process
- flashable output
- first boot into setup mode

This is the moment where ClawPi starts becoming a real OS product.

Current status:

- the repo now includes an initial custom `pi-gen` stage for ClawPi
- `scripts/build_image.sh` can assemble that stage from the current proven install path
- `scripts/build_image.sh` can now sync that stage into a real `pi-gen` checkout and write the matching `config`
- the repo now includes a small helper for installing `pi-gen` build dependencies on Debian
- the build path now checks for the current CM5/DietPi `16k` page-size mismatch and points the user to the `pi-gen` `arm64` branch
- the custom `stage-clawpi` stage now carries a proper `prerun.sh` rootfs handoff from `stage2`
- the image path now includes the runtime packages and service wiring for a temporary setup network plus phone-driven onboarding
- the setup network now includes captive-portal hints and a direct local-IP fallback for phone onboarding
- normal mode now includes the beginning of a local browser handoff so the setup phone can continue at `http://<device-name>.local/`
- the first real arm64 `pi-gen` build on the CM5 now completes and produces a flashable artifact
- the next proving-ground step is flashing that image to CM5 eMMC and validating the headless onboarding flow
- the full end-to-end flashable image flow still needs to be completed

## Phase 7 — ownership, pairing, and recovery

Goal:

- make the system safer and easier to manage

Outputs:

- pairing direction
- ownership flow
- recovery behavior
- reset path

## Phase 8 — automation and repeatability

Goal:

- make builds and testing easier to repeat

Outputs:

- CI
- image automation
- artifact generation
- better repeatability for development

## Long-term direction

Later phases may include things like:

- better recovery tools
- stronger update flows
- richer browser and tool behavior
- better memory and proactive behavior
- device-to-device pairing helpers
- more complete image and hardware support

The important thing is not to build these too early.

The important thing is to keep building in the right order.
