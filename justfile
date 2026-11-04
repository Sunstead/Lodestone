# Lodestone task runner. `brew install just`, or use the npm scripts in
# package.json, which mirror the main recipes.

default:
    @just --list

# Install frontend and e2e dependencies and the Playwright browser.
setup:
    npm --prefix frontend install
    npm --prefix e2e install
    cd e2e && npx playwright install chromium

# Frontend only, in-browser demo data. No backend needed.
dev-demo:
    cd frontend && VITE_LODESTONE_MODE=demo npm run dev

# Backend with simulated servers (no Docker, no downloads) + frontend.
dev-fake:
    node scripts/dev.mjs fake

# Backend running real servers in Docker + frontend. Needs Docker running.
dev:
    node scripts/dev.mjs real

# Backend only, simulated runtime, serving the built frontend on :7878.
serve-fake: build-web
    cd backend && cargo run --bin lodestone -- --runtime fake --providers fake --accept-eula --dev-user dev --static-dir ../frontend/dist

# Production-mode frontend build (talks to the backend, no demo fallback).
build-web:
    cd frontend && npm run build:live

# Fast checks: Rust tests, clippy, type-check, unit tests, demo contract.
test:
    cd backend && cargo test
    cd backend && cargo clippy --all-targets -- -D warnings
    cd frontend && npm run typecheck && npm test

# Contract parity: the same scenarios against the demo AND a live backend.
test-contract:
    node scripts/with-backend.mjs -- npm --prefix frontend test -- src/api/contract.test.ts

# Real-container tests (Docker + network): vanilla/Paper/Fabric servers.
test-docker:
    cd backend && cargo test --features docker-tests -- --test-threads=1 --nocapture

# Live provider tests against Mojang/PaperMC/Fabric/Modrinth APIs.
test-live:
    cd backend && cargo test -- --ignored

# Browser tests: demo mode and the full stack on simulated servers.
e2e: build-web
    cd e2e && npx playwright test

# Regenerate frontend/src/api/generated from the Rust model.
gen-types:
    cd backend && cargo test --quiet --lib export_bindings

# Fail if the generated TypeScript contract is stale.
check-types: gen-types
    git diff --exit-code -- frontend/src/api/generated

# Spin up a real vanilla server end to end and talk to it (Docker + network).
smoke:
    node scripts/smoke.mjs

# Everything CI runs.
ci: test test-contract check-types e2e

# Build the production image (backend + built UI). See docs/deploy.md.
image:
    docker build -t lodestone .
