# Makefile for janet-world
# Run from this directory (janet-world/).

BINARY    := target/release/janet-world-server
IMAGE     ?= janet-world:latest
CHART_DIR := helm
NAMESPACE ?= janet

.PHONY: all submodules build test check fmt \
        docker-build docker-push \
        helm-lint helm-template helm-install helm-uninstall \
        clean

## ── Submodules ────────────────────────────────────────────────────────────────

submodules:
	git submodule update --init --recursive

## ── Rust ──────────────────────────────────────────────────────────────────────

all: build

build:
	cargo build --release --bin janet-world-server

test:
	cargo test --workspace

check:
	cargo check --workspace

fmt:
	cargo fmt --all

## ── Docker ────────────────────────────────────────────────────────────────────

docker-build: submodules
	docker build -f docker/Dockerfile -t $(IMAGE) .

docker-push:
	docker push $(IMAGE)

## ── Helm ──────────────────────────────────────────────────────────────────────

helm-lint:
	helm lint $(CHART_DIR)

helm-template:
	helm template janet-world $(CHART_DIR) --namespace $(NAMESPACE)

helm-install:
	helm upgrade --install janet-world $(CHART_DIR) \
	    --namespace $(NAMESPACE) --create-namespace

helm-uninstall:
	helm uninstall janet-world --namespace $(NAMESPACE)

## ── Clean ─────────────────────────────────────────────────────────────────────

clean:
	cargo clean
