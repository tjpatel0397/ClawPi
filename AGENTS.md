# AGENTS.md

## Mission

Build ClawPi as an Agentic OS for Raspberry Pi devices.

ClawPi is not meant to feel like a normal app project.

The goal is to create a real operating system experience where agent behavior is part of the system itself.

That means this repo should be treated like an OS project first.

## What ClawPi is trying to become

ClawPi is meant to become a flashable OS for Raspberry Pi devices that gives users a system designed around agent behavior from the start.

That means thinking about things like:

- boot flow
- first-boot setup
- system defaults
- recovery behavior
- device control
- browser use
- memory
- action-taking
- proactive help

## How to think when working in this repo

Do not start from the mindset of:

- “let’s build a normal backend”
- “let’s build a normal frontend”
- “let’s build a dashboard first”
- “let’s build a chatbot app”

Start from the mindset of:

- “what does the operating system need?”
- “what should happen when the device boots?”
- “how should setup work without a monitor?”
- “what system behavior should feel built-in?”
- “what should belong to the OS, not just to an app?”

## Current direction

Right now ClawPi is still very early.

The focus is on shaping the foundations:

- repo structure
- system philosophy
- boot and setup flow
- runtime direction
- image-building path
- real-device testing on Raspberry Pi hardware

The current CM5 device running DietPi is a proving ground, not the final product.

The end goal is still a real flashable ClawPi OS image.

## Working rules

### 1. Keep the project OS-first

Always think in terms of:

- boot targets
- image layers
- first-boot behavior
- system services
- device state
- system-level integration

Do not drift into a generic SaaS or app architecture unless explicitly asked.

### 2. Keep the project agent-first

ClawPi exists to explore what an agentic operating system looks like.

So when making decisions, prefer ideas that move the system toward:

- action-taking
- memory
- context awareness
- proactive help
- real system-level behavior

### 3. Use Rust for ClawPi-owned runtime code

Rust should be the default choice for ClawPi-owned runtime logic wherever it makes sense.

Examples:

- setup binaries
- boot helpers
- device control tools
- small daemons
- OS-facing utilities
- wrappers around Linux behavior

Do not force Rust into places where it is not the right tool, such as:

- systemd unit files
- systemd target files
- pi-gen files
- shell scripts used as build or install glue
- static config files

### 4. Do not rebuild large features without a reason

Before building a new subsystem, first check whether the capability already exists in the runtime stack we plan to use.

Examples may include:

- browser automation
- MCP support
- memory
- scheduling
- pairing
- dashboard or management UI

Do not reinvent large pieces too early.

### 5. Keep phases small

Do not jump ahead.

Build one layer at a time.

Do not add future-phase complexity unless asked.

### 6. Keep changes reviewable

Prefer small, focused changes.

Do not introduce giant speculative structures.

Do not create many moving parts at once.

## Code and structure expectations

When adding code:

- keep it readable
- keep modules focused
- avoid giant files
- prefer simple boundaries
- avoid speculative abstractions
- make local testing easy

When adding folders:

- keep the top-level repo clean
- create new top-level folders only when they clearly belong

## Documentation expectations

If the architecture or direction changes, update the docs in the same task.

At minimum, consider whether these need changes:

- `README.md`
- `docs/architecture.md`
- `docs/roadmap.md`
- ADR files

## End-of-task report

At the end of each task, always report:

1. files changed
2. commands to run
3. how to test on Mac
4. how to test on the current Pi
5. risks or follow-up work
