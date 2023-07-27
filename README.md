# Netaccess
A CLI (command line interface) application to manage internet access without going through a bunch of procedures in netaccess.iitm.ac.in. This application manages all the HTTP requests with only a single command.

## Installation

### Building from source
* Install rust following this [link](https://www.rust-lang.org/tools/install). Windows users will require a Visual Studio installation (2019 / 2022) and C++ tools coming with it, see [here](https://learn.microsoft.com/en-us/cpp/build/vscpp-step-0-installation?view=msvc-170#step-4---choose-workloads).
* Once rust is setup, compile the binary with the following command:
```
cargo build --release
```
* Install the built binary with the following command:
```
cargo install --path .
```

### Prebuilt binaries
* Release page of this repo will have latest binaries for all platforms whenever there are major changes. You may download the one for your system and use it.
* Download the zip file and extract the contained binary. Copy the path of the directory to which it is extracted and add it to the `PATH` environment variable.
* For mac users, aarch64 corresponds to the M1/M2 chips and x86_64 is for the Intel variant, and neither of them will work on the other variant so choose accordingly.

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
