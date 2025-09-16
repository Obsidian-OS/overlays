# ObsidianOS Overlays

A Rust library designed to provide a file system overlay mechanism by intercepting various system calls. This allows for the redirection of file access and execution to alternative paths defined in a configuration file.

**Note:** This library is specifically designed for ObsidianOS. While it may function on other distributions, its use outside of ObsidianOS is not officially supported or recommended.

## Features

- **Comprehensive Filesystem Interception:** Intercepts a wide range of filesystem-related system calls for redirection and overlaying. This includes:
    - **File Opening:** `open`, `open64`, `openat`, `openat64`, `fopen`, `fopen64`, `creat`, `creat64`
    - **File Status:** `stat`, `lstat`, `stat64`, `lstat64`, `statx`
    - **Access Control:** `access`, `faccessat`
    - **Symbolic Links:** `readlink`, `readlinkat`, `symlink`, `symlinkat`, `link`, `linkat`
    - **Execution:** `execve`, `execvp`, `execv`
    - **Directory Operations:** `unlink`, `unlinkat`, `rmdir`, `mkdir`, `mkdirat`, `rename`, `renameat`, `chdir`, `opendir`, `readdir`, `readdir64`, `closedir`
    - **Permissions/Ownership:** `chmod`, `fchmodat`, `chown`, `fchownat`, `lchown`
    - **File Truncation:** `truncate`

- **Directory Merging for `ls` and similar tools:** When `opendir` and `readdir` are intercepted, the library merges the contents of the original directory with its corresponding overlay directory. This means tools like `ls` will display files from both the original location and the overlay. Overlayed files with the same name will take precedence, effectively shadowing the original files.

- **Configurable Overlays:** Overlay paths are configured via `/etc/obsidianos-overlays.conf`.

- **Blacklisting:** Prevents specified paths from being overlaid. This is useful for protecting critical system directories or avoiding unintended behavior. Blacklisted paths will always resolve to their original location, bypassing any overlays.
    - **Default Blacklist:** Includes essential system directories like `/dev`, `/sys`, `/proc`, `/tmp`, and `/run` to prevent system instability.
    - **Configurable Blacklist:** Additional blacklist patterns can be defined in `/etc/obsidianos-overlays.blacklist`. This file supports glob-like patterns (e.g., `/usr/local/bin/*` or `*.log`) which are converted to regular expressions. Lines starting with `#` are treated as comments.



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
