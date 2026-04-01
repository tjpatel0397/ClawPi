# ClawPi

**ClawPi is an Agentic OS for Raspberry Pi devices.**

The goal of ClawPi is to turn a Raspberry Pi into a real AI-native computer system

## Vision

Most operating systems were built for screens, windows, apps, files, and human-clicked workflows.

ClawPi is being built from a different starting point:

- the computer should be able to understand context
- the computer should be able to take action
- the computer should be able to remember
- the computer should be able to help proactively
- the computer should be able to work without needing a full monitor, keyboard, and mouse setup

ClawPi is meant to explore what an **agentic operating system** looks like on small, affordable, hackable hardware.

## What ClawPi is trying to become

ClawPi is meant to become a flashable OS for Raspberry Pi devices that gives users a system designed around agent behavior from the start.

That means a ClawPi device should eventually feel like:

- a personal AI system
- a computer that can act on your behalf
- a system that can browse, automate, remember, and assist
- a device that can be set up and used without traditional desktop assumptions
- a platform for building new kinds of AI-native hardware products

## Core idea

The big idea behind ClawPi is simple:

**What would an operating system look like if it was designed for agents first, instead of apps first?**

ClawPi is an attempt to answer that question.

## Why Raspberry Pi

Raspberry Pi devices are small, flexible, affordable, and widely available.

They are a good place to experiment with a new kind of operating system because they let us build something real, physical, and hackable without needing custom silicon or huge budgets.

## Project goals

ClawPi is being built to explore things like:

- first-boot setup without needing a monitor
- agent-native system behavior
- built-in automation and action-taking
- memory and long-running context
- browser and tool use
- proactive assistance
- hardware that feels more like an AI device than a traditional mini computer

## Current stage

ClawPi is in a very early stage.

Right now the focus is on shaping the foundations:

- repo structure
- system philosophy
- boot and setup flow
- runtime direction
- image-building path
- real-device testing on Raspberry Pi hardware
- a proving-ground install path for the current CM5 device

## Long-term direction

The long-term goal is for someone to be able to flash ClawPi onto a Raspberry Pi device and get a true agentic system experience out of the box.

## Status

ClawPi is still early.

The repo now includes:

- a Rust workspace for ClawPi-owned runtime code
- early systemd targets and services for boot-mode selection
- a proving-ground install path for the current DietPi-based CM5
- a minimal setup contract built around `/etc/clawpi/config.toml`
- a first headless onboarding path that opens a temporary ClawPi setup network and local setup page in setup mode, with captive-portal hints and a direct `http://192.168.64.1/` fallback
- a first normal-mode local Claw gateway at `http://<device-name>.local/` so the phone handoff can continue without requiring SSH
- local AI setup fields in the ClawPi config contract so the device can be given its provider, model, and API key after Wi-Fi onboarding
- a small browser-based management UI that can both store AI credentials and send a simple prompt through the configured model
- a minimal normal-mode daemon that writes runtime state under `/run/clawpi`
- a recovery handoff that redirects the device back into setup mode
- an initial custom `pi-gen` stage path that `scripts/build_image.sh` can sync into a real `pi-gen` checkout
- a small `scripts/install_pi_gen_deps.sh` helper for preparing a Debian build host
- a custom `stage-clawpi` stage that now inherits the `stage2` rootfs, installs the onboarding runtime packages, and applies the ClawPi payload
- a first arm64 image build path that now completes on the CM5 proving ground

ClawPi can now produce a first flashable image, but the full OS experience is
still early.
