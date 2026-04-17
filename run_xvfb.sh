#!/bin/bash
export DISPLAY=:99
Xvfb :99 -screen 0 1920x1080x24 &
sleep 2
npx tauri dev > /tmp/tauri_dev.log 2>&1
