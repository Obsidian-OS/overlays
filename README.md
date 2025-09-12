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

The library's overlay configuration is loaded from `/etc/obsidianos-overlays.conf`. This file should list overlay paths, with one path per line. Lines beginning with `#` are treated as comments, and empty lines are ignored.

Example `/etc/obsidianos-overlays.conf`:

```
/path/to/overlay1
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

## License

This project is licensed under the [MIT License](LICENSE).