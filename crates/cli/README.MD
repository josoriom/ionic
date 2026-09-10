# Install `ionic`

## 0) Choose install directory

`INSTALL_DIR` can be any directory on your machine, for example the folder where you usually keep your command-line programs (e.g. `~/.local/bin` or `/usr/local/bin`).

```bash
INSTALL_DIR="/path/to/your/directory"
mkdir -p "$INSTALL_DIR"
```

---

## 1) Download

Binaries are attached to each GitHub release. The links below always fetch the latest release; to pin a version, replace `latest/download` with `download/v0.1.0`.

### macOS — arm64

```bash
curl -fL "https://github.com/phenological/ionic/releases/latest/download/ionic-aarch64-apple-darwin" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### macOS — x86_64

```bash
curl -fL "https://github.com/phenological/ionic/releases/latest/download/ionic-x86_64-apple-darwin" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### Linux — x86_64

```bash
curl -fL "https://github.com/phenological/ionic/releases/latest/download/ionic-x86_64-unknown-linux-gnu" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### Linux — arm64

```bash
curl -fL "https://github.com/phenological/ionic/releases/latest/download/ionic-aarch64-unknown-linux-gnu" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### Windows — x86_64

```powershell
$INSTALL_DIR = "C:\path\to\your\directory"
New-Item -ItemType Directory -Force -Path $INSTALL_DIR
Invoke-WebRequest "https://github.com/phenological/ionic/releases/latest/download/ionic-x86_64-pc-windows-msvc.exe" -OutFile "$INSTALL_DIR\ionic.exe"
```

---

## 2) Add `INSTALL_DIR` to `PATH` (shell environment)

This appends an `export PATH=...` line to your shell, so your `PATH` environment variable includes `INSTALL_DIR` and `ionic` is runnable from any directory.
`source` reloads the shell file in the current terminal session (otherwise open a new terminal).

### macOS (zsh)

```bash
echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$HOME/.zshrc"
source "$HOME/.zshrc"
```

### Linux (bash)

```bash
echo "export PATH=\"$INSTALL_DIR:\$PATH\"" >> "$HOME/.bashrc"
source "$HOME/.bashrc"
```

---

## 3) Test

```bash
ionic --help
```

## 4) [DOCS](./src/README.MD)
