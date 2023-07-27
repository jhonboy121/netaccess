# Netaccess
A CLI (command line interface) application to manage internet access without going through a bunch of procedures in netaccess.iitm.ac.in. This application manages all the HTTP requests with only a single command.

## Building from source
* Install rust following this [link](https://www.rust-lang.org/tools/install). Windows users will require a Visual Studio installation (2019 / 2022) and C++ tools coming with it, see [here](https://learn.microsoft.com/en-us/cpp/build/vscpp-step-0-installation?view=msvc-170#step-4---choose-workloads).
* Once rust is setup, compile the binary with the following command:
```
cargo build --release
```
* Install the built binary with the following command:
```
cargo install --path .
```

## Usage
To print a list of available commands, provide the --help argument. For example:

This will print all of the subcommands and arguments along with documentation.
```
netaccess --help
```
This will print help for the `status` subcommand.
```
netaccess status --help
```
