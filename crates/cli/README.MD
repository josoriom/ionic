# Install `ionic`

## 0) Choose install directory

`INSTALL_DIR` can be any directory on your machine, for example the folder where you usually keep your command-line programs (e.g. `~/.local/bin` or `/usr/local/bin`).

```bash
INSTALL_DIR="/path/to/your/directory"
mkdir -p "$INSTALL_DIR"
```

---

## 1) Download

### macOS — arm64

```bash
curl -fL "https://raw.githubusercontent.com/josoriom/ionic/main/artifacts/0.1.0/aarch64-apple-darwin/ionic" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### macOS — x86_64

```bash
curl -fL "https://raw.githubusercontent.com/josoriom/ionic/main/artifacts/0.1.0/x86_64-apple-darwin/ionic" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### Linux — x86_64

```bash
curl -fL "https://raw.githubusercontent.com/josoriom/ionic/main/artifacts/0.1.0/x86_64-unknown-linux-gnu/ionic" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### Linux — arm64

```bash
curl -fL "https://raw.githubusercontent.com/josoriom/ionic/main/artifacts/0.1.0/aarch64-unknown-linux-gnu/ionic" -o "$INSTALL_DIR/ionic"
chmod +x "$INSTALL_DIR/ionic"
```

### Windows — x86_64

```powershell
$INSTALL_DIR = "C:\path\to\your\directory"
New-Item -ItemType Directory -Force -Path $INSTALL_DIR
Invoke-WebRequest "https://raw.githubusercontent.com/josoriom/ionic/main/artifacts/0.1.0/x86_64-pc-windows-gnu/ionic.exe" -OutFile "$INSTALL_DIR\ionic.exe"
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
