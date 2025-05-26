# Xiaozhi AI Chatbot

This is a rust version of [Xiaozhi AI Chatbot](https://github.com/78/xiaozhi-esp32), currently still in very early development. Built with [embassy](https://github.com/embassy-rs/embassy) with async support.

## Build Instructions

To build the project, you will be needing esp toolchain for rust, you can install it with `espup`, see tutorial from the [offical book](https://docs.esp-rs.org/book/installation/riscv-and-xtensa.html)

Then go to `firmware` directory and build it as you do like any other rust project,

```
cd firmware
cargo build --release
```
