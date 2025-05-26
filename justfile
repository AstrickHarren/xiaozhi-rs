PYTHON := "./venv/bin/python3"

server:
    cd server && {{PYTHON}} main.py

run:
    #! /bin/sh
    . ~/export-esp.sh
    cd firmware && cargo +esp r
