# Netaccess
A fast, robust and interactive CLI (command line interface) application written in [Rust](https://www.rust-lang.org) to manage internet access without all the hassle with <https://netaccess.iitm.ac.in>. This application has the additional capability of monitoring and auto authorizing your system's IP address.

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
* [Release page](https://github.com/jhonboy121/netaccess/releases) of this repo will have latest binaries for all supported platforms whenever there are major changes.
* Download the zip file for your system and extract the contained binary. Copy the path of the directory to which it is extracted and add it to the `PATH` environment variable.
* For mac users, `aarch64` corresponds to the M1/M2 chips and `x86_64` is for the Intel variant, and neither of them will work on the other variant so choose accordingly.

## Usage
To print a list of available commands, provide the --help argument. For example:

This will print all of the subcommands and arguments along with documentation.
```
netaccess --help
```
For example, this will print help for the `status` subcommand.
```
netaccess status --help
```

## Notes
* This application is intended for use by students at IIT Madras alone, and will not work for anyone else.
* All commands require your LDAP username and password as there is no way to safely store it in the system.
* Username and password will be prompted to enter, and password input will be hidden (for your own safety) so just enter the password and hit enter.