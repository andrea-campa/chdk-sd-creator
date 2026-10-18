# Contributing

Thanks for helping out! There are three useful ways to contribute.

## Report how it went on your camera

Only a few models have been tried in a real camera. If you've used the app, [open a camera report](https://github.com/andrea-campa/chdk-sd-creator/issues/new?template=camera_report.yml), whether it worked or not. Include:
- the camera model and firmware version;
- the start method you used;
- the card size and format.

## Report a bug or suggest a feature

Use the [issue templates](https://github.com/andrea-campa/chdk-sd-creator/issues/new/choose). For bugs, the install log on the app's last screen (under **Install log**) is the most helpful thing to include.

## Send a pull request

1. Fork the repo and create a branch from `main`.
2. Make your change. Keep pull requests focused: one fix or feature each.
3. Run the checks below and make sure they pass:

   ```bash
   cd src-tauri
   cargo fmt --check
   cargo clippy --all-targets -- -D warnings
   cargo test
   ```

4. If you changed anything in the UI, include a screenshot in the pull request.

### Camera data

`src-tauri/cameras.toml` is generated; don't edit it by hand.
- **To refresh it from CHDK:** run `python3 tools/gen_cameras.py`.
- **To fix a name or add notes for a model:** edit `src-tauri/cameras.overrides.toml`, then re-run the generator. Overrides always win.

### Code style

- **Rust:** `rustfmt` defaults, no clippy warnings, and `anyhow` errors with context the user can act on.
- **Front end:** plain HTML, CSS and JavaScript, with no framework or build step. Colours are CSS variables, so light and dark themes both work.
- **User-facing text:** say what happened and what to do next.

## License

By contributing, you agree that your contributions are licensed under the project's [GPL-3.0-or-later](LICENSE) license.

## Code of conduct

Be kind and assume good intent. Harassment of any kind isn't tolerated.
