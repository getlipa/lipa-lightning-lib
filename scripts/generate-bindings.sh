#!/usr/bin/env bash

uniffi-bindgen generate src/lipalightninglib.udl --no-format --out-dir bindings/kotlin --language kotlin
uniffi-bindgen generate src/lipalightninglib.udl --no-format --out-dir bindings/swift --language swift