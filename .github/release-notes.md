## Downloads

- **macOS (Apple Silicon and Intel):** `Pecunia-<version>-macos-universal.dmg`. Open it and drag
  Pecunia to Applications. The app is not notarised yet, so the first time, right-click it and
  choose **Open** (or run `xattr -dr com.apple.quarantine /Applications/Pecunia.app`).
- **macOS CLI only:** `pecunia-cli-<version>-macos-universal.tar.gz`.
- **Linux:** `pecunia-<version>-linux-x86_64.tar.gz` or `-linux-arm64.tar.gz`. Unpack it and run
  `./pecunia`; `pecunia.desktop` and `pecunia.png` are included for a menu entry.
- **Windows:** `pecunia-<version>-windows-x86_64.zip`. Unzip it and run `pecunia.exe`. It isn't
  code-signed yet, so SmartScreen may ask you to confirm (More info › Run anyway).
- `SHA256SUMS.txt` holds the checksums.

The CLI (`pecunia-cli`) is also inside the Mac app at `Pecunia.app/Contents/MacOS/pecunia-cli`.
