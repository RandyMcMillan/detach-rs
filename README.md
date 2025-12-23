# detach

A Rust project providing functionality for detaching processes and interacting with a gnostr relay. Built with Actix Web and Tokio, it offers a robust solution for background task management and Nostr ecosystem integration.

## Features

*   **Process Detachment**: Execute tasks in the background, independent of the main process.
*   **`gnostr-relay` Integration**: Seamless interaction with Nostr relays for decentralized communication.
*   **Web Server Capabilities**: Leverage Actix Web for efficient HTTP handling and API exposure.
*   **Asynchronous Operations**: Utilizes Tokio for high-performance, non-blocking operations.
*   **Command-Line Interface**: Provides multiple executables for various functionalities.

## Installation

To get started with `detach`, ensure you have Rust and Cargo installed. If not, you can install them using `rustup`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Once Rust is set up, you can clone the repository and build the project:

```bash
git clone https://github.com/gnostr-org/detach.git # Replace with actual repository URL if different
cd detach
cargo build --release
```

The compiled executables will be located in the `target/release/` directory.

## Usage

This project provides several executables, each designed for a specific purpose:

*   `detach-rs`: Likely a general-purpose detachment utility.
*   `gnostr-detach`: Potentially for detaching Nostr-related processes.
*   `gnostr-relay-detach`: Suggests detachment capabilities specifically for `gnostr-relay` operations.

You can run them from the `target/release/` directory:

```bash
./target/release/detach-rs --help
./target/release/gnostr-detach --help
./target/release/gnostr-relay-detach --help
```

Refer to the help output for each executable for detailed usage instructions.

## License

This project is licensed under the MIT License. See the `LICENSE` file for more details.
