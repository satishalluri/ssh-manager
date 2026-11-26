# SSH Manager

A native Linux desktop app written in Rust. GTK 4 draws the host library, VTE provides the terminal tabs, and the system OpenSSH client connects to each host. Version 2 reads the same SQLite database and desktop keyring entries as version 1.

## Run

Run the compiled app from this checkout in your Linux desktop session.

```sh
./ssh-manager
```

To show this checkout in your desktop’s application menu and dock with the SSH Manager icon, register its launcher once.

```sh
./target/release/ssh-manager --install-launcher
```

The included amd64 Debian package was built on Ubuntu 26.04. It installs the Rust executable, the **SSH Manager** application-menu entry, and the app icons. The package declares the minimum glibc version needed by the compiled executable.

```sh
sudo apt install ./dist/ssh-manager_2.0.0_amd64.deb
```

Saving passwords requires a Secret Service provider in your desktop session, such as GNOME Keyring. The app and its SSH password helper both run from the Rust executable.

## Connect to a host

1. Click **Add host** and enter a hostname, IPv4 or IPv6 address, or SSH config alias. You can set a username, port, display name, and folder. Empty username and port fields use your OpenSSH configuration.
2. Select **SSH key or prompt** to use your agent, a private key file, or interactive authentication. Select **Saved password** to store a password in your desktop keyring. Unlock the keyring if your desktop prompts you.
3. Click **Save and connect** to open a terminal tab, or **Save host** to connect later. On a first connection, check the host-key fingerprint shown by OpenSSH before accepting it.
4. Select a saved host to edit or remove it. Double-click a host to open another terminal. A disconnected tab keeps its output and offers **Reconnect**.

OpenSSH reads your usual configuration and known hosts. The saved-password mode uses SSH password authentication. For servers that use keyboard-interactive authentication or MFA, choose **SSH key or prompt** and respond inside the terminal.

## Storage

Host details and opaque credential references live in `$XDG_DATA_HOME/ssh-manager/hosts.sqlite3` (by default `~/.local/share/ssh-manager/hosts.sqlite3`). Passwords live in the desktop keyring. The askpass helper retrieves a saved password for the destination SSH process and returns it over OpenSSH’s private pipe. Host-key confirmations and other challenges stay interactive.

When editing a host, leave the password blank to keep it. Enter a new password when changing a password-authenticated host’s address, port, or username. Replacing a password, switching authentication methods, or removing the host retires the old keyring item. If keyring cleanup is interrupted, the app retries after the next change and on startup.

## Keyboard shortcuts

| Location | Shortcut | Action |
| --- | --- | --- |
| Host library | Ctrl+N | Add a host |
| Host library | Ctrl+F | Search hosts |
| Host editor | Ctrl+S | Save the host |
| Terminal | Ctrl+Shift+C / Ctrl+Shift+V | Copy / paste |
| Terminal | Ctrl+Shift+W | Close the tab |

## Development

The Rust source is in `src/`. Build with Rust 1.89 or later, Cargo, a C linker, pkg-config, and the development libraries for GTK 4, VTE for GTK 4, libsecret, and SQLite. On Debian and Ubuntu the native development packages are `libgtk-4-dev`, `libvte-2.91-gtk4-dev`, `libsecret-1-dev`, and `libsqlite3-dev`. The desktop app uses OpenSSH 8.7 or later.

```sh
cargo build --release --locked
cargo test --locked
./packaging/build-deb.sh
```
