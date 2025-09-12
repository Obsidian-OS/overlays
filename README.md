# ObsidianOS Overlays

A Rust library designed to provide a file system overlay mechanism by intercepting various system calls. This allows for the redirection of file access and execution to alternative paths defined in a configuration file.

**Note:** This library is specifically designed for ObsidianOS. While it may function on other distributions, its use outside of ObsidianOS is not officially supported or recommended.

## Features

- **File Operations Interception:** Intercepts `open`, `open64`, `openat`, `openat64`, `fopen`, and `fopen64` for file opening operations.
- **File Status Interception:** Intercepts `stat`, `lstat`, `stat64`, and `lstat64` for retrieving file status information.
- **Access Control Interception:** Intercepts `access` and `faccessat` for file access checks.
- **Symbolic Link Interception:** Intercepts `readlink` and `readlinkat` for reading symbolic links.
- **Execution Interception:** Intercepts `execve`, `execvp`, and `execv` for program execution.
- **Configurable Overlays:** Overlay paths are configured via `/etc/obsidianos-overlays.conf`.

## Configuration

The library's overlay configuration is loaded from `/etc/obsidianos-overlays.conf`. This file should list overlay paths, with one path per line. Anything after `#` is treated as a comment, and empty lines are ignored and if the file doesn't exist it will assume "no overlays".

Example `/etc/obsidianos-overlays.conf`:

```
/path/to/overlay1 # my overlay
/path/to/another/overlay
# This is a comment
```

When a program attempts to access a file, `obsidianos-overlays` checks for an overlaid version of the file within the configured overlay paths. If an overlaid file is found, it will be used in place of the original.

## Usage

This is a low-level library intended for preloading using mechanisms such as `LD_PRELOAD` to intercept system calls.

To utilize this library, compile it and then set the `LD_PRELOAD` environment variable to the path of the compiled library before executing your application:

```bash
# Assuming the library is compiled to target/release/libobsidianos_overlays.so
LD_PRELOAD=/path/to/target/release/libobsidianos_overlays.so your_application
```

### Verbose Mode

To enable verbose output, set the `OBSIDIANOS_OVERLAYS_VERBOSE` environment variable to `1`. When enabled, the library will print messages to `stderr` whenever an overlay is successfully applied, showing the original path and the overlaid path.

Example:

```bash
OBSIDIANOS_OVERLAYS_VERBOSE=1 LD_PRELOAD=/path/to/target/release/libobsidianos_overlays.so your_application
```

Example verbose output:

```
[*] ObsidianOS Overlays: /usr/bin/foo -> /path/to/overlay1/usr/bin/foo
```

## License

This project is licensed under the [MIT License](LICENSE).
