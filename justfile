PYTHON := "./venv/bin/python3"

alias r := run
alias b := build

server:
    cd server && {{PYTHON}} main.py

run *ARGS:
    #! /bin/sh
    . ~/export-esp.sh

    ESP_LOG=info
    ARGS=""
    for i in {{ARGS}}; do
      if [[ "$i" == "trace" ]]; then
        ESP_LOG=trace;
        break;
      fi
      if [[ "$i" == "debug" ]]; then
        ESP_LOG=debug;
        break;
      fi
      if [[ "$i" == "info" ]]; then
        ESP_LOG=info
        break;
      fi
      if [[ "$i" == "warn" ]]; then
        ESP_LOG=warn
        break;
      fi
      if [[ "$i" == "error" ]]; then
        ESP_LOG=error
        break;
      fi
      ARGS="$i $ARGS";
    done
    cd firmware && ESPFLASH_PORT=/dev/cu.usbmodem1101 ESP_LOG=$ESP_LOG cargo +esp r $ARGS;

build *ARGS:
    #! /bin/sh
    . ~/export-esp.sh

    ESP_LOG=info
    ARGS=""
    for i in {{ARGS}}; do
      if [[ "$i" == "trace" ]]; then
        ESP_LOG=trace;
        break;
      fi
      if [[ "$i" == "debug" ]]; then
        ESP_LOG=debug;
        break;
      fi
      if [[ "$i" == "info" ]]; then
        ESP_LOG=info
        break;
      fi
      if [[ "$i" == "warn" ]]; then
        ESP_LOG=warn
        break;
      fi
      if [[ "$i" == "error" ]]; then
        ESP_LOG=error
        break;
      fi
      ARGS="$i $ARGS";
    done
    cd firmware && ESPFLASH_PORT=/dev/cu.usbmodem1101 ESP_LOG=$ESP_LOG cargo +esp b $ARGS;
