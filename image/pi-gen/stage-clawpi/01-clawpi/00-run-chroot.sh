#!/bin/sh
set -eu

install -d /etc/systemd/system/multi-user.target.wants
ln -snf ../clawpi-mode.service /etc/systemd/system/multi-user.target.wants/clawpi-mode.service
