# GitHub + Release Setup

## 1) Connect local repo to GitHub

Check remote:

```powershell
git remote -v
```

If needed, set remote:

```powershell
git remote set-url origin https://github.com/<your-org-or-user>/Dictum.git
```

Authenticate (recommended with GitHub CLI):

```powershell
gh auth login
```

Push branch:

```powershell
git push -u origin main
```

## 2) Configure release workflow secrets

Repository Settings -> Secrets and variables -> Actions -> New repository secret

Add:
- `WINDOWS_CERT_BASE64`: Base64-encoded `.pfx` signing certificate
- `WINDOWS_CERT_PASSWORD`: Password for the `.pfx`

If these are not present, release artifacts are built but not Authenticode-signed.

## 3) Trigger release

Option A: Push a version tag (auto release upload):

```powershell
git tag v0.1.0
git push origin v0.1.0
```

Option B: Run manually:
- GitHub -> Actions -> `Windows Release` -> `Run workflow`

## 4) Produced artifacts

The workflow generates and uploads:
- `dictum.exe`
- `Dictum_<version>_x64-setup.exe`
- `SHA256SUMS.txt`

On tag builds, these are attached to the GitHub Release automatically.

## 5) Pre-release checklist for Dictum

Before cutting a public release, verify all of the following locally or in CI:

- `cargo check`
- `cargo test -p dictum-core`
- `cargo test -p dictum-app`
- `npm run typecheck`
- `npm run build`
- the updater default repo slug is `sinergaoptima/dictum` in both frontend and backend
- `SHA256SUMS.txt` contains entries for both the installer and `dictum.exe`
- the installer passes Authenticode verification after build

## 6) Updater smoke test checklist

Run these from the previous public installer, not only from a local dev build:

- check for updates with the default repo slug
- verify the expected installer asset is discovered
- verify the expected checksum is discovered
- verify install launches only when checksum validation succeeds
- verify invalid repo slug returns a readable error
- verify missing checksum asset blocks install
- verify checksum mismatch blocks install

## 7) Release workflow notes

- Use a canary release or manual workflow run after any signing workflow change.
- Treat unexpected workflow warnings as release blockers until they are understood or explicitly waived.
- Keep rollback notes for at least one previous public version in the GitHub release body.
