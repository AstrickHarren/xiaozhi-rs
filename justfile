PYTHON := "./server/venv/bin/python3"

server:
    {{PYTHON}} server/main.py

run:
    #! /bin/sh
    . ~/export-esp.sh
    cd firmware && cargo +esp r
