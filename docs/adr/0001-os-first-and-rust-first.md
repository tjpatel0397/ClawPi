# ADR 0001: ClawPi is OS-first, agent-first, and Rust-first

## Status

Accepted

## Date

2026-03-29

## Why this decision exists

ClawPi has a big idea behind it.

It is not meant to be just another app project.

It is meant to become an Agentic OS for Raspberry Pi devices.

That means we need to lock the direction early, before the repo drifts into the wrong shape.

## Decision

ClawPi will be developed with these guiding rules:

- OS-first
- agent-first
- Rust-first for ClawPi-owned runtime code

## What OS-first means

ClawPi should be treated like an operating system project.

That means thinking first about things like:

- image building
- boot flow
- setup flow
- system defaults
- device behavior
- recovery
- system-level integration

Not just app screens or app APIs.

## What agent-first means

ClawPi exists to explore what happens when the operating system is designed around agent behavior from the start.

That means decisions should move the project toward:

- context awareness
- action-taking
- memory
- proactive help
- system-level behavior instead of bolt-on behavior

## What Rust-first means

Rust should be the default language for ClawPi-owned runtime code wherever it makes sense.

Examples:

- small daemons
- device control tools
- setup binaries
- system-facing helpers
- wrappers around Linux behavior

This does not mean Rust must be used for every file in the repo.

Some parts are naturally better as:

- systemd files
- shell glue
- config files
- image build files

The goal is not purity.

The goal is a clean and durable system.

## Consequences

### Positive

- keeps the project aligned with its real purpose
- reduces the chance that the repo turns into a generic app stack
- makes room for system-level thinking from the start
- keeps ClawPi-owned runtime code efficient and clear
- helps keep the long-term OS vision intact

### Negative

- requires more discipline early
- makes the project less familiar than a normal app repo
- may slow down some fast experiments
- still requires non-Rust glue in some places

## Notes

The current DietPi install on the CM5 is a proving ground for development.

It is not the final product.

The final product is still meant to be a flashable ClawPi operating system image.
