SHELL := /usr/bin/env bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := install

CARGO ?= cargo
DESKTOP_FILE_INSTALL ?= desktop-file-install
CARGO_FLAGS ?=
BUILD_PROFILE ?= release
FEATURES ?= heif
JOBS ?= 4
CARGO_TARGET_DIR ?= target
export CARGO_TARGET_DIR

PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin
DATADIR ?= $(PREFIX)/share
DESTDIR ?=

PROFILE_DIR ?= $(if $(filter dev,$(BUILD_PROFILE)),debug,$(BUILD_PROFILE))
BUILD_DIR = $(CARGO_TARGET_DIR)/$(PROFILE_DIR)
APP_ID := ai.storyteller.photocraft

.PHONY: all build install install-desktop update update-install help

all: install

build:
	"$(CARGO)" build --locked --profile "$(BUILD_PROFILE)" --jobs "$(JOBS)" \
		--target-dir "$(CARGO_TARGET_DIR)" -p photocraft -p photocraft-cli --bins \
		$(if $(strip $(FEATURES)),--features "$(FEATURES)") $(CARGO_FLAGS)

install: build
	install -d "$(DESTDIR)$(BINDIR)"
	install -m 755 "$(BUILD_DIR)/photocraft" "$(DESTDIR)$(BINDIR)/photocraft"
	install -m 755 "$(BUILD_DIR)/photocraft-cli" "$(DESTDIR)$(BINDIR)/photocraft-cli"
	$(MAKE) --no-print-directory install-desktop

install-desktop:
	test -x "$(DESTDIR)$(BINDIR)/photocraft"
	"$(DESKTOP_FILE_INSTALL)" --dir="$(DESTDIR)$(DATADIR)/applications" \
		--mode=644 --set-key=Exec --set-value='"$(BINDIR)/photocraft" %F' \
		--set-key=TryExec --set-value="$(BINDIR)/photocraft" \
		--rebuild-mime-info-cache "packaging/linux/$(APP_ID).desktop"
	for icon in assets/app-icon/hicolor/*/apps/$(APP_ID).*; do \
		icon_path="$${icon#assets/app-icon/}"; \
		install -D -m 644 "$$icon" "$(DESTDIR)$(DATADIR)/icons/$$icon_path"; \
	done

update:
	git pull --ff-only
	$(MAKE) --no-print-directory install

update-install: update

help:
	@printf '%s\n' \
		'make                  Build and install PhotoCraft and photocraft-cli.' \
		'make build            Build both native binaries without installing.' \
		'make install-desktop  Repair launchers and icons without rebuilding.' \
		'make update           Fast-forward the checkout, then build and install.' \
		'make -n update        Preview the update and install commands.' \
		'Configuration: PREFIX, BINDIR, DATADIR, DESTDIR, CARGO, CARGO_FLAGS,' \
		'               CARGO_TARGET_DIR, BUILD_PROFILE, PROFILE_DIR, FEATURES (default: heif), JOBS, DESKTOP_FILE_INSTALL.' \
		'Desktop installation requires desktop-file-utils.'
