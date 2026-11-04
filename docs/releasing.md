# Releases

## Prepare a source change

Run the [development checks](development.md), review [CHANGELOG.md](../CHANGELOG.md), and regenerate the CLI reference. Run the repository checker from the project root:

```powershell
./scripts/check-repository.ps1
```

Review the files selected for publication. `target/` and `dist/` contain local build products and external test inputs; they are excluded by `.gitignore`. Validation snapshots contain input hashes and result summaries, without captured workstation output. Keep repository metadata such as the Cargo `repository` URL aligned with the actual GitHub destination once it is chosen.

`./scripts/check-repository.ps1 -ListFiles` lists the checked source files for review, including GitHub workflows and Cargo configuration.

## Build Windows artifacts

From Windows x64 with the MSVC Rust toolchain installed:

```powershell
./scripts/package.ps1 -Archive
```

The default output is `dist/bof2exe-VERSION-windows-x64`. Use `-Output` to choose another directory. Existing output directories and ZIP files are rejected before building.

The package contains both converter and runner executables, the greeting example, documentation, source and compiled example fixtures, project licenses, dependency metadata, dependency notices, and `SHA256SUMS` for the executables. `-Archive` also creates the ZIP and its SHA-256 sidecar.

Check `bof2exe.exe --version`, `hello.exe --help`, and `hello.exe --name Alice --count 3` in the packaged directory. The packaging script runs the greeting checks before creating an archive. Dependency-license checks run in CI; the package includes the notices available in the resolved Cargo source graph.

## Publish

If starting from this source directory without Git history, create a repository on the `main` branch and review the staged files before the first commit. Configure the intended GitHub remote, push the source, and wait for CI to pass. Release artifacts belong on a GitHub Release rather than in source history.

After a version is reviewed and CI passes, create its version tag and upload the ZIP and checksum sidecar to the corresponding GitHub Release. The Windows packaging workflow also provides a downloadable CI artifact for inspection. It does not create tags or publish releases automatically.

Generated executables must be signed after conversion if a signing workflow is used; embedding a payload changes the template's bytes. The payload hash detects corruption and does not authenticate the publisher.
